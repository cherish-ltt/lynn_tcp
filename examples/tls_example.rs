//! TLS 1.3 encrypted server-client example (requires the `tls` feature).
//!
//! Demonstrates:
//! - Generating a throwaway self-signed certificate (dev only — use real
//!   certificates issued by your CA in production)
//! - Server: `LynnServerConfigBuilder::with_tls_cert_paths(cert, key)`
//! - Client: `LynnClientConfigBuilder::with_tls(...)` with a CA trust anchor
//!   and a server name to verify against
//!
//! # Run
//!
//! ```bash
//! cargo run --example tls_example --features tls
//! ```

use std::time::Duration;

use lynn_tcp::{
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::*,
    lynn_tls::TlsClientConfigBuilder,
};
use tracing_subscriber::fmt;

async fn echo_handler(input_buf_vo: InputBufVO) -> HandlerResult {
    let payload = String::from_utf8_lossy(&input_buf_vo.get_all_bytes()).to_string();
    println!("📨 [server] received over TLS: {payload}");
    let addr = input_buf_vo.get_input_addr().unwrap();
    HandlerResult::new_with_send(1, format!("Echo: {payload}").into(), vec![addr])
}

fn write_temp_file(name: &str, contents: &str) -> String {
    let path =
        std::env::temp_dir().join(format!("lynn_tls_example_{}_{}", std::process::id(), name));
    std::fs::write(&path, contents).expect("write temp pem file");
    path.to_string_lossy().to_string()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt::init();

    println!("🚀 Starting Lynn TCP TLS 1.3 example...");
    println!();

    // ===== Step 1: generate a self-signed certificate for localhost (dev only) =====
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".to_string()])?;
    let cert_path = write_temp_file("cert.pem", &cert.pem());
    let key_path = write_temp_file("key.pem", &signing_key.serialize_pem());
    println!("🔐 Generated temporary self-signed certificate for 'localhost'");

    // ===== Step 2: start a TLS-enabled server on a free port =====
    let port = std::net::TcpListener::bind("127.0.0.1:0")?
        .local_addr()?
        .port();
    let addr = format!("127.0.0.1:{port}");
    let server_addr = addr.clone();
    let server_cert = cert_path.clone();

    let _server_handle = tokio::spawn(async move {
        let config = LynnServerConfigBuilder::new()
            .with_addr(&server_addr)
            .expect("valid addr")
            // TLS stays OFF unless configured: passing cert/key enables TLS 1.3.
            .with_tls_cert_paths(&server_cert, &key_path)
            .build();
        let _server = LynnServer::new_with_config(config)
            .await
            .add_router(1, echo_handler)
            .start()
            .await;
    });

    tokio::time::sleep(Duration::from_millis(300)).await;

    // ===== Step 3: connect a TLS client (verifies the server certificate) =====
    let client_config = LynnClientConfigBuilder::new()
        .with_server_addr(&addr)
        .expect("valid addr")
        .with_tls(
            TlsClientConfigBuilder::new()
                // Trust the self-signed certificate as CA (dev shortcut).
                .with_ca_cert_path(&cert_path)
                // Verify the server against this name (cert SAN).
                .with_server_name("localhost")
                .build(),
        )
        .build();
    let mut client = LynnClient::new_with_config(client_config)
        .await
        .start()
        .await;

    println!(
        "🔌 [client] connected (TLS 1.3) = {}",
        client.is_connected()
    );

    // ===== Step 4: exchange a message over the encrypted channel =====
    client
        .send_data(HandlerResult::new_with_send_to_server(
            1,
            "secure hello".into(),
        ))
        .await?;

    if let Some(response) = client.get_receive_data().await {
        println!(
            "📥 [client] response over TLS: {:?}",
            String::from_utf8_lossy(&response.get_all_bytes())
        );
        println!("✅ TLS round trip succeeded!");
    } else {
        println!("❌ [client] no response received");
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
    println!();
    println!("🏁 TLS example completed successfully!");

    Ok(())
}
