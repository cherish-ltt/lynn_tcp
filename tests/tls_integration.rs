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

// ── helpers ─────────────────────────────────────────────────────────────

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
        .await;
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

// ── tests ───────────────────────────────────────────────────────────────

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
            .await;
    });

    // start() logs the error and returns; nothing must be listening.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let connect = TcpStream::connect(format!("127.0.0.1:{port}")).await;
    assert!(
        connect.is_err(),
        "server with invalid TLS config must not listen"
    );
    server.abort();
}
