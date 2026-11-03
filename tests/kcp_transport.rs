#![cfg(all(feature = "server", feature = "client", feature = "kcp"))]

use std::{net::SocketAddr, time::Duration};

use lynn_tcp::{
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_kcp::KcpConfig,
    lynn_metrics::METRICS,
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_state::AppState,
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
};
use tokio::{
    task::JoinHandle,
    time::{sleep, timeout},
};

static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn echo(input: InputBufVO, state: AppState<u8>) -> HandlerResult {
    assert_eq!(*state, 17);
    HandlerResult::new_with_send(
        1,
        input.get_all_bytes().into(),
        vec![input.get_input_addr().unwrap()],
    )
    .unwrap()
}

async fn start_server(
    config: LynnServerConfigBuilder<'static>,
) -> (SocketAddr, JoinHandle<lynn_tcp::Result<()>>) {
    let probe = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let config = config
        .with_addr(addr)
        .unwrap()
        .with_kcp(KcpConfig {
            idle_timeout_secs: 5,
            ..KcpConfig::default()
        })
        .with_server_max_taskpool_size(&4)
        .with_connection_rate_limit(&0)
        .with_max_connections_per_ip(&0)
        .build();
    let server = tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .with_state(17u8)
            .add_router(1, echo)
            .add_router(2, echo)
            .start(),
    );
    sleep(Duration::from_millis(20)).await;
    assert!(!server.is_finished(), "server failed before client startup");
    (addr, server)
}

async fn client(addr: SocketAddr) -> LynnClient<'static> {
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)
        .unwrap()
        .with_kcp(KcpConfig {
            idle_timeout_secs: 5,
            ..KcpConfig::default()
        })
        .with_server_check_heart_interval(&1)
        .with_reconnect_max_attempts(&1)
        .with_connect_timeout_secs(&2)
        .build();
    timeout(
        Duration::from_secs(4),
        LynnClient::new_with_config(config).await.start(),
    )
    .await
    .unwrap()
}

async fn exchange(client: &mut LynnClient<'_>, method: u16, payload: Vec<u8>) {
    client
        .send_data(HandlerResult::new_with_send_to_server(method, payload.clone().into()).unwrap())
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let reply = timeout(Duration::from_secs(30), client.get_receive_data())
        .await
        .expect("KCP response timed out")
        .unwrap();
    println!(
        "echo bytes={} elapsed={:?}",
        payload.len(),
        started.elapsed()
    );
    assert_eq!(reply.get_all_bytes().as_ref(), payload.as_slice());
}

async fn inactive() {
    timeout(Duration::from_secs(8), async {
        while METRICS.connections.active.get() != 0.0 {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn routes_state_empty_large_frames_and_multiple_clients_are_isolated() {
    let _lock = LOCK.lock().await;
    let (addr, server) = start_server(LynnServerConfigBuilder::new()).await;
    let mut a = client(addr).await;
    let mut b = client(addr).await;
    assert!(a.is_connected() && b.is_connected());
    for size in [
        0,
        1,
        1371,
        16384,
        300_000,
        1024 * 1024,
        10 * 1024 * 1024 - 5,
    ] {
        exchange(&mut a, 1, vec![0x51; size]).await;
        exchange(&mut b, 2, vec![0x62; 13]).await;
    }
    assert_eq!(METRICS.connections.active.get(), 2.0);
    drop(a);
    exchange(&mut b, 1, b"survivor".to_vec()).await;
    drop(b);
    inactive().await;
    server.abort();
    let _ = server.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn owner_drop_releases_udp_listener_and_sessions_even_with_cloned_sender() {
    let _lock = LOCK.lock().await;
    let (addr, server) = start_server(LynnServerConfigBuilder::new()).await;
    let mut peer = client(addr).await;
    assert!(peer.is_connected());
    exchange(&mut peer, 1, b"before drop".to_vec()).await;
    let sender = peer.get_sender().await.unwrap();
    drop(peer);
    timeout(Duration::from_secs(1), sender.closed())
        .await
        .unwrap();
    server.abort();
    let _ = server.await;
    inactive().await;
    timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(socket) = tokio::net::UdpSocket::bind(addr).await {
                drop(socket);
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connection_limit_is_reclaimed_after_idle_and_unreachable_peer_is_not_connected() {
    let _lock = LOCK.lock().await;
    let (addr, server) =
        start_server(LynnServerConfigBuilder::new().with_server_max_connections(Some(&1))).await;
    let mut first = client(addr).await;
    assert!(first.is_connected());
    exchange(&mut first, 1, b"first".to_vec()).await;
    let rejected = client(addr).await;
    assert!(!rejected.is_connected());
    drop(first);
    drop(rejected);
    inactive().await;
    let mut next = client(addr).await;
    assert!(next.is_connected());
    exchange(&mut next, 1, b"replacement".to_vec()).await;
    drop(next);
    server.abort();
    let _ = server.await;
    inactive().await;
    assert!(!client(addr).await.is_connected());
}

#[tokio::test]
async fn invalid_config_and_udp_bind_errors_are_explicit() {
    let _lock = LOCK.lock().await;
    let occupied = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let config = LynnServerConfigBuilder::new()
        .with_addr(occupied.local_addr().unwrap())
        .unwrap()
        .with_kcp(KcpConfig::default())
        .build();
    assert!(
        matches!(LynnServer::new_with_config(config).await.start().await,Err(lynn_tcp::LynnError::Io(e)) if e.kind()==std::io::ErrorKind::AddrInUse)
    );
    let config = LynnServerConfigBuilder::new()
        .with_kcp(KcpConfig {
            idle_timeout_secs: 0,
            ..KcpConfig::default()
        })
        .build();
    assert!(
        LynnServer::new_with_config(config)
            .await
            .start()
            .await
            .is_err()
    );
    let stream_config = KcpConfig {
        protocol: lynn_tcp::lynn_kcp::KcpOptions::stream_mode(),
        ..KcpConfig::default()
    };
    let config = LynnServerConfigBuilder::new()
        .with_kcp(stream_config.clone())
        .build();
    assert!(matches!(
        LynnServer::new_with_config(config).await.start().await,
        Err(lynn_tcp::LynnError::Config(_))
    ));
    let config = LynnClientConfigBuilder::new()
        .with_kcp(stream_config)
        .build();
    assert!(
        !LynnClient::new_with_config(config)
            .await
            .start()
            .await
            .is_connected()
    );
    #[cfg(feature = "tls")]
    {
        let config = LynnServerConfigBuilder::new()
            .with_kcp(KcpConfig::default())
            .with_tls_cert_paths("absent-cert", "absent-key")
            .build();
        assert!(matches!(
            LynnServer::new_with_config(config).await.start().await,
            Err(lynn_tcp::LynnError::Config(_))
        ));
        let config = LynnClientConfigBuilder::new()
            .with_kcp(KcpConfig::default())
            .with_tls(lynn_tcp::lynn_tls::TlsClientConfig::default())
            .build();
        assert!(
            !LynnClient::new_with_config(config)
                .await
                .start()
                .await
                .is_connected()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn malformed_framework_input_releases_session_and_allows_next_connection() {
    let _lock = LOCK.lock().await;
    let (addr, server) =
        start_server(LynnServerConfigBuilder::new().with_server_max_connections(Some(&1))).await;
    let closed = METRICS.connections.closed_total.get();
    let received = METRICS.messages.received_total.get();
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)
        .unwrap()
        .with_kcp(KcpConfig::default())
        .with_message_header_mark(&0)
        .with_reconnect_interval_secs(&60)
        .build();
    let peer = LynnClient::new_with_config(config).await.start().await;
    timeout(Duration::from_secs(3), async {
        while METRICS.connections.closed_total.get() == closed {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("malformed heartbeat frame must close without waiting for KCP idle timeout");
    assert_eq!(METRICS.messages.received_total.get(), received);
    drop(peer);
    let mut next = client(addr).await;
    assert!(next.is_connected());
    exchange(&mut next, 1, b"after protocol error".to_vec()).await;
    drop(next);
    server.abort();
    let _ = server.await;
    inactive().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deterministic_udp_loss_duplication_delay_and_reordering_preserve_data() {
    let _lock = LOCK.lock().await;
    let (addr, server) = start_server(LynnServerConfigBuilder::new()).await;
    let relay = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let relay_addr = relay.local_addr().unwrap();
    let proxy = tokio::spawn(async move {
        let mut client = None;
        let mut bytes = [0; 65536];
        let mut count = 0;
        let mut delayed: Vec<(Vec<u8>, SocketAddr)> = Vec::new();
        let mut tick = tokio::time::interval(Duration::from_millis(3));
        loop {
            tokio::select! {
                _ = tick.tick() => {
                    for (packet,target) in delayed.drain(..).rev() {
                        relay.send_to(&packet,target).await.unwrap();
                    }
                },
                result = relay.recv_from(&mut bytes) => {
                    let (n,source) = result.unwrap();
                    let target = if source == addr {client.unwrap()} else {client=Some(source);addr};
                    count += 1;
                    if count % 13 == 0 {continue;}
                    if count % 5 == 0 {
                        assert!(delayed.len() < 512);
                        delayed.push((bytes[..n].to_vec(),target));
                    } else {relay.send_to(&bytes[..n],target).await.unwrap();}
                    if count % 11 == 0 {relay.send_to(&bytes[..n],target).await.unwrap();}
                }
            }
        }
    });
    let mut peer = client(relay_addr).await;
    assert!(peer.is_connected());
    for index in 0..20 {
        exchange(
            &mut peer,
            1,
            vec![index; if index % 2 == 0 { 65536 } else { 17 }],
        )
        .await;
    }
    drop(peer);
    proxy.abort();
    let _ = proxy.await;
    server.abort();
    let _ = server.await;
    inactive().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn client_reconnects_after_server_restart_and_sender_does_not_retain_old_session() {
    let _lock = LOCK.lock().await;
    let (addr, server) = start_server(LynnServerConfigBuilder::new()).await;
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)
        .unwrap()
        .with_kcp(KcpConfig {
            idle_timeout_secs: 1,
            ..KcpConfig::default()
        })
        .with_reconnect_max_attempts(&10)
        .with_reconnect_interval_secs(&1)
        .build();
    let mut peer = LynnClient::new_with_config(config).await.start().await;
    exchange(&mut peer, 1, b"before".to_vec()).await;
    server.abort();
    let _ = server.await;
    timeout(Duration::from_secs(4), async {
        while peer.is_connected() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    inactive().await;
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_kcp(KcpConfig::default())
        .with_connection_rate_limit(&0)
        .with_max_connections_per_ip(&0)
        .build();
    let replacement = tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .with_state(17u8)
            .add_router(1, echo)
            .start(),
    );
    timeout(Duration::from_secs(8), async {
        while !peer.is_connected() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    exchange(&mut peer, 1, b"after".to_vec()).await;
    drop(peer);
    replacement.abort();
    let _ = replacement.await;
    inactive().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_kcp_requests_have_exact_payloads_and_server_metrics() {
    let _lock = LOCK.lock().await;
    let received = METRICS.messages.received_total.get();
    let sent = METRICS.messages.sent_total.get();
    let total = METRICS.connections.total.get();
    let closed = METRICS.connections.closed_total.get();
    let (addr, server) = start_server(LynnServerConfigBuilder::new()).await;
    let mut tasks = tokio::task::JoinSet::new();
    let release = std::sync::Arc::new(tokio::sync::Barrier::new(33));
    for id in 0..32 {
        let release = release.clone();
        tasks.spawn(async move {
            let config = LynnClientConfigBuilder::new()
                .with_server_addr(addr)
                .unwrap()
                .with_kcp(KcpConfig {
                    idle_timeout_secs: 5,
                    ..KcpConfig::default()
                })
                .with_server_check_heart_interval(&3600)
                .build();
            let mut peer = LynnClient::new_with_config(config).await.start().await;
            assert!(peer.is_connected());
            for sequence in 0..25 {
                exchange(&mut peer, 1, vec![id, sequence]).await;
            }
            release.wait().await;
        });
    }
    timeout(Duration::from_secs(30), async {
        while METRICS.messages.sent_total.get() != sent + 800.0 {
            sleep(Duration::from_millis(5)).await;
        }
        release.wait().await;
        while let Some(task) = tasks.join_next().await {
            task.unwrap();
        }
    })
    .await
    .unwrap();
    inactive().await;
    assert_eq!(METRICS.connections.total.get(), total + 32.0);
    assert_eq!(METRICS.connections.closed_total.get(), closed + 32.0);
    assert_eq!(METRICS.messages.received_total.get(), received + 832.0);
    assert_eq!(METRICS.messages.sent_total.get(), sent + 800.0);
    server.abort();
    let _ = server.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ipv6_custom_marks_and_local_oversize_rejection_keep_connection_usable() {
    let _lock = LOCK.lock().await;
    let probe = std::net::UdpSocket::bind("[::1]:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_kcp(KcpConfig::default())
        .with_message_header_mark(&1234)
        .with_message_tail_mark(&4321)
        .build();
    let server = tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .with_state(17u8)
            .add_router(1, echo)
            .start(),
    );
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)
        .unwrap()
        .with_kcp(KcpConfig::default())
        .with_message_header_mark(&1234)
        .with_message_tail_mark(&4321)
        .build();
    let mut peer = LynnClient::new_with_config(config).await.start().await;
    assert!(peer.is_connected());
    assert!(
        HandlerResult::new_with_send_to_server(1, vec![0; 10 * 1024 * 1024 - 4].into()).is_err()
    );
    exchange(&mut peer, 1, b"still healthy".to_vec()).await;
    drop(peer);
    server.abort();
    let _ = server.await;
    inactive().await;
}
