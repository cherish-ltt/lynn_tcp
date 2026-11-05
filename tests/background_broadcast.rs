#![cfg(feature = "server")]

use std::{net::SocketAddr, sync::Arc, time::Duration};

use lynn_tcp::{
    bytes::Bytes,
    lynn_metrics::METRICS,
    lynn_server::{BroadcastHandle, BroadcastReport, LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    task::JoinHandle,
    time::{sleep, timeout},
};

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn echo(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(1, "ok".into(), vec![input.get_input_addr().unwrap()]).unwrap()
}

async fn wait_for(condition: impl Fn() -> bool) {
    timeout(Duration::from_secs(4), async {
        while !condition() {
            sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}

fn address() -> SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

async fn start(
    config: LynnServerConfigBuilder<'static>,
) -> (
    SocketAddr,
    BroadcastHandle,
    JoinHandle<lynn_tcp::Result<()>>,
) {
    let addr = address();
    let server = LynnServer::new_with_config(
        config
            .with_addr(addr)
            .unwrap()
            .with_server_max_taskpool_size(&2)
            .with_connection_rate_limit(&0)
            .with_max_connections_per_ip(&0)
            .build(),
    )
    .await
    .add_router(1, echo);
    let handle = server.broadcast_handle();
    let task = tokio::spawn(server.start());
    wait_for(|| handle.broadcast(42, Bytes::new()).is_ok()).await;
    (addr, handle, task)
}

async fn connect(addr: SocketAddr) -> TcpStream {
    timeout(Duration::from_secs(3), TcpStream::connect(addr))
        .await
        .unwrap()
        .unwrap()
}

async fn read_frame(
    peer: &mut (impl AsyncRead + Unpin),
    method: u16,
    payload: &[u8],
    marks: (u16, u16),
) {
    let mut frame = vec![0; payload.len() + 15];
    match timeout(Duration::from_secs(4), peer.read_exact(&mut frame)).await {
        Ok(result) => result.unwrap(),
        Err(error) => panic!(
            "frame read timed out: method={method}, payload_len={}, {error}",
            payload.len()
        ),
    };
    assert_eq!(&frame[..2], &marks.0.to_le_bytes());
    assert_eq!(&frame[2..10], &(payload.len() as u64 + 5).to_le_bytes());
    assert_eq!(frame[10], 1);
    assert_eq!(&frame[11..13], &method.to_le_bytes());
    assert_eq!(&frame[13..13 + payload.len()], payload);
    assert_eq!(&frame[13 + payload.len()..], &marks.1.to_le_bytes());
}

async fn roundtrip(peer: &mut (impl AsyncRead + AsyncWrite + Unpin), marks: (u16, u16)) {
    let mut frame = marks.0.to_le_bytes().to_vec();
    frame.extend_from_slice(&5u64.to_le_bytes());
    frame.extend_from_slice(&[1, 1, 0]);
    frame.extend_from_slice(&marks.1.to_le_bytes());
    peer.write_all(&frame).await.unwrap();
    read_frame(peer, 1, b"ok", marks).await;
}

async fn first_push(handle: &BroadcastHandle, payload: Bytes) {
    timeout(Duration::from_secs(4), async {
        loop {
            let report = handle.broadcast(42, payload.clone()).unwrap();
            assert_eq!((report.full, report.closed), (0, 0));
            if report.enqueued == 1 {
                break;
            }
            assert_eq!(report.enqueued, 0);
            sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn background_push_limits_metrics_and_instance_marks() {
    let _lock = LOCK.lock().await;
    // Keep the maximum-frame check independent of Linux's fixed small send-buffer throughput.
    let (a, first, first_task) =
        start(LynnServerConfigBuilder::new().with_send_buffer_size(&0)).await;
    assert_eq!(
        first.broadcast(42, Bytes::new()).unwrap(),
        BroadcastReport::default()
    );
    let mut a1 = connect(a).await;
    first_push(&first, Bytes::new()).await;
    read_frame(&mut a1, 42, b"", (9177, 7719)).await;
    let mut a2 = connect(a).await;
    timeout(Duration::from_secs(4), async {
        loop {
            let report = first.broadcast(42, Bytes::new()).unwrap();
            assert_eq!((report.full, report.closed), (0, 0));
            read_frame(&mut a1, 42, b"", (9177, 7719)).await;
            if report.enqueued == 2 {
                break;
            }
            assert_eq!(report.enqueued, 1);
            sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
    read_frame(&mut a2, 42, b"", (9177, 7719)).await;
    let (b, second, second_task) = start(
        LynnServerConfigBuilder::new()
            .with_message_header_mark(&0xABCD)
            .with_message_tail_mark(&0xEF01),
    )
    .await;
    let mut b1 = connect(b).await;
    first_push(&second, Bytes::new()).await;
    read_frame(&mut b1, 42, b"", (0xABCD, 0xEF01)).await;
    wait_for(|| METRICS.connections.active.get() == 3.0).await;
    // A current-thread runtime has finished each writer poll before the reads above return.
    let sent = METRICS.messages.sent_total.get();
    let bytes = METRICS.network.bytes_sent_total.get();
    let received = METRICS.messages.received_total.get();
    assert_eq!(
        first.clone().broadcast(42, "push".into()).unwrap(),
        BroadcastReport {
            enqueued: 2,
            full: 0,
            closed: 0
        }
    );
    assert_eq!(second.broadcast(42, "other".into()).unwrap().enqueued, 1);
    read_frame(&mut a1, 42, b"push", (9177, 7719)).await;
    read_frame(&mut a2, 42, b"push", (9177, 7719)).await;
    read_frame(&mut b1, 42, b"other", (0xABCD, 0xEF01)).await;
    wait_for(|| METRICS.messages.sent_total.get() == sent + 3.0).await;
    assert_eq!(METRICS.network.bytes_sent_total.get(), bytes + 58.0);
    assert_eq!(METRICS.messages.received_total.get(), received);
    for len in [0, 10 * 1024 * 1024 - 5, 10 * 1024 * 1024 - 4] {
        let result = first.broadcast(42, Bytes::from(vec![7; len]));
        if len > 10 * 1024 * 1024 - 5 {
            assert!(matches!(result, Err(lynn_tcp::LynnError::Protocol(_))));
        } else {
            assert_eq!(result.unwrap().enqueued, 2);
            let payload = vec![7; len];
            read_frame(&mut a1, 42, &payload, (9177, 7719)).await;
            read_frame(&mut a2, 42, &payload, (9177, 7719)).await;
        }
    }
    roundtrip(&mut a1, (9177, 7719)).await;
    roundtrip(&mut b1, (0xABCD, 0xEF01)).await;
    first_task.abort();
    second_task.abort();
    let _ = first_task.await;
    let _ = second_task.await;
    wait_for(|| METRICS.connections.active.get() == 0.0).await;
    assert_eq!(
        timeout(Duration::from_secs(2), a1.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    assert!(
        first
            .broadcast(42, Bytes::new())
            .unwrap_err()
            .to_string()
            .contains("closed")
    );
}

#[cfg(feature = "tls")]
#[tokio::test]
async fn background_push_uses_authenticated_tls_and_owner_cancellation() {
    let _lock = LOCK.lock().await;
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let dir = std::env::temp_dir().join(format!("lynn-broadcast-tls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, signing_key.serialize_pem()).unwrap();
    let (addr, handle, task) = start(LynnServerConfigBuilder::new().with_tls(
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
    let mut peer = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect("localhost".try_into().unwrap(), connect(addr).await)
        .await
        .unwrap();
    let sent = METRICS.messages.sent_total.get();
    let bytes = METRICS.network.bytes_sent_total.get();
    first_push(&handle, "tls push".into()).await;
    read_frame(&mut peer, 42, b"tls push", (9177, 7719)).await;
    wait_for(|| METRICS.messages.sent_total.get() == sent + 1.0).await;
    assert_eq!(METRICS.network.bytes_sent_total.get(), bytes + 23.0);
    roundtrip(&mut peer, (9177, 7719)).await;
    task.abort();
    let _ = task.await;
    wait_for(|| METRICS.connections.active.get() == 0.0).await;
    assert!(handle.broadcast(42, Bytes::new()).is_err());
    match timeout(Duration::from_secs(2), peer.read(&mut [0]))
        .await
        .unwrap()
    {
        Ok(0) => {},
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
            ) => {},
        other => panic!("TLS connection survived owner cancellation: {other:?}"),
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(all(feature = "kcp", feature = "client"))]
#[tokio::test]
async fn background_push_uses_kcp_and_does_not_retain_udp_listener() {
    use lynn_tcp::{
        lynn_client::{LynnClient, LynnClientConfigBuilder},
        lynn_kcp::KcpConfig,
        lynn_tcp_dependents::InputBufVOTrait,
    };
    let _lock = LOCK.lock().await;
    let (addr, handle, task) =
        start(LynnServerConfigBuilder::new().with_kcp(KcpConfig::default())).await;
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)
        .unwrap()
        .with_kcp(KcpConfig::default())
        .with_connect_timeout_secs(&2)
        .with_reconnect_max_attempts(&1)
        .build();
    let mut peer = timeout(
        Duration::from_secs(4),
        LynnClient::new_with_config(config).await.start(),
    )
    .await
    .unwrap();
    assert!(peer.is_connected());
    first_push(&handle, "kcp push".into()).await;
    let reply = timeout(Duration::from_secs(4), peer.get_receive_data())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply.get_all_bytes().as_ref(), b"kcp push");
    peer.send_data(HandlerResult::new_with_send_to_server(1, Bytes::new()).unwrap())
        .await
        .unwrap();
    let reply = timeout(Duration::from_secs(4), peer.get_receive_data())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply.get_all_bytes().as_ref(), b"ok");
    task.abort();
    let _ = task.await;
    wait_for(|| METRICS.connections.active.get() == 0.0).await;
    assert!(handle.broadcast(42, Bytes::new()).is_err());
    let socket = tokio::net::UdpSocket::bind(addr).await.unwrap();
    drop(socket);
    drop(peer);
}

#[tokio::test]
async fn handles_do_not_retain_unstarted_running_or_failed_server_state() {
    let _lock = LOCK.lock().await;
    for running in [false, true] {
        let state = Arc::new(());
        let weak = Arc::downgrade(&state);
        let server = LynnServer::new_with_addr(address()).await.with_state(state);
        let handle = server.broadcast_handle();
        assert!(
            handle
                .broadcast(42, Bytes::new())
                .unwrap_err()
                .to_string()
                .contains("not started")
        );
        if running {
            let task = tokio::spawn(server.start());
            wait_for(|| handle.broadcast(42, Bytes::new()).is_ok()).await;
            task.abort();
            let _ = task.await;
        } else {
            drop(server);
        }
        wait_for(|| weak.upgrade().is_none()).await;
        assert!(
            handle
                .broadcast(42, Bytes::new())
                .unwrap_err()
                .to_string()
                .contains("closed")
        );
    }
    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let server = LynnServer::new_with_addr(occupied.local_addr().unwrap()).await;
    let handle = server.broadcast_handle();
    assert!(server.start().await.is_err());
    assert!(
        handle
            .broadcast(42, Bytes::new())
            .unwrap_err()
            .to_string()
            .contains("closed")
    );
}
