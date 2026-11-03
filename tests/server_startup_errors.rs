#![cfg(feature = "server")]

use std::time::Duration;

use lynn_tcp::lynn_server::LynnServer;
use tokio::{net::TcpListener, time::timeout};

#[tokio::test]
async fn occupied_port_returns_bind_error_to_caller() {
    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = occupied.local_addr().unwrap();
    let expected = TcpListener::bind(addr).await.unwrap_err();
    assert_eq!(expected.kind(), std::io::ErrorKind::AddrInUse);
    let server = LynnServer::new_with_addr(addr).await;
    let completion = timeout(Duration::from_secs(2), tokio::spawn(server.start()))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(
        completion,
        Err(lynn_tcp::LynnError::Io(error)) if error.kind() == std::io::ErrorKind::AddrInUse
    ));
}

#[cfg(feature = "tls")]
#[tokio::test]
async fn invalid_tls_config_returns_error_to_caller() {
    use lynn_tcp::{lynn_server::LynnServerConfigBuilder, lynn_tls::TlsServerConfig};

    let config = LynnServerConfigBuilder::new()
        .with_addr("127.0.0.1:0")
        .unwrap()
        .with_tls(TlsServerConfig::new(
            "/nonexistent/lynn_tcp/server.pem",
            "/nonexistent/lynn_tcp/server.key",
        ))
        .build();
    let completion = timeout(
        Duration::from_secs(2),
        tokio::spawn(LynnServer::new_with_config(config).await.start()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(matches!(completion, Err(lynn_tcp::LynnError::Tls(_))));
}
