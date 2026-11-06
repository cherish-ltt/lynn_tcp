use std::{
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, mpsc},
    task::{Context, Poll, Waker},
    time::{Duration, SystemTime},
};

use crossbeam_deque::Injector;
use dashmap::DashMap;
use tokio::{
    io::{AsyncRead, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::{RwLock, Semaphore, oneshot},
    time::timeout,
};

use crate::{
    application::server::{
        server_common::{add_client, read_connection},
        server_tasks::ServerTasks,
    },
    domain::{routing::router::LynnRouter, state::state_registry::StateRegistry},
    infrastructure::{
        connection::connection_limiter::ConnectionPermits,
        tcp::stream::{BoxedReadHalf, LynnStream},
    },
};

struct GatedReadDrop {
    inner: BoxedReadHalf,
    entered: Option<oneshot::Sender<()>>,
    release: mpsc::Receiver<()>,
}

impl AsyncRead for GatedReadDrop {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buffer)
    }
}

impl Drop for GatedReadDrop {
    fn drop(&mut self) {
        let _ = self.entered.take().unwrap().send(());
        // The controller's sender also releases this gate if an assertion panics.
        let _ = self.release.recv();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn drain_waits_for_read_half_destructor_on_another_worker() {
    let (_owner, tasks) = ServerTasks::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    let (stream, addr) = listener.accept().await.unwrap();
    drain_with_gated_reader(LynnStream::Plain(stream), addr, &tasks).await;
    drop(peer);
}

#[cfg(feature = "kcp")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn drain_keeps_kcp_listener_until_read_half_and_driver_are_destroyed() {
    use crate::infrastructure::kcp::{KcpConfig, Mode, stream::KcpStream};
    use tokio::net::UdpSocket;

    let (_owner, tasks) = ServerTasks::new();
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let peer_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let address = socket.local_addr().unwrap();
    let peer_addr = peer_socket.local_addr().unwrap();
    peer_socket.connect(address).await.unwrap();
    let (incoming, receiver) = tokio::sync::mpsc::channel(128);
    let (mut stream, ready) = KcpStream::start(
        socket.clone(),
        peer_addr,
        701,
        KcpConfig::default(),
        Mode::Plain,
        Some(receiver),
    )
    .unwrap();
    stream.track_server_worker(&tasks);
    let listener_tasks = tasks.clone();
    let listener = tokio::spawn(async move {
        let drained = async {
            listener_tasks.draining().await;
            listener_tasks.drained().await;
        };
        tokio::pin!(drained);
        let mut packet = [0; 65536];
        loop {
            tokio::select! {
                biased;
                _ = &mut drained => break,
                received = socket.recv_from(&mut packet) => {
                    let (length, _) = received.unwrap();
                    if incoming.send(packet[..length].to_vec()).await.is_err() {
                        break;
                    }
                },
            }
        }
        drop(incoming);
    });
    let (peer, peer_ready) = KcpStream::start(
        peer_socket,
        address,
        701,
        KcpConfig::default(),
        Mode::Plain,
        None,
    )
    .unwrap();
    for ready in [ready, peer_ready] {
        timeout(Duration::from_secs(3), ready)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    drain_with_gated_reader(LynnStream::Kcp(stream), peer_addr, &tasks).await;
    timeout(Duration::from_secs(3), listener)
        .await
        .unwrap()
        .unwrap();
    let mut peer_finished = peer.completion();
    drop(peer);
    timeout(Duration::from_secs(3), async {
        while peer_finished.changed().await.is_ok() {}
    })
    .await
    .unwrap();
}

#[cfg(feature = "kcp")]
#[tokio::test]
async fn genuine_kcp_listener_failure_is_recorded_before_driver_completion() {
    use crate::infrastructure::kcp::{KcpConfig, Mode, stream::KcpStream};
    use tokio::net::UdpSocket;

    let (_owner, tasks) = ServerTasks::new();
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (sender, incoming) = tokio::sync::mpsc::channel(1);
    let (mut stream, _ready) = KcpStream::start(
        socket,
        peer.local_addr().unwrap(),
        702,
        KcpConfig::default(),
        Mode::Plain,
        Some(incoming),
    )
    .unwrap();
    stream.track_server_worker(&tasks);
    let failure = stream.completion();
    tasks.begin_draining();
    drop(sender);
    timeout(Duration::from_secs(3), tasks.drained())
        .await
        .unwrap();
    assert!(tasks.has_failed());
    assert!(failure.has_changed().is_err());
    assert_eq!(failure.borrow().as_deref(), Some("KCP listener stopped"));
    drop(stream);
    timeout(Duration::from_secs(3), tasks.finished())
        .await
        .unwrap();
}

async fn drain_with_gated_reader(stream: LynnStream, addr: SocketAddr, tasks: &ServerTasks) {
    #[cfg(feature = "kcp")]
    let mut driver_error = match &stream {
        LynnStream::Kcp(stream) => Some(stream.completion()),
        _ => None,
    };
    let clients = Arc::new(DashMap::new());
    let identity = Arc::new(RwLock::new(SystemTime::now()));
    let (read, session) = add_client(
        clients.clone(),
        stream,
        addr,
        identity.clone(),
        ConnectionPermits {
            global: None,
            per_ip: None,
            observation: None,
        },
        0,
        tasks.clone(),
        (9177, 7719),
    )
    .await
    .unwrap();
    let observer = session.handle.observer();
    let (entered, destroying) = oneshot::channel();
    let (release, gate) = mpsc::channel();
    tasks.begin_draining();
    let reader = tasks.spawn(read_connection(
        Box::new(GatedReadDrop {
            inner: read,
            entered: Some(entered),
            release: gate,
        }),
        Arc::new(Semaphore::new(1)),
        addr,
        clients,
        9177,
        7719,
        Arc::new(LynnRouter::new()),
        Arc::new(Injector::new()),
        identity,
        Arc::new(StateRegistry::new()),
        session,
        1,
        0,
    ));
    timeout(Duration::from_secs(3), destroying)
        .await
        .unwrap()
        .unwrap();
    let prematurely_drained = {
        let mut drained = std::pin::pin!(tasks.drained());
        drained
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_ready()
    };
    assert!(!observer.is_closed());
    #[cfg(feature = "kcp")]
    if prematurely_drained && let Some(error) = driver_error.as_mut() {
        timeout(Duration::from_secs(3), async {
            while error.borrow().is_none() {
                error.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert_eq!(error.borrow().as_deref(), Some("KCP listener stopped"));
    }
    release.send(()).unwrap();
    timeout(Duration::from_secs(3), reader)
        .await
        .unwrap()
        .unwrap();
    timeout(Duration::from_secs(3), tasks.finished())
        .await
        .unwrap();
    assert!(observer.is_closed());
    assert!(
        !prematurely_drained,
        "drain barrier completed before read-half destruction"
    );
    assert!(!tasks.has_failed());
}
