#![cfg(feature = "server")]

use std::{future::Future, net::SocketAddr, sync::Arc, time::Duration};

use lynn_tcp::{
    bytes::Bytes,
    lynn_middleware::{Next, RequestContext},
    lynn_server::{LynnServer, LynnServerConfigBuilder, SessionHandle, SessionSendOutcome},
    lynn_tcp_dependents::{AppState, HandlerResult},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::{Semaphore, mpsc},
    task::JoinHandle,
    time::timeout,
};

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const LIMIT: Duration = Duration::from_secs(5);

async fn register(
    request: RequestContext,
    registrations: AppState<mpsc::UnboundedSender<SessionHandle>>,
) -> HandlerResult {
    registrations.send(request.session().clone()).unwrap();
    HandlerResult::new_with_send(1, "registered".into(), vec![request.peer_addr()]).unwrap()
}

async fn start(
    config: LynnServerConfigBuilder<'static>,
) -> (
    SocketAddr,
    mpsc::UnboundedReceiver<SessionHandle>,
    JoinHandle<lynn_tcp::Result<()>>,
) {
    let (sender, registrations) = mpsc::unbounded_channel();
    let server = LynnServer::new_with_config(
        config
            .with_addr("127.0.0.1:0")
            .unwrap()
            .with_server_max_taskpool_size(&2)
            .with_connection_rate_limit(&0)
            .with_max_connections_per_ip(&0)
            .with_send_buffer_size(&0)
            .build(),
    )
    .await
    .with_state(sender)
    .add_router(1, register);
    let ready = server.startup_handle();
    let task = tokio::spawn(server.start());
    let addr = timeout(LIMIT, ready.wait()).await.unwrap().unwrap();
    (addr, registrations, task)
}

fn frame(method: u16, payload: &[u8], marks: (u16, u16)) -> Vec<u8> {
    let mut frame = marks.0.to_le_bytes().to_vec();
    frame.extend_from_slice(&(payload.len() as u64 + 5).to_le_bytes());
    frame.push(1);
    frame.extend_from_slice(&method.to_le_bytes());
    frame.extend_from_slice(payload);
    frame.extend_from_slice(&marks.1.to_le_bytes());
    frame
}

async fn read_frame(
    peer: &mut (impl AsyncRead + Unpin),
    method: u16,
    payload: &[u8],
    marks: (u16, u16),
) {
    let expected = frame(method, payload, marks);
    let mut actual = vec![0; expected.len()];
    timeout(LIMIT, peer.read_exact(&mut actual))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(actual, expected);
}

async fn assert_no_frame<T>(receive: impl Future<Output = T>) {
    assert!(
        timeout(Duration::from_millis(100), receive).await.is_err(),
        "receive completed during expected quiet window"
    );
}

async fn registered(
    peer: &mut (impl AsyncRead + AsyncWrite + Unpin),
    registrations: &mut mpsc::UnboundedReceiver<SessionHandle>,
    marks: (u16, u16),
) -> SessionHandle {
    peer.write_all(&frame(1, b"", marks)).await.unwrap();
    peer.flush().await.unwrap();
    let session = timeout(LIMIT, registrations.recv()).await.unwrap().unwrap();
    read_frame(peer, 1, b"registered", marks).await;
    session
}

#[tokio::test]
async fn targeted_push_isolated_instances_limits_and_final_close() {
    let _lock = LOCK.lock().await;
    let (addr, mut registrations, task) = start(LynnServerConfigBuilder::new()).await;
    let mut a = TcpStream::connect(addr).await.unwrap();
    let mut b = TcpStream::connect(addr).await.unwrap();
    let first = registered(&mut a, &mut registrations, (9177, 7719)).await;
    let second = registered(&mut b, &mut registrations, (9177, 7719)).await;
    let custom_marks = (0xABCD, 0x1234);
    let (other_addr, mut other_registrations, other_task) = start(
        LynnServerConfigBuilder::new()
            .with_message_header_mark(&0xABCD)
            .with_message_tail_mark(&0x1234),
    )
    .await;
    let mut other_peer = TcpStream::connect(other_addr).await.unwrap();
    let other = registered(&mut other_peer, &mut other_registrations, custom_marks).await;
    assert_ne!(first.id(), second.id());
    assert_ne!(first.id(), other.id());
    assert_eq!(first.clone().id(), first.id());
    let sent = lynn_tcp::lynn_metrics::METRICS.messages.sent_total.get();
    let bytes = lynn_tcp::lynn_metrics::METRICS
        .network
        .bytes_sent_total
        .get();
    let received = lynn_tcp::lynn_metrics::METRICS
        .messages
        .received_total
        .get();
    assert_eq!(
        first.try_send(42, "private A".into()).unwrap(),
        SessionSendOutcome::Enqueued
    );
    read_frame(&mut a, 42, b"private A", (9177, 7719)).await;
    assert_no_frame(b.read(&mut [0])).await;
    assert_eq!(
        second.try_send(43, "private B".into()).unwrap(),
        SessionSendOutcome::Enqueued
    );
    read_frame(&mut b, 43, b"private B", (9177, 7719)).await;
    assert_eq!(
        other.try_send(42, "custom".into()).unwrap(),
        SessionSendOutcome::Enqueued
    );
    read_frame(&mut other_peer, 42, b"custom", custom_marks).await;
    assert_eq!(
        lynn_tcp::lynn_metrics::METRICS.messages.sent_total.get(),
        sent + 3.0
    );
    assert_eq!(
        lynn_tcp::lynn_metrics::METRICS
            .network
            .bytes_sent_total
            .get(),
        bytes + 69.0
    );
    assert_eq!(
        lynn_tcp::lynn_metrics::METRICS
            .messages
            .received_total
            .get(),
        received
    );
    for length in [0, 10 * 1024 * 1024 - 5, 10 * 1024 * 1024 - 4] {
        let payload = Bytes::from(vec![7; length]);
        let result = first.try_send(44, payload.clone());
        if length > 10 * 1024 * 1024 - 5 {
            assert!(matches!(result, Err(lynn_tcp::LynnError::Protocol(_))));
        } else {
            assert_eq!(result.unwrap(), SessionSendOutcome::Enqueued);
            read_frame(&mut a, 44, &payload, (9177, 7719)).await;
        }
    }
    assert_eq!(
        first.try_send(45, "healthy".into()).unwrap(),
        SessionSendOutcome::Enqueued
    );
    read_frame(&mut a, 45, b"healthy", (9177, 7719)).await;
    let observer = first.observer();
    assert!(!observer.is_closed());
    // A cancelled wait must leave the retained observer usable.
    assert!(
        timeout(Duration::from_millis(20), observer.closed())
            .await
            .is_err()
    );
    let waiting = observer.clone();
    let waiter = tokio::spawn(async move { waiting.closed().await });
    a.shutdown().await.unwrap();
    assert_eq!(timeout(LIMIT, a.read(&mut [0])).await.unwrap().unwrap(), 0);
    timeout(LIMIT, observer.closed()).await.unwrap();
    timeout(LIMIT, waiter).await.unwrap().unwrap();
    assert!(observer.is_closed());
    timeout(LIMIT, observer.closed()).await.unwrap();
    assert_eq!(
        first.try_send(42, Bytes::new()).unwrap(),
        SessionSendOutcome::Closed
    );
    assert!(!second.observer().is_closed());
    task.abort();
    let _ = task.await;
    timeout(LIMIT, second.observer().closed()).await.unwrap();
    assert_eq!(
        second.try_send(42, Bytes::new()).unwrap(),
        SessionSendOutcome::Closed
    );
    other_task.abort();
    let _ = other_task.await;
    timeout(LIMIT, other.observer().closed()).await.unwrap();
}

#[tokio::test]
async fn normal_eof_waits_for_handler_middleware_return_and_response() {
    let _lock = LOCK.lock().await;
    let (registered, mut registrations) = mpsc::unbounded_channel();
    let release = Arc::new(Semaphore::new(0));
    let handler_release = release.clone();
    let after_release = Arc::new(Semaphore::new(0));
    let middleware_release = after_release.clone();
    let after_entered = Arc::new(Semaphore::new(0));
    let middleware_entered = after_entered.clone();
    let state = Arc::new(());
    let weak_state = Arc::downgrade(&state);
    let server = LynnServer::new_with_addr("127.0.0.1:0")
        .await
        .with_state(state)
        .with_middleware(move |request, next: Next| {
            let release = middleware_release.clone();
            let entered = middleware_entered.clone();
            async move {
                let result = next.run(request).await;
                entered.add_permits(1);
                release.acquire().await.unwrap().forget();
                result
            }
        })
        .add_router(1, move |request: RequestContext| {
            let registered = registered.clone();
            let release = handler_release.clone();
            async move {
                registered.send(request.session().clone()).unwrap();
                release.acquire().await.unwrap().forget();
                HandlerResult::new_with_send(1, "completed".into(), vec![request.peer_addr()])
                    .unwrap()
            }
        });
    let ready = server.startup_handle();
    let task = tokio::spawn(server.start());
    let addr = timeout(LIMIT, ready.wait()).await.unwrap().unwrap();
    let mut peer = TcpStream::connect(addr).await.unwrap();
    peer.write_all(&frame(1, b"", (9177, 7719))).await.unwrap();
    let handle = timeout(LIMIT, registrations.recv()).await.unwrap().unwrap();
    let observer = handle.observer();
    peer.shutdown().await.unwrap();
    assert!(
        timeout(Duration::from_millis(20), observer.closed())
            .await
            .is_err()
    );
    release.add_permits(1);
    timeout(LIMIT, after_entered.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert!(!observer.is_closed());
    assert!(
        timeout(Duration::from_millis(20), observer.closed())
            .await
            .is_err()
    );
    assert_no_frame(peer.read(&mut [0])).await;
    after_release.add_permits(1);
    read_frame(&mut peer, 1, b"completed", (9177, 7719)).await;
    timeout(LIMIT, observer.closed()).await.unwrap();
    assert_eq!(
        timeout(LIMIT, peer.read(&mut [0])).await.unwrap().unwrap(),
        0
    );
    task.abort();
    let _ = task.await;
    timeout(LIMIT, async {
        while weak_state.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        handle.try_send(2, Bytes::new()).unwrap(),
        SessionSendOutcome::Closed
    );
}

#[tokio::test]
async fn observers_complete_on_protocol_error_idle_timeout_and_heartbeat_eviction() {
    let _lock = LOCK.lock().await;
    for mode in 0..3 {
        let config = match mode {
            1 => LynnServerConfigBuilder::new().with_read_timeout_secs(&1),
            2 => LynnServerConfigBuilder::new()
                .with_server_check_heart_interval(&1)
                .with_server_check_heart_timeout_time(&1),
            _ => LynnServerConfigBuilder::new(),
        };
        let (addr, mut registrations, task) = start(config).await;
        let mut peer = TcpStream::connect(addr).await.unwrap();
        let handle = registered(&mut peer, &mut registrations, (9177, 7719)).await;
        if mode == 0 {
            let mut invalid = 9177_u16.to_le_bytes().to_vec();
            invalid.extend_from_slice(&0_u64.to_le_bytes());
            peer.write_all(&invalid).await.unwrap();
        }
        timeout(LIMIT, handle.observer().closed()).await.unwrap();
        assert_eq!(
            handle.try_send(2, Bytes::new()).unwrap(),
            SessionSendOutcome::Closed
        );
        assert_eq!(
            timeout(LIMIT, peer.read(&mut [0])).await.unwrap().unwrap(),
            0
        );
        task.abort();
        let _ = task.await;
    }
}

#[cfg(feature = "tls")]
#[tokio::test]
async fn tls_targeted_push_and_final_shutdown_are_observed() {
    let _lock = LOCK.lock().await;
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let dir = std::env::temp_dir().join(format!("lynn-session-tls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, signing_key.serialize_pem()).unwrap();
    let (addr, mut registrations, task) = start(LynnServerConfigBuilder::new().with_tls(
        lynn_tcp::lynn_tls::TlsServerConfig::new(
            cert_path.to_str().unwrap(),
            key_path.to_str().unwrap(),
        ),
    ))
    .await;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.der().clone()).unwrap();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let mut a = connector
        .connect(
            "localhost".try_into().unwrap(),
            TcpStream::connect(addr).await.unwrap(),
        )
        .await
        .unwrap();
    let mut b = connector
        .connect(
            "localhost".try_into().unwrap(),
            TcpStream::connect(addr).await.unwrap(),
        )
        .await
        .unwrap();
    let first = registered(&mut a, &mut registrations, (9177, 7719)).await;
    let second = registered(&mut b, &mut registrations, (9177, 7719)).await;
    assert_eq!(
        first.try_send(2, "tls A".into()).unwrap(),
        SessionSendOutcome::Enqueued
    );
    read_frame(&mut a, 2, b"tls A", (9177, 7719)).await;
    assert_no_frame(b.read(&mut [0])).await;
    assert_eq!(
        second.try_send(3, "tls B".into()).unwrap(),
        SessionSendOutcome::Enqueued
    );
    read_frame(&mut b, 3, b"tls B", (9177, 7719)).await;
    a.shutdown().await.unwrap();
    assert_eq!(timeout(LIMIT, a.read(&mut [0])).await.unwrap().unwrap(), 0);
    timeout(LIMIT, first.observer().closed()).await.unwrap();
    assert!(!second.observer().is_closed());
    task.abort();
    let _ = task.await;
    timeout(LIMIT, second.observer().closed()).await.unwrap();
    assert_eq!(
        second.try_send(4, Bytes::new()).unwrap(),
        SessionSendOutcome::Closed
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(all(feature = "kcp", feature = "client"))]
#[tokio::test]
async fn kcp_targeted_push_and_owner_cancellation_release_listener() {
    use lynn_tcp::{
        lynn_client::{LynnClient, LynnClientConfigBuilder},
        lynn_kcp::KcpConfig,
        lynn_tcp_dependents::InputBufVOTrait,
    };
    let _lock = LOCK.lock().await;
    let (addr, mut registrations, task) =
        start(LynnServerConfigBuilder::new().with_kcp(KcpConfig::default())).await;
    let mut clients = Vec::new();
    let mut handles = Vec::new();
    for _ in 0..2 {
        let config = LynnClientConfigBuilder::new()
            .with_server_addr(addr)
            .unwrap()
            .with_kcp(KcpConfig::default())
            .with_connect_timeout_secs(&2)
            .with_reconnect_max_attempts(&1)
            .build();
        let mut client = timeout(LIMIT, LynnClient::new_with_config(config).await.start())
            .await
            .unwrap();
        assert!(client.is_connected());
        client
            .send_data(HandlerResult::new_with_send_to_server(1, Bytes::new()).unwrap())
            .await
            .unwrap();
        let handle = timeout(LIMIT, registrations.recv()).await.unwrap().unwrap();
        let reply = timeout(LIMIT, client.get_receive_data())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reply.get_all_bytes().as_ref(), b"registered");
        clients.push(client);
        handles.push(handle);
    }
    assert_ne!(handles[0].id(), handles[1].id());
    for (index, payload) in ["kcp A", "kcp B"].into_iter().enumerate() {
        assert_eq!(
            handles[index].try_send(2, payload.into()).unwrap(),
            SessionSendOutcome::Enqueued
        );
        let reply = timeout(LIMIT, clients[index].get_receive_data())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reply.get_all_bytes().as_ref(), payload.as_bytes());
        if index == 0 {
            assert_no_frame(clients[1].get_receive_data()).await;
        }
    }
    task.abort();
    let _ = task.await;
    for handle in &handles {
        timeout(LIMIT, handle.observer().closed()).await.unwrap();
        assert_eq!(
            handle.try_send(3, Bytes::new()).unwrap(),
            SessionSendOutcome::Closed
        );
    }
    timeout(LIMIT, async {
        loop {
            match tokio::net::UdpSocket::bind(addr).await {
                Ok(socket) => break drop(socket),
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                    tokio::task::yield_now().await
                },
                Err(error) => panic!("listener rebind failed: {error}"),
            }
        }
    })
    .await
    .unwrap();
}
