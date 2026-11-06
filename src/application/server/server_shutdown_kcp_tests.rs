use super::*;
use crate::{
    application::server::server_config::LynnServerConfigBuilder,
    domain::handler::request_context::RequestContext,
    domain::model::session::{SessionHandle, SessionSendOutcome},
    domain::state::app_state::AppState,
    infrastructure::kcp::{KcpConfig, Mode, stream::KcpStream},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UdpSocket,
    sync::{Notify, mpsc, oneshot},
    time::timeout,
};

struct Gate {
    entered: Notify,
    released: Notify,
    registered: mpsc::UnboundedSender<SessionHandle>,
}

async fn delayed(request: RequestContext, gate: AppState<Arc<Gate>>) -> HandlerResult {
    gate.registered.send(request.session().clone()).unwrap();
    gate.entered.notify_one();
    gate.released.notified().await;
    HandlerResult::new_with_send(1, "kcp-drained".into(), vec![request.peer_addr()]).unwrap()
}

async fn make_server(gate: Arc<Gate>) -> LynnServer<'static> {
    LynnServer::new_with_config(
        LynnServerConfigBuilder::new()
            .with_addr("127.0.0.1:0")
            .unwrap()
            .with_server_max_taskpool_size(&2)
            .with_kcp(KcpConfig::default())
            .build(),
    )
    .await
    .with_state(gate)
    .add_router(1, delayed)
}

fn start_peer(
    socket: Arc<UdpSocket>,
    addr: SocketAddr,
    conv: u32,
) -> (KcpStream, oneshot::Receiver<std::io::Result<()>>) {
    KcpStream::start(socket, addr, conv, KcpConfig::default(), Mode::Plain, None).unwrap()
}

async fn connect(addr: SocketAddr, conv: u32) -> KcpStream {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    socket.connect(addr).await.unwrap();
    let (peer, ready) = start_peer(socket, addr, conv);
    timeout(Duration::from_secs(3), ready)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    peer
}

async fn send(peer: &mut KcpStream) {
    let mut frame = 9177u16.to_le_bytes().to_vec();
    frame.extend_from_slice(&5u64.to_le_bytes());
    frame.extend_from_slice(&[1, 1, 0]);
    frame.extend_from_slice(&7719u16.to_le_bytes());
    peer.write_all(&frame).await.unwrap();
    peer.flush().await.unwrap();
}

#[tokio::test]
async fn kcp_drains_data_and_eof_while_rejecting_new_sessions() {
    let (registered, mut sessions) = mpsc::unbounded_channel();
    let gate = Arc::new(Gate {
        entered: Notify::new(),
        released: Notify::new(),
        registered,
    });
    let weak = Arc::downgrade(&gate);
    let server = make_server(gate.clone()).await;
    let tasks = server.tasks.clone();
    let ready = server.startup_handle();
    let (stop, signal) = oneshot::channel();
    let task = tokio::spawn(server.start_with_shutdown(
        async {
            let _ = signal.await;
        },
        Duration::from_secs(3),
    ));
    let addr = ready.wait().await.unwrap();
    let mut peer = connect(addr, 101).await;
    send(&mut peer).await;
    timeout(Duration::from_secs(3), gate.entered.notified())
        .await
        .unwrap();
    let session = timeout(Duration::from_secs(3), sessions.recv())
        .await
        .unwrap()
        .unwrap();
    stop.send(()).unwrap();
    tasks.draining().await;
    assert_eq!(
        session.try_send(2, "rejected".into()).unwrap(),
        SessionSendOutcome::Closed
    );
    assert!(!session.observer().is_closed());
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    socket.connect(addr).await.unwrap();
    let (rejected, mut ready) = start_peer(socket, addr, 102);
    assert!(
        timeout(Duration::from_millis(30), &mut ready)
            .await
            .is_err()
    );
    assert!(!task.is_finished());
    gate.released.notify_one();
    drop(gate);
    let mut received = Vec::new();
    timeout(Duration::from_secs(3), peer.read_to_end(&mut received))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&received[13..received.len() - 2], b"kcp-drained");
    assert_eq!(
        timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        ShutdownOutcome::Drained
    );
    assert!(session.observer().is_closed());
    assert!(weak.upgrade().is_none());
    timeout(Duration::from_secs(1), tasks.finished())
        .await
        .unwrap();
    drop(UdpSocket::bind(addr).await.unwrap());
    drop(rejected);
}

#[tokio::test]
async fn kcp_missing_data_or_eof_ack_uses_the_total_deadline() {
    for awaiting_data in [false, true] {
        let (registered, mut sessions) = mpsc::unbounded_channel();
        let gate = Arc::new(Gate {
            entered: Notify::new(),
            released: Notify::new(),
            registered,
        });
        let weak = Arc::downgrade(&gate);
        let server = make_server(gate.clone()).await;
        let tasks = server.tasks.clone();
        let ready = server.startup_handle();
        let (stop, signal) = oneshot::channel();
        let task = tokio::spawn(server.start_with_shutdown(
            async {
                let _ = signal.await;
            },
            Duration::from_millis(80),
        ));
        let addr = ready.wait().await.unwrap();
        let proxy = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let proxy_addr = proxy.local_addr().unwrap();
        let client = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let client_addr = client.local_addr().unwrap();
        client.connect(proxy_addr).await.unwrap();
        let forward = Arc::new(AtomicBool::new(true));
        let forwarding = forward.clone();
        let proxy_task = tokio::spawn(async move {
            let mut buf = [0; 65536];
            loop {
                let (len, source) = proxy.recv_from(&mut buf).await.unwrap();
                if source == addr {
                    proxy.send_to(&buf[..len], client_addr).await.unwrap();
                } else if forwarding.load(Ordering::Acquire) {
                    proxy.send_to(&buf[..len], addr).await.unwrap();
                }
            }
        });
        let (mut peer, ready) = start_peer(client, proxy_addr, 200);
        timeout(Duration::from_secs(3), ready)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        send(&mut peer).await;
        timeout(Duration::from_secs(3), gate.entered.notified())
            .await
            .unwrap();
        let session = timeout(Duration::from_secs(3), sessions.recv())
            .await
            .unwrap()
            .unwrap();
        if !awaiting_data {
            gate.released.notify_one();
            let mut response = [0; 26];
            timeout(Duration::from_secs(3), peer.read_exact(&mut response))
                .await
                .unwrap()
                .unwrap();
            // Only stop ACKs once the normal response has fully flushed on the server.
            tasks.requests_finished().await;
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        forward.store(false, Ordering::Release);
        stop.send(()).unwrap();
        tasks.draining().await;
        if awaiting_data {
            gate.released.notify_one();
        }
        drop(gate);
        assert_eq!(
            timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            ShutdownOutcome::TimedOut
        );
        assert!(session.observer().is_closed());
        assert!(weak.upgrade().is_none());
        drop(UdpSocket::bind(addr).await.unwrap());
        proxy_task.abort();
        assert!(proxy_task.await.unwrap_err().is_cancelled());
        drop(peer);
    }
}
