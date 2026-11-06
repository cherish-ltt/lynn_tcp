use super::{
    Certificates, KcpConfig, KcpStream, Mode,
    frames::{connect, frame},
};
use crate::{
    application::server::server_tasks::ServerTasks,
    infrastructure::{
        connection::connection_limiter::ConnectionLimiter,
        kcp::listener::KcpListener,
        tcp::stream::StreamAcceptor,
        tls::tls_provider::{ClientTls, build_client_tls_mode, build_server_acceptor_mode},
    },
    lynn_middleware::RequestContext,
    lynn_server::{
        LynnServer, LynnServerConfigBuilder, SessionHandle, SessionSendOutcome, ShutdownOutcome,
    },
    lynn_state::AppState,
    lynn_tcp_dependents::HandlerResult,
    lynn_tls::{TlsClientConfig, TlsServerConfig},
};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UdpSocket,
    sync::{Notify, mpsc, oneshot},
    time::timeout,
};

const LIMIT: Duration = Duration::from_secs(3);

struct Gate {
    registered: mpsc::UnboundedSender<SessionHandle>,
    released: Notify,
}

fn connector(certs: &Certificates) -> ClientTls {
    build_client_tls_mode(
        &TlsClientConfig {
            ca_cert_path: Some(certs.ca.clone()),
            client_cert_path: Some(certs.client_cert.clone()),
            client_key_path: Some(certs.client_key.clone()),
            ..Default::default()
        },
        "127.0.0.1:1",
        true,
    )
    .unwrap()
}

async fn server(certs: &Certificates, gate: Arc<Gate>) -> LynnServer<'static> {
    async fn delayed(request: RequestContext, gate: AppState<Arc<Gate>>) -> HandlerResult {
        gate.registered.send(request.session().clone()).unwrap();
        gate.released.notified().await;
        HandlerResult::new_with_send(1, "drained response".into(), vec![request.peer_addr()])
            .unwrap()
    }
    LynnServer::new_with_config(
        LynnServerConfigBuilder::new()
            .with_addr("127.0.0.1:0")
            .unwrap()
            .with_kcp(KcpConfig::default())
            .with_tls(
                TlsServerConfig::new(&certs.server_cert, &certs.server_key)
                    .with_client_ca(&certs.ca),
            )
            .with_server_max_taskpool_size(&2)
            .with_server_max_connections(Some(&2))
            .with_max_connections_per_ip(&2)
            .with_connection_rate_limit(&0)
            .build(),
    )
    .await
    .with_state(gate)
    .add_router(1, delayed)
}

#[tokio::test]
async fn mtls_drain_finishes_pushes_response_and_close_while_cancelling_half_handshake() {
    let certs = Certificates::new();
    let (registered, mut registrations) = mpsc::unbounded_channel();
    let gate = Arc::new(Gate {
        registered,
        released: Notify::new(),
    });
    let weak = Arc::downgrade(&gate);
    let server = server(&certs, gate.clone()).await;
    let ready = server.startup_handle();
    let broadcast = server.broadcast_handle();
    let (stop, signal) = oneshot::channel();
    let (observed, draining) = oneshot::channel();
    let task = tokio::spawn(server.start_with_shutdown(
        async {
            signal.await.unwrap();
            // On this current-thread runtime, the drain boundary executes before the receiver runs.
            observed.send(()).unwrap();
        },
        LIMIT,
    ));
    let addr = timeout(LIMIT, ready.wait()).await.unwrap().unwrap();
    let mut peer = connect(addr, &connector(&certs)).await;
    timeout(LIMIT, async {
        peer.write_all(&frame(b"register")).await.unwrap();
        peer.flush().await.unwrap();
    })
    .await
    .unwrap();
    let session = timeout(LIMIT, registrations.recv()).await.unwrap().unwrap();

    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    socket.connect(addr).await.unwrap();
    let (pending, ready) =
        KcpStream::start(socket, addr, 61, KcpConfig::default(), Mode::Tls, None).unwrap();
    timeout(LIMIT, ready).await.unwrap().unwrap().unwrap();
    assert_eq!(
        session.try_send(1, "private before".into()).unwrap(),
        SessionSendOutcome::Enqueued
    );
    assert_eq!(
        broadcast
            .broadcast(1, "broadcast before".into())
            .unwrap()
            .enqueued,
        1
    );
    stop.send(()).unwrap();
    timeout(LIMIT, draining).await.unwrap().unwrap();
    assert_eq!(
        session.try_send(1, "after boundary".into()).unwrap(),
        SessionSendOutcome::Closed
    );
    assert!(!session.observer().is_closed());
    assert!(broadcast.broadcast(1, "after boundary".into()).is_err());
    assert!(!task.is_finished());
    gate.released.notify_one();
    drop(gate);

    let mut received = Vec::new();
    timeout(LIMIT, peer.read_to_end(&mut received))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        received,
        [
            frame(b"private before"),
            frame(b"broadcast before"),
            frame(b"drained response")
        ]
        .concat()
    );
    assert_eq!(
        timeout(LIMIT, task).await.unwrap().unwrap().unwrap(),
        ShutdownOutcome::Drained
    );
    assert!(session.observer().is_closed());
    assert!(weak.upgrade().is_none());
    drop(UdpSocket::bind(addr).await.unwrap());
    drop((peer, pending));
}

#[tokio::test]
async fn mtls_missing_response_ack_times_out_and_releases_session_before_returning() {
    let certs = Certificates::new();
    let (registered, mut registrations) = mpsc::unbounded_channel();
    let gate = Arc::new(Gate {
        registered,
        released: Notify::new(),
    });
    let weak = Arc::downgrade(&gate);
    let server = server(&certs, gate.clone()).await;
    let ready = server.startup_handle();
    let (stop, signal) = oneshot::channel();
    let (observed, draining) = oneshot::channel();
    let task = tokio::spawn(server.start_with_shutdown(
        async {
            signal.await.unwrap();
            observed.send(()).unwrap();
        },
        Duration::from_millis(250),
    ));
    let addr = timeout(LIMIT, ready.wait()).await.unwrap().unwrap();
    let proxy = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = proxy.local_addr().unwrap();
    let forward = Arc::new(AtomicBool::new(true));
    let forwarding = forward.clone();
    let relay = tokio::spawn(async move {
        let mut buffer = [0; 1400];
        let mut client = None;
        loop {
            let (length, source) = proxy.recv_from(&mut buffer).await.unwrap();
            if source == addr {
                proxy
                    .send_to(&buffer[..length], client.unwrap())
                    .await
                    .unwrap();
            } else {
                client = Some(source);
                if forwarding.load(Ordering::Acquire) {
                    proxy.send_to(&buffer[..length], addr).await.unwrap();
                }
            }
        }
    });
    let mut peer = connect(proxy_addr, &connector(&certs)).await;
    timeout(LIMIT, async {
        peer.write_all(&frame(b"register")).await.unwrap();
        peer.flush().await.unwrap();
    })
    .await
    .unwrap();
    let session = timeout(LIMIT, registrations.recv()).await.unwrap().unwrap();
    forward.store(false, Ordering::Release);
    gate.released.notify_one();
    drop(gate);
    let expected = frame(b"drained response");
    let mut received = vec![0; expected.len()];
    timeout(LIMIT, peer.read_exact(&mut received))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received, expected);
    stop.send(()).unwrap();
    timeout(LIMIT, draining).await.unwrap().unwrap();
    assert_eq!(
        session.try_send(1, "after boundary".into()).unwrap(),
        SessionSendOutcome::Closed
    );
    assert!(!session.observer().is_closed());
    assert_eq!(
        timeout(LIMIT, task).await.unwrap().unwrap().unwrap(),
        ShutdownOutcome::TimedOut
    );
    assert!(session.observer().is_closed());
    assert!(weak.upgrade().is_none());
    drop(UdpSocket::bind(addr).await.unwrap());
    relay.abort();
    assert!(relay.await.unwrap_err().is_cancelled());
    drop(peer);
}

#[tokio::test]
async fn draining_without_owner_cancellation_reclaims_unauthenticated_driver_and_permits() {
    let certs = Certificates::new();
    let listener = KcpListener::bind("127.0.0.1:0", KcpConfig::default(), Mode::Tls)
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let acceptor = build_server_acceptor_mode(
        &TlsServerConfig::new(&certs.server_cert, &certs.server_key).with_client_ca(&certs.ca),
        true,
    )
    .unwrap();
    let (_owner, tasks) = ServerTasks::new();
    let queue = Arc::new(crossbeam_deque::Injector::new());
    let limiter = Arc::new(ConnectionLimiter::new(0, 1));
    let task = tokio::spawn(listener.run(
        Some(1),
        Some(limiter.clone()),
        queue.clone(),
        Arc::new(StreamAcceptor::KcpTls(Arc::new(acceptor))),
        tasks.clone(),
        1,
    ));
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    socket.connect(addr).await.unwrap();
    let (peer, ready) =
        KcpStream::start(socket, addr, 62, KcpConfig::default(), Mode::Tls, None).unwrap();
    timeout(LIMIT, ready).await.unwrap().unwrap().unwrap();
    assert!(limiter.acquire(addr.ip()).await.is_none());
    assert!(queue.is_empty());
    tasks.begin_draining();
    timeout(LIMIT, tasks.drained()).await.unwrap();
    timeout(LIMIT, tasks.finished()).await.unwrap();
    assert!(limiter.acquire(addr.ip()).await.is_some());
    assert!(queue.is_empty());
    assert!(!tasks.has_failed());
    assert!(!task.is_finished());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    drop(UdpSocket::bind(addr).await.unwrap());
    drop(peer);
}
