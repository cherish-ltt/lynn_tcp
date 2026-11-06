#![cfg(feature = "server")]

use std::{sync::Arc, time::Duration};

use lynn_tcp::{
    lynn_server::{LynnServer, LynnServerConfigBuilder, ServerReady, ServerStartupError},
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

async fn address(ready: ServerReady) -> std::net::SocketAddr {
    let addr = timeout(Duration::from_secs(3), ready.wait())
        .await
        .unwrap()
        .unwrap();
    assert_ne!(addr.port(), 0);
    addr
}

async fn echo(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(1, "ok".into(), vec![input.get_input_addr().unwrap()]).unwrap()
}

async fn roundtrip(mut peer: impl AsyncRead + AsyncWrite + Unpin) {
    let mut frame = 9177u16.to_le_bytes().to_vec();
    frame.extend_from_slice(&5u64.to_le_bytes());
    frame.extend_from_slice(&[1, 1, 0]);
    frame.extend_from_slice(&7719u16.to_le_bytes());
    peer.write_all(&frame).await.unwrap();
    peer.flush().await.unwrap();
    let mut response = [0; 17];
    timeout(Duration::from_secs(3), peer.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response[13..15], b"ok");
}

#[tokio::test]
async fn startup_returns_the_bound_address() {
    let server = LynnServer::new_with_addr("127.0.0.1:0")
        .await
        .add_router(1, echo);
    let ready = server.startup_handle();
    let late = ready.clone();
    let task = tokio::spawn(server.start());
    let addr = address(ready).await;
    roundtrip(TcpStream::connect(addr).await.unwrap()).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_eq!(address(late).await, addr);
}

#[tokio::test]
async fn dropping_observers_does_not_stop_or_mix_servers() {
    let first = LynnServer::new_with_addr("127.0.0.1:0")
        .await
        .add_router(1, echo);
    let second = LynnServer::new_with_addr("127.0.0.1:0")
        .await
        .add_router(1, echo);
    let ready_first = first.startup_handle();
    drop(first.startup_handle());
    let ready_second = second.startup_handle();
    let task_first = tokio::spawn(first.start());
    let task_second = tokio::spawn(second.start());
    let (first_addr, second_addr) = tokio::join!(address(ready_first), address(ready_second));
    assert_ne!(first_addr, second_addr);
    roundtrip(TcpStream::connect(first_addr).await.unwrap()).await;
    roundtrip(TcpStream::connect(second_addr).await.unwrap()).await;
    task_first.abort();
    task_second.abort();
    assert!(task_first.await.unwrap_err().is_cancelled());
    assert!(task_second.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn unstarted_and_unpolled_owners_cancel_observers_and_release_state() {
    for create_start_future in [false, true] {
        let state = Arc::new(());
        let weak = Arc::downgrade(&state);
        let server = LynnServer::new_with_addr("127.0.0.1:0")
            .await
            .with_state_arc(state);
        let ready = server.startup_handle();
        if create_start_future {
            drop(server.start());
        } else {
            drop(server);
        }
        assert_eq!(
            timeout(Duration::from_secs(2), ready.wait()).await.unwrap(),
            Err(ServerStartupError::Cancelled)
        );
        timeout(Duration::from_secs(2), async {
            while weak.upgrade().is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn owner_drop_wakes_pending_observers() {
    let server = LynnServer::new_with_addr("127.0.0.1:0").await;
    let waiter = tokio::spawn(server.startup_handle().wait());
    tokio::task::yield_now().await;
    assert!(!waiter.is_finished());
    drop(server);
    assert_eq!(
        timeout(Duration::from_secs(2), waiter)
            .await
            .unwrap()
            .unwrap(),
        Err(ServerStartupError::Cancelled)
    );
}

#[tokio::test]
async fn failures_reach_late_observers_without_erasing_original_errors() {
    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = LynnServer::new_with_addr(occupied.local_addr().unwrap()).await;
    let ready = server.startup_handle();
    assert!(
        matches!(server.start().await, Err(lynn_tcp::LynnError::Io(error))
        if error.kind() == std::io::ErrorKind::AddrInUse)
    );
    assert_eq!(ready.wait().await, Err(ServerStartupError::Failed));

    let config = LynnServerConfigBuilder::new()
        .with_addr("127.0.0.1:0")
        .unwrap()
        .with_server_max_taskpool_size(&0)
        .build();
    let server = LynnServer::new_with_config(config).await;
    let ready = server.startup_handle();
    assert!(matches!(
        server.start().await,
        Err(lynn_tcp::LynnError::Config(_))
    ));
    assert_eq!(ready.wait().await, Err(ServerStartupError::Failed));
}

#[cfg(feature = "tls")]
#[tokio::test]
async fn tls_is_initialized_before_readiness() {
    use lynn_tcp::lynn_tls::TlsServerConfig;
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let base = std::env::temp_dir().join(format!("lynn-ready-{}", std::process::id()));
    let cert_path = base.with_extension("pem");
    let key_path = base.with_extension("key");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, signing_key.serialize_pem()).unwrap();
    let config = LynnServerConfigBuilder::new()
        .with_addr("127.0.0.1:0")
        .unwrap()
        .with_tls(TlsServerConfig::new(
            cert_path.to_str().unwrap(),
            key_path.to_str().unwrap(),
        ))
        .build();
    let server = LynnServer::new_with_config(config)
        .await
        .add_router(1, echo);
    let ready = server.startup_handle();
    let task = tokio::spawn(server.start());
    let addr = address(ready).await;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.der().clone()).unwrap();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let peer = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect(
            "localhost".try_into().unwrap(),
            TcpStream::connect(addr).await.unwrap(),
        )
        .await
        .unwrap();
    roundtrip(peer).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    std::fs::remove_file(cert_path).unwrap();
    std::fs::remove_file(key_path).unwrap();
}

#[cfg(feature = "tls")]
#[tokio::test]
async fn invalid_tls_never_reports_ready() {
    let config = LynnServerConfigBuilder::new()
        .with_addr("127.0.0.1:0")
        .unwrap()
        .with_tls_cert_paths("/absent/lynn-ready.pem", "/absent/lynn-ready.key")
        .build();
    let server = LynnServer::new_with_config(config).await;
    let ready = server.startup_handle();
    assert!(matches!(
        server.start().await,
        Err(lynn_tcp::LynnError::Tls(_))
    ));
    assert_eq!(ready.wait().await, Err(ServerStartupError::Failed));
}

#[cfg(feature = "kcp")]
#[tokio::test]
async fn invalid_kcp_never_reports_ready() {
    let config = LynnServerConfigBuilder::new()
        .with_addr("127.0.0.1:0")
        .unwrap()
        .with_kcp(lynn_tcp::lynn_kcp::KcpConfig {
            protocol: lynn_tcp::lynn_kcp::KcpOptions::stream_mode(),
            ..Default::default()
        })
        .build();
    let server = LynnServer::new_with_config(config).await;
    let ready = server.startup_handle();
    assert!(matches!(
        server.start().await,
        Err(lynn_tcp::LynnError::Config(_))
    ));
    assert_eq!(ready.wait().await, Err(ServerStartupError::Failed));

    let occupied = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let config = LynnServerConfigBuilder::new()
        .with_addr(occupied.local_addr().unwrap())
        .unwrap()
        .with_kcp(lynn_tcp::lynn_kcp::KcpConfig::default())
        .build();
    let server = LynnServer::new_with_config(config).await;
    let ready = server.startup_handle();
    assert!(
        matches!(server.start().await, Err(lynn_tcp::LynnError::Io(error))
        if error.kind() == std::io::ErrorKind::AddrInUse)
    );
    assert_eq!(ready.wait().await, Err(ServerStartupError::Failed));
}

#[cfg(all(feature = "kcp", feature = "client"))]
#[tokio::test]
async fn kcp_readiness_returns_the_udp_address() {
    use lynn_tcp::{
        lynn_client::{LynnClient, LynnClientConfigBuilder},
        lynn_kcp::KcpConfig,
        lynn_tcp_dependents::InputBufVOTrait,
    };
    let config = LynnServerConfigBuilder::new()
        .with_addr("127.0.0.1:0")
        .unwrap()
        .with_kcp(KcpConfig::default())
        .build();
    let server = LynnServer::new_with_config(config)
        .await
        .add_router(1, echo);
    let ready = server.startup_handle();
    let task = tokio::spawn(server.start());
    let addr = address(ready).await;
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)
        .unwrap()
        .with_reconnect_max_attempts(&1)
        .with_kcp(KcpConfig::default())
        .build();
    let mut client = timeout(
        Duration::from_secs(3),
        LynnClient::new_with_config(config).await.start(),
    )
    .await
    .unwrap();
    assert!(client.is_connected());
    client
        .send_data(HandlerResult::new_with_send_to_server(1, "hello".into()).unwrap())
        .await
        .unwrap();
    let response = timeout(Duration::from_secs(3), client.get_receive_data())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.get_all_bytes(), &b"ok"[..]);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}
