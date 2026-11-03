#![cfg(feature = "server")]

use std::{net::SocketAddr, time::Duration};

use lynn_tcp::{
    lynn_metrics::{METRICS, export_metrics},
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{sleep, timeout},
};

async fn echo(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(1, "ok".into(), vec![input.get_input_addr().unwrap()]).unwrap()
}

fn frame() -> Vec<u8> {
    let mut bytes = Vec::from(9177u16.to_le_bytes());
    bytes.extend_from_slice(&5u64.to_le_bytes());
    bytes.extend_from_slice(&[1, 1, 0]);
    bytes.extend_from_slice(&7719u16.to_le_bytes());
    bytes
}

async fn connect(addr: SocketAddr) -> TcpStream {
    timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(peer) = TcpStream::connect(addr).await {
                return peer;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_traffic_updates_metrics_without_manual_increments() {
    let _lock = TEST_LOCK.lock().await;
    let before = (
        METRICS.connections.total.get(),
        METRICS.messages.received_total.get(),
        METRICS.messages.sent_total.get(),
        METRICS.network.bytes_received_total.get(),
        METRICS.network.bytes_sent_total.get(),
    );
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .with_connection_rate_limit(&0)
        .with_max_connections_per_ip(&0)
        .build();
    let server = tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo)
            .start(),
    );
    let mut peer = connect(addr).await;
    peer.write_all(&frame()).await.unwrap();
    let mut response = [0; 17];
    timeout(Duration::from_secs(2), peer.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response[13..15], b"ok");
    wait_for(|| METRICS.messages.sent_total.get() == before.2 + 1.0).await;
    let after = (
        METRICS.connections.total.get(),
        METRICS.messages.received_total.get(),
        METRICS.messages.sent_total.get(),
        METRICS.network.bytes_received_total.get(),
        METRICS.network.bytes_sent_total.get(),
    );
    println!(
        "successful echo: before={before:?}, after={after:?}, active={}",
        METRICS.connections.active.get()
    );
    drop(peer);
    wait_for(|| METRICS.connections.active.get() == 0.0).await;
    assert_eq!(METRICS.connections.closed_total.get(), after.0);
    server.abort();
    let _ = server.await;
    assert_eq!(
        after,
        (
            before.0 + 1.0,
            before.1 + 1.0,
            before.2 + 1.0,
            before.3 + 15.0,
            before.4 + 17.0
        )
    );
    assert!(export_metrics().contains("lynn_connections_total"));
}

static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn wait_for(condition: impl Fn() -> bool) {
    timeout(Duration::from_secs(4), async {
        while !condition() {
            sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("metric did not reach expected value");
}

async fn server(
    config: LynnServerConfigBuilder<'static>,
) -> (SocketAddr, tokio::task::JoinHandle<lynn_tcp::Result<()>>) {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let config = config
        .with_addr(addr)
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .with_connection_rate_limit(&0)
        .with_max_connections_per_ip(&0)
        .build();
    let server = tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo)
            .start(),
    );
    (addr, server)
}

async fn exchange(peer: &mut (impl tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin)) {
    peer.write_all(&frame()).await.unwrap();
    let mut response = [0; 17];
    timeout(Duration::from_secs(3), peer.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response[13..15], b"ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multiple_messages_fragmentation_eof_and_idle_connections_count_once() {
    let _lock = TEST_LOCK.lock().await;
    let before = (
        METRICS.connections.total.get(),
        METRICS.connections.closed_total.get(),
        METRICS.messages.received_total.get(),
        METRICS.messages.sent_total.get(),
    );
    let (addr, task) = server(LynnServerConfigBuilder::new()).await;
    let mut peer = connect(addr).await;
    let idle = connect(addr).await;
    wait_for(|| METRICS.connections.active.get() == 2.0).await;
    for _ in 0..8 {
        exchange(&mut peer).await;
    }
    for byte in frame() {
        peer.write_all(&[byte]).await.unwrap();
    }
    let mut reply = [0; 17];
    peer.read_exact(&mut reply).await.unwrap();
    let mut batch = frame();
    batch.extend(frame());
    peer.write_all(&batch).await.unwrap();
    peer.shutdown().await.unwrap();
    let mut replies = Vec::new();
    timeout(Duration::from_secs(3), peer.read_to_end(&mut replies))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(replies.len(), 34);
    drop(idle);
    wait_for(|| METRICS.connections.active.get() == 0.0).await;
    assert_eq!(METRICS.connections.total.get(), before.0 + 2.0);
    assert_eq!(METRICS.connections.closed_total.get(), before.1 + 2.0);
    assert_eq!(METRICS.messages.received_total.get(), before.2 + 11.0);
    assert_eq!(METRICS.messages.sent_total.get(), before.3 + 11.0);
    task.abort();
    let _ = task.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_timeout_admission_rejection_and_cancellation_balance_lifecycle() {
    let _lock = TEST_LOCK.lock().await;
    let before = (
        METRICS.connections.total.get(),
        METRICS.connections.closed_total.get(),
        METRICS.connections.failed_total.get(),
        METRICS.messages.invalid_total.get(),
    );
    let (addr, task) = server(
        LynnServerConfigBuilder::new()
            .with_read_timeout_secs(&1)
            .with_server_max_connections(Some(&1)),
    )
    .await;
    let mut peer = connect(addr).await;
    exchange(&mut peer).await;
    let mut rejected = connect(addr).await;
    assert_eq!(
        timeout(Duration::from_secs(3), rejected.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    wait_for(|| METRICS.connections.failed_total.get() == before.2 + 1.0).await;
    peer.write_all(&[0, 0]).await.unwrap();
    assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
    wait_for(|| METRICS.connections.active.get() == 0.0).await;
    assert_eq!(METRICS.messages.invalid_total.get(), before.3 + 1.0);
    let mut idle = connect(addr).await;
    assert_eq!(
        timeout(Duration::from_secs(3), idle.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    wait_for(|| METRICS.connections.active.get() == 0.0).await;
    let mut cancelled = connect(addr).await;
    exchange(&mut cancelled).await;
    task.abort();
    let _ = task.await;
    wait_for(|| METRICS.connections.active.get() == 0.0).await;
    assert_eq!(METRICS.connections.total.get(), before.0 + 4.0);
    assert_eq!(METRICS.connections.closed_total.get(), before.1 + 4.0);
    assert_eq!(METRICS.connections.failed_total.get(), before.2 + 1.0);
}

#[cfg(feature = "tls")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tls_counts_plaintext_frames_and_failed_handshakes_without_double_close() {
    use std::sync::Arc;
    let _lock = TEST_LOCK.lock().await;
    let before = (
        METRICS.connections.total.get(),
        METRICS.connections.closed_total.get(),
        METRICS.connections.failed_total.get(),
        METRICS.network.bytes_received_total.get(),
        METRICS.network.bytes_sent_total.get(),
    );
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let dir = std::env::temp_dir().join(format!("lynn-metrics-tls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, signing_key.serialize_pem()).unwrap();
    let tls = lynn_tcp::lynn_tls::TlsServerConfig::new(
        cert_path.to_str().unwrap(),
        key_path.to_str().unwrap(),
    );
    let (addr, task) = server(LynnServerConfigBuilder::new().with_tls(tls)).await;
    let mut bad = connect(addr).await;
    bad.write_all(b"not a TLS handshake").await.unwrap();
    let _ = timeout(Duration::from_secs(3), bad.read(&mut [0]))
        .await
        .unwrap();
    drop(bad);
    wait_for(|| METRICS.connections.active.get() == 0.0).await;
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
    exchange(&mut peer).await;
    peer.shutdown().await.unwrap();
    let mut rest = Vec::new();
    peer.read_to_end(&mut rest).await.unwrap();
    wait_for(|| METRICS.connections.active.get() == 0.0).await;
    assert_eq!(METRICS.connections.total.get(), before.0 + 2.0);
    assert_eq!(METRICS.connections.closed_total.get(), before.1 + 2.0);
    assert_eq!(METRICS.connections.failed_total.get(), before.2 + 1.0);
    assert_eq!(METRICS.network.bytes_received_total.get(), before.3 + 15.0);
    assert_eq!(METRICS.network.bytes_sent_total.get(), before.4 + 17.0);
    task.abort();
    let _ = task.await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_churn_preserves_exact_counters_and_reclaims_all_sessions() {
    let _lock = TEST_LOCK.lock().await;
    let before = (
        METRICS.connections.total.get(),
        METRICS.connections.closed_total.get(),
        METRICS.messages.received_total.get(),
        METRICS.messages.sent_total.get(),
        METRICS.network.bytes_received_total.get(),
        METRICS.network.bytes_sent_total.get(),
    );
    let (addr, task) = server(LynnServerConfigBuilder::new()).await;
    let mut jobs = tokio::task::JoinSet::new();
    for _ in 0..64 {
        jobs.spawn(async move {
            for _ in 0..3 {
                let mut peer = connect(addr).await;
                for _ in 0..20 {
                    exchange(&mut peer).await;
                }
            }
        });
    }
    timeout(Duration::from_secs(30), async {
        while let Some(job) = jobs.join_next().await {
            job.unwrap();
        }
    })
    .await
    .unwrap();
    wait_for(|| METRICS.connections.active.get() == 0.0).await;
    assert_eq!(METRICS.connections.total.get(), before.0 + 192.0);
    assert_eq!(METRICS.connections.closed_total.get(), before.1 + 192.0);
    assert_eq!(METRICS.messages.received_total.get(), before.2 + 3840.0);
    assert_eq!(METRICS.messages.sent_total.get(), before.3 + 3840.0);
    assert_eq!(
        METRICS.network.bytes_received_total.get(),
        before.4 + 3840.0 * 15.0
    );
    assert_eq!(
        METRICS.network.bytes_sent_total.get(),
        before.5 + 3840.0 * 17.0
    );
    task.abort();
    let _ = task.await;
}
