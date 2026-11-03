#![cfg(all(feature = "server", feature = "tls"))]

use lynn_tcp::{
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
    lynn_tls::TlsServerConfig,
};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{sleep, timeout},
};

struct Certificate {
    directory: PathBuf,
    cert: String,
    key: String,
    connector: tokio_rustls::TlsConnector,
}
impl Certificate {
    fn new(label: &str) -> Self {
        let directory =
            std::env::temp_dir().join(format!("lynn-handshake-{label}-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let rcgen::CertifiedKey { cert, signing_key } =
            rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert_path = directory.join("cert.pem");
        let key_path = directory.join("key.pem");
        std::fs::write(&cert_path, cert.pem()).unwrap();
        std::fs::write(&key_path, signing_key.serialize_pem()).unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.add(cert.der().clone()).unwrap();
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        Self {
            directory,
            cert: cert_path.to_string_lossy().into(),
            key: key_path.to_string_lossy().into(),
            connector: tokio_rustls::TlsConnector::from(Arc::new(config)),
        }
    }
}
impl Drop for Certificate {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}
async fn echo(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(1, "ok".into(), vec![input.get_input_addr().unwrap()])
}
fn frame() -> Vec<u8> {
    let mut frame = 9177u16.to_le_bytes().to_vec();
    frame.extend_from_slice(&5u64.to_le_bytes());
    frame.extend_from_slice(&[1, 1, 0]);
    frame.extend_from_slice(&7719u16.to_le_bytes());
    frame
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn incomplete_handshakes_do_not_block_active_requests() {
    for workers in [&1, &2] {
        active_requests_survive_handshakes(workers).await;
    }
}

async fn active_requests_survive_handshakes(workers: &'static usize) {
    let cert = Certificate::new("active");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_server_max_taskpool_size(workers)
        .with_server_check_heart_timeout_time(&60)
        .with_tls(TlsServerConfig::new(&cert.cert, &cert.key))
        .build();
    let server = tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo)
            .start(),
    );
    let raw = timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(socket) = TcpStream::connect(addr).await {
                break socket;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let mut active = cert
        .connector
        .connect(
            rustls::pki_types::ServerName::try_from("localhost").unwrap(),
            raw,
        )
        .await
        .unwrap();
    active.write_all(&frame()).await.unwrap();
    active.flush().await.unwrap();
    timeout(Duration::from_secs(3), active.read_exact(&mut [0; 17]))
        .await
        .unwrap()
        .unwrap();
    let stalled = [
        TcpStream::connect(addr).await.unwrap(),
        TcpStream::connect(addr).await.unwrap(),
    ];
    sleep(Duration::from_millis(150)).await;
    active.write_all(&frame()).await.unwrap();
    active.flush().await.unwrap();
    let result = timeout(Duration::from_millis(500), active.read_exact(&mut [0; 17])).await;
    eprintln!("active TLS response with two silent handshakes: {result:?}");
    drop(stalled);
    server.abort();
    assert!(
        result.is_ok(),
        "unauthenticated handshakes blocked active requests"
    );
}

#[tokio::test]
async fn handshake_capacity_releases_on_failure_timeout_and_cancellation() {
    let cert = Certificate::new("capacity");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_server_max_taskpool_size(&1)
        .with_server_max_connections(Some(&2))
        .with_max_connections_per_ip(&2)
        .with_connection_rate_limit(&0)
        .with_tls(TlsServerConfig::new(&cert.cert, &cert.key))
        .build();
    let server = tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo)
            .start(),
    );
    let mut pending = timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(socket) = TcpStream::connect(addr).await {
                break socket;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    for expire in [false, true] {
        sleep(Duration::from_millis(100)).await;
        let mut excess = TcpStream::connect(addr).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(2), excess.read(&mut [0]))
                .await
                .unwrap()
                .unwrap(),
            0,
            "saturated handshake capacity must reject, not queue more tasks"
        );
        if expire {
            assert_eq!(
                timeout(Duration::from_secs(12), pending.read(&mut [0]))
                    .await
                    .unwrap()
                    .unwrap(),
                0,
                "silent handshake must time out"
            );
        } else {
            pending.write_all(b"not a TLS record").await.unwrap();
            let mut alert = Vec::new();
            timeout(Duration::from_secs(2), pending.read_to_end(&mut alert))
                .await
                .unwrap()
                .unwrap();
            assert!(
                alert.len() <= 64,
                "only a bounded TLS alert may precede EOF"
            );
        }
        drop(pending);
        sleep(Duration::from_millis(20)).await;
        let raw = TcpStream::connect(addr).await.unwrap();
        let mut authenticated = timeout(
            Duration::from_secs(2),
            cert.connector.connect("localhost".try_into().unwrap(), raw),
        )
        .await
        .unwrap()
        .unwrap();
        authenticated.write_all(&frame()).await.unwrap();
        authenticated.flush().await.unwrap();
        timeout(
            Duration::from_secs(2),
            authenticated.read_exact(&mut [0; 17]),
        )
        .await
        .unwrap()
        .unwrap();
        drop(authenticated);
        sleep(Duration::from_millis(20)).await;
        pending = TcpStream::connect(addr).await.unwrap();
    }
    sleep(Duration::from_millis(100)).await;
    server.abort();
    assert!(server.await.unwrap_err().is_cancelled());
    assert_eq!(
        timeout(Duration::from_secs(2), pending.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}
