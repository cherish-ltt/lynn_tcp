//! Integration tests: TLS 1.3 transport (feature = `tls`).
//!
//! Uses `rcgen` to generate a throwaway self-signed certificate for
//! "localhost" and runs full LynnServer ↔ LynnClient round trips over TLS.

#![cfg(feature = "tls")]

use std::time::Duration;

use lynn_tcp::{
    bytes::Bytes,
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
    lynn_tls::{TlsClientConfig, TlsClientConfigBuilder, TlsServerConfig},
    tokio::{io::AsyncReadExt, net::TcpStream, time::timeout},
};

// helpers

/// Writes `contents` to a uniquely named temp file and returns its path.
fn temp_file(name: &str, contents: &str) -> String {
    let path =
        std::env::temp_dir().join(format!("lynn_tcp_tls_test_{}_{}", std::process::id(), name));
    std::fs::write(&path, contents).expect("write temp file");
    path.to_string_lossy().to_string()
}

/// Generates a self-signed certificate for `localhost` and returns the
/// paths of the written PEM files: (cert, key). `label` keeps parallel
/// tests from racing on the same temp files.
fn generate_certs(label: &str) -> (String, String) {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
            .expect("generate certificate");
    let cert_path = temp_file(&format!("{label}_server_cert.pem"), &cert.pem());
    let key_path = temp_file(
        &format!("{label}_server_key.pem"),
        &signing_key.serialize_pem(),
    );
    (cert_path, key_path)
}

async fn echo_handler(input: InputBufVO) -> HandlerResult {
    let payload = String::from_utf8_lossy(&input.get_all_bytes()).to_string();
    HandlerResult::new_with_send(
        1,
        format!("Echo: {payload}").into(),
        vec![input.get_input_addr().unwrap()],
    )
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind probe")
        .local_addr()
        .unwrap()
        .port()
}

async fn spawn_tls_server(addr: String, cert_path: String, key_path: String) {
    let config = LynnServerConfigBuilder::new()
        .with_addr(&addr)
        .expect("addr")
        .with_tls(TlsServerConfig::new(&cert_path, &key_path))
        .build();
    LynnServer::new_with_config(config)
        .await
        .add_router(1, echo_handler)
        .start()
        .await
        .unwrap();
}

async fn wait_server(addr: &str) {
    for _ in 0..120 {
        if TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never became reachable at {addr}");
}

// tests

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_roundtrip_with_configured_certificates() {
    let _ = lynn_tcp::tracing_subscriber::fmt::try_init();
    let (cert_path, key_path) = generate_certs("roundtrip");
    let port = free_port();
    let addr = format!("127.0.0.1:{port}");

    tokio::spawn(spawn_tls_server(
        addr.clone(),
        cert_path.clone(),
        key_path.clone(),
    ));
    wait_server(&addr).await;

    // A plain client must NOT be able to talk to the TLS server.
    let mut plain = TcpStream::connect(&addr).await.expect("plain connect");
    let mut probe = [0u8; 1];
    let plain_closed = timeout(Duration::from_secs(3), plain.read(&mut probe)).await;
    if let Ok(Ok(n)) = plain_closed {
        assert_eq!(n, 0, "plain client must hit EOF (no plaintext echo)");
    }

    let client_config = LynnClientConfigBuilder::new()
        .with_server_addr(&addr)
        .expect("addr")
        .with_tls(
            TlsClientConfigBuilder::new()
                .with_ca_cert_path(&cert_path)
                .with_server_name("localhost")
                .build(),
        )
        .build();
    let mut client = LynnClient::new_with_config(client_config)
        .await
        .start()
        .await;

    client
        .send_data(HandlerResult::new_with_send_to_server(
            1,
            Bytes::from("secure hello"),
        ))
        .await
        .expect("send over TLS");

    let response = timeout(Duration::from_secs(5), client.get_receive_data())
        .await
        .expect("timeout waiting for TLS response")
        .expect("TLS connection closed");
    assert_eq!(
        String::from_utf8_lossy(&response.get_all_bytes()),
        "Echo: secure hello"
    );
}

#[tokio::test]
async fn failed_tls_handshakes_do_not_exhaust_per_ip_quota() {
    use tokio::io::AsyncWriteExt;
    let (cert_path, key_path) = generate_certs("quota");
    let addr = format!("127.0.0.1:{}", free_port());
    let config = LynnServerConfigBuilder::new()
        .with_addr(&addr)
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .with_max_connections_per_ip(&1)
        .with_connection_rate_limit(&0)
        .with_tls(TlsServerConfig::new(&cert_path, &key_path))
        .build();
    tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo_handler)
            .start(),
    );
    tokio::time::sleep(Duration::from_millis(100)).await;
    for _ in 0..3 {
        let mut invalid = TcpStream::connect(&addr).await.unwrap();
        invalid.write_all(b"not a TLS record").await.unwrap();
        let mut alert = Vec::new();
        let result = timeout(Duration::from_secs(1), invalid.read_to_end(&mut alert))
            .await
            .unwrap();
        assert!(
            result.is_ok() || result.unwrap_err().kind() == std::io::ErrorKind::ConnectionReset
        );
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(&addr)
        .unwrap()
        .with_tls(
            TlsClientConfigBuilder::new()
                .with_ca_cert_path(&cert_path)
                .with_server_name("localhost")
                .build(),
        )
        .build();
    let mut client = LynnClient::new_with_config(config).await.start().await;
    client
        .send_data(HandlerResult::new_with_send_to_server(
            1,
            Bytes::from("after failures"),
        ))
        .await
        .unwrap();
    let response = timeout(Duration::from_secs(2), client.get_receive_data())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response.get_all_bytes()[..], b"Echo: after failures");
    std::fs::remove_file(cert_path).unwrap();
    std::fs::remove_file(key_path).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ipv6_tls_roundtrip_verifies_ip_san_without_name_override() {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["::1".to_string()]).unwrap();
    let cert_path = temp_file("ipv6_cert.pem", &cert.pem());
    let key_path = temp_file("ipv6_key.pem", &signing_key.serialize_pem());
    let probe = std::net::TcpListener::bind("[::1]:0").expect("IPv6 loopback available");
    let addr = probe.local_addr().unwrap().to_string();
    drop(probe);
    let server = tokio::spawn(spawn_tls_server(
        addr.clone(),
        cert_path.clone(),
        key_path.clone(),
    ));
    wait_server(&addr).await;

    let config = LynnClientConfigBuilder::new()
        .with_server_addr(&addr)
        .unwrap()
        .with_tls(
            TlsClientConfigBuilder::new()
                .with_ca_cert_path(&cert_path)
                .build(),
        )
        .build();
    let mut client = LynnClient::new_with_config(config).await.start().await;
    client
        .send_data(HandlerResult::new_with_send_to_server(
            1,
            Bytes::from_static(b"IPv6 verified"),
        ))
        .await
        .unwrap();
    let response = timeout(Duration::from_secs(5), client.get_receive_data())
        .await
        .unwrap()
        .expect("IPv6 TLS connection must remain open");
    assert_eq!(response.get_all_bytes(), b"Echo: IPv6 verified"[..]);

    let wrong_name = LynnClientConfigBuilder::new()
        .with_server_addr(&addr)
        .unwrap()
        .with_reconnect_max_attempts(&1)
        .with_tls(
            TlsClientConfigBuilder::new()
                .with_ca_cert_path(&cert_path)
                .with_server_name("localhost")
                .build(),
        )
        .build();
    let mut rejected = LynnClient::new_with_config(wrong_name).await.start().await;
    assert!(
        timeout(Duration::from_secs(2), rejected.get_receive_data())
            .await
            .unwrap()
            .is_none()
    );
    server.abort();
    std::fs::remove_file(cert_path).unwrap();
    std::fs::remove_file(key_path).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_client_without_trust_anchor_fails_to_start() {
    let (cert_path, key_path) = generate_certs("no_ca");
    let port = free_port();
    let addr = format!("127.0.0.1:{port}");

    tokio::spawn(spawn_tls_server(addr.clone(), cert_path, key_path));
    wait_server(&addr).await;

    // TLS enabled but no CA configured: start() must report failure and the
    // client must remain unusable.
    let client_config = LynnClientConfigBuilder::new()
        .with_server_addr(&addr)
        .expect("addr")
        .with_tls(TlsClientConfig::default())
        .build();
    let mut client = LynnClient::new_with_config(client_config)
        .await
        .start()
        .await;

    let err = client
        .send_data(HandlerResult::new_with_send_to_server(1, Bytes::new()))
        .await;
    assert!(err.is_err(), "client without trust anchor must not connect");
}

#[tokio::test]
async fn tls_read_timeout_closes_idle_authenticated_connection() {
    let (cert_path, key_path) = generate_certs("read_timeout");
    let addr = format!("127.0.0.1:{}", free_port());
    let config = LynnServerConfigBuilder::new()
        .with_addr(&addr)
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .with_read_timeout_secs(&1)
        .with_tls(TlsServerConfig::new(&cert_path, &key_path))
        .build();
    tokio::spawn(LynnServer::new_with_config(config).await.start());
    wait_server(&addr).await;

    let cert_pem = std::fs::read(&cert_path).unwrap();
    let mut roots = rustls::RootCertStore::empty();
    for cert in rustls_pemfile::certs(&mut cert_pem.as_slice()) {
        roots.add(cert.unwrap()).unwrap();
    }
    let client = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(client));
    let mut socket = connector
        .connect(
            "localhost".try_into().unwrap(),
            TcpStream::connect(&addr).await.unwrap(),
        )
        .await
        .unwrap();
    let result = timeout(Duration::from_millis(1800), socket.read(&mut [0]))
        .await
        .unwrap();
    assert!(
        matches!(result, Ok(0)) || result.unwrap_err().kind() == std::io::ErrorKind::UnexpectedEof
    );
    std::fs::remove_file(cert_path).unwrap();
    std::fs::remove_file(key_path).unwrap();
}

#[tokio::test]
async fn tls_server_with_missing_cert_files_fails_to_start() {
    let port = free_port();
    let addr = format!("127.0.0.1:{port}");

    let server = tokio::spawn(async move {
        let config = LynnServerConfigBuilder::new()
            .with_addr(&addr)
            .expect("addr")
            .with_tls(TlsServerConfig::new(
                "/nonexistent/cert.pem",
                "/nonexistent/key.pem",
            ))
            .build();
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo_handler)
            .start()
            .await
    });

    let result = timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(result, Err(lynn_tcp::LynnError::Tls(_))));
    let connect = TcpStream::connect(format!("127.0.0.1:{port}")).await;
    assert!(
        connect.is_err(),
        "server with invalid TLS config must not listen"
    );
}
