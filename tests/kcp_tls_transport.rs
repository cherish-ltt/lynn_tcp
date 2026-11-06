#![cfg(all(feature = "server", feature = "client", feature = "kcp-tls"))]

use lynn_tcp::{
    lynn_client::{ConnectionState, LynnClient, LynnClientConfigBuilder},
    lynn_kcp::KcpConfig,
    lynn_metrics::METRICS,
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_state::AppState,
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
    lynn_tls::{TlsClientConfig, TlsServerConfig},
};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::UdpSocket,
    task::JoinHandle,
    time::{sleep, timeout},
};
#[path = "support/tls_certificates.rs"]
mod certificates;
use certificates::Certificates;
static LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn echo(input: InputBufVO, calls: AppState<Arc<AtomicUsize>>) -> HandlerResult {
    calls.fetch_add(1, Ordering::SeqCst);
    HandlerResult::new_with_send(
        1,
        input.get_all_bytes().into(),
        vec![input.get_input_addr().unwrap()],
    )
    .unwrap()
}

async fn server(
    addr: impl std::net::ToSocketAddrs,
    tls: Option<TlsServerConfig>,
    calls: Arc<AtomicUsize>,
) -> (
    SocketAddr,
    lynn_tcp::lynn_server::BroadcastHandle,
    JoinHandle<lynn_tcp::Result<()>>,
) {
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_kcp(KcpConfig {
            idle_timeout_secs: 2,
            ..Default::default()
        })
        .with_server_max_taskpool_size(&2)
        .with_server_max_connections(Some(&4))
        .with_connection_rate_limit(&0)
        .with_max_connections_per_ip(&0);
    let config = match tls {
        Some(tls) => config.with_tls(tls),
        None => config,
    };
    let server = LynnServer::new_with_config(config.build())
        .await
        .with_state(calls)
        .add_router(1, echo)
        .add_router(2, echo);
    let handle = server.broadcast_handle();
    let ready = server.startup_handle();
    let task = tokio::spawn(server.start());
    let addr = timeout(Duration::from_secs(2), ready.wait())
        .await
        .unwrap()
        .unwrap();
    (addr, handle, task)
}

async fn client(addr: SocketAddr, tls: Option<TlsClientConfig>) -> LynnClient<'static> {
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)
        .unwrap()
        .with_kcp(KcpConfig {
            idle_timeout_secs: 2,
            ..Default::default()
        })
        .with_connect_timeout_secs(&1)
        .with_reconnect_max_attempts(&1)
        .with_server_check_heart_interval(&3600);
    let config = match tls {
        Some(tls) => config.with_tls(tls),
        None => config,
    };
    timeout(
        Duration::from_secs(3),
        LynnClient::new_with_config(config.build()).await.start(),
    )
    .await
    .unwrap()
}

fn trusted(certs: &Certificates) -> TlsClientConfig {
    TlsClientConfig {
        ca_cert_path: Some(certs.ca.clone()),
        ..Default::default()
    }
}

async fn exchange(peer: &mut LynnClient<'_>, method: u16, data: Vec<u8>) {
    peer.send_data(HandlerResult::new_with_send_to_server(method, data.clone().into()).unwrap())
        .await
        .unwrap();
    let reply = timeout(Duration::from_secs(30), peer.get_receive_data())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply.get_all_bytes().as_ref(), data.as_slice());
}

async fn cleanup(task: JoinHandle<lynn_tcp::Result<()>>) {
    task.abort();
    let _ = task.await;
    timeout(Duration::from_secs(4), async {
        while METRICS.connections.active.get() != 0.0 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn secure_udp_routes_state_broadcast_large_frames_and_plaintext_metrics() {
    let _lock = LOCK.lock().await;
    let certs = Certificates::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let (addr, broadcast, task) = server(
        "127.0.0.1:0",
        Some(TlsServerConfig::new(&certs.server_cert, &certs.server_key)),
        calls.clone(),
    )
    .await;
    let warmup_messages = METRICS.messages.received_total.get();
    let warmup_received = METRICS.network.bytes_received_total.get();
    let mut peer = client(addr, Some(trusted(&certs))).await;
    assert!(peer.is_connected());
    exchange(&mut peer, 1, b"single small frame".to_vec()).await;
    // The immediate heartbeat tick can arrive after the first business reply.
    timeout(Duration::from_secs(3), async {
        while METRICS.messages.received_total.get() < warmup_messages + 2.0 {
            sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(METRICS.messages.received_total.get(), warmup_messages + 2.0);
    assert_eq!(
        METRICS.network.bytes_received_total.get() - warmup_received,
        (b"single small frame".len() + 30) as f64
    );
    let received = METRICS.network.bytes_received_total.get();
    let sent = METRICS.network.bytes_sent_total.get();
    let mut total = 0;
    for (i, length) in [0, 16 * 1024 - 1, 16 * 1024, 65536, 10 * 1024 * 1024 - 5]
        .into_iter()
        .enumerate()
    {
        exchange(
            &mut peer,
            if i % 2 == 0 { 1 } else { 2 },
            vec![i as u8; length],
        )
        .await;
        total += length + 15;
    }
    assert_eq!(
        METRICS.network.bytes_received_total.get() - received,
        total as f64
    );
    assert_eq!(METRICS.network.bytes_sent_total.get() - sent, total as f64);
    assert_eq!(calls.load(Ordering::SeqCst), 6);
    assert_eq!(
        broadcast
            .broadcast(3, b"background".as_slice().into())
            .unwrap()
            .enqueued,
        1
    );
    let reply = timeout(Duration::from_secs(3), peer.get_receive_data())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply.get_all_bytes().as_ref(), b"background");
    drop(peer);
    cleanup(task).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ip_dns_and_mtls_authentication_failures_never_reach_handler_or_broadcast() {
    let _lock = LOCK.lock().await;
    let certs = Certificates::new();
    let other = Certificates::new();
    for ipv6 in [false, true] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (addr, broadcast, task) = server(
            if ipv6 { "[::1]:0" } else { "127.0.0.1:0" },
            Some(
                TlsServerConfig::new(&certs.server_cert, &certs.server_key)
                    .with_client_ca(&certs.ca),
            ),
            calls.clone(),
        )
        .await;
        let mut valid = trusted(&certs);
        valid.client_cert_path = Some(certs.client_cert.clone());
        valid.client_key_path = Some(certs.client_key.clone());
        let mut variants = vec![trusted(&certs)];
        let mut wrong_ca = valid.clone();
        wrong_ca.ca_cert_path = Some(other.ca.clone());
        variants.push(wrong_ca);
        let mut wrong_name = valid.clone();
        wrong_name.server_name = Some("wrong.example".into());
        variants.push(wrong_name);
        let mut partial = valid.clone();
        partial.client_key_path = None;
        variants.push(partial);
        let mut partial = valid.clone();
        partial.client_cert_path = None;
        variants.push(partial);
        let mut missing_ca = valid.clone();
        missing_ca.ca_cert_path = None;
        variants.push(missing_ca);
        let mut wrong_identity = valid.clone();
        wrong_identity.client_cert_path = Some(other.client_cert.clone());
        wrong_identity.client_key_path = Some(other.client_key.clone());
        variants.push(wrong_identity);
        let mut insecure = valid.clone();
        insecure.danger_accept_invalid_certs = true;
        variants.push(insecure);
        for invalid in variants {
            let mut peer = client(addr, Some(invalid)).await;
            assert!(
                !peer.is_connected(),
                "failed authentication must never publish Connected"
            );
            assert!(peer.subscribe_connection_state().is_none());
            assert!(peer.get_sender().await.is_none());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(
                broadcast.broadcast(3, Vec::new().into()).unwrap().enqueued,
                0
            );
            drop(peer);
            timeout(Duration::from_secs(4), async {
                while METRICS.connections.active.get() != 0.0 {
                    sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
        }
        for dns in [false, true] {
            let mut config = valid.clone();
            if dns {
                config.server_name = Some("localhost".into());
            }
            let mut peer = client(addr, Some(config)).await;
            assert!(peer.is_connected());
            exchange(&mut peer, 2, b"verified mTLS identity".to_vec()).await;
            drop(peer);
            timeout(Duration::from_secs(4), async {
                while METRICS.connections.active.get() != 0.0 {
                    sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .unwrap();
        }
        cleanup(task).await;
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let (addr, broadcast, task) = server(
        "127.0.0.1:0",
        Some(TlsServerConfig::new(&certs.expired_cert, &certs.server_key)),
        calls.clone(),
    )
    .await;
    let peer = client(addr, Some(trusted(&certs))).await;
    assert!(!peer.is_connected());
    assert!(peer.subscribe_connection_state().is_none());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        broadcast.broadcast(3, Vec::new().into()).unwrap().enqueued,
        0
    );
    drop(peer);
    cleanup(task).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn intermediate_chains_authenticate_and_missing_intermediates_never_reach_business() {
    let _lock = LOCK.lock().await;
    let certs = Certificates::with_intermediate();
    for (server_chain, client_chain, accepted) in [
        (&certs.server_cert, &certs.client_cert, true),
        (&certs.server_leaf, &certs.client_cert, false),
        (&certs.server_cert, &certs.client_leaf, false),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let (addr, broadcast, task) = server(
            "127.0.0.1:0",
            Some(TlsServerConfig::new(server_chain, &certs.server_key).with_client_ca(&certs.ca)),
            calls.clone(),
        )
        .await;
        let mut config = trusted(&certs);
        config.client_cert_path = Some(client_chain.clone());
        config.client_key_path = Some(certs.client_key.clone());
        let mut peer = client(addr, Some(config)).await;
        assert_eq!(peer.is_connected(), accepted);
        if accepted {
            exchange(&mut peer, 1, b"root to intermediate to leaf".to_vec()).await;
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        } else {
            assert!(peer.subscribe_connection_state().is_none());
            assert!(peer.get_sender().await.is_none());
            assert_eq!(calls.load(Ordering::SeqCst), 0);
            assert_eq!(
                broadcast.broadcast(3, Vec::new().into()).unwrap().enqueued,
                0
            );
        }
        drop(peer);
        cleanup(task).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn secure_client_state_tracks_reconnect_generation_and_stops_after_retries() {
    let _lock = LOCK.lock().await;
    let certs = Certificates::new();
    let tls = TlsServerConfig::new(&certs.server_cert, &certs.server_key).with_client_ca(&certs.ca);
    let calls = Arc::new(AtomicUsize::new(0));
    let (addr, _, task) = server("127.0.0.1:0", Some(tls.clone()), calls.clone()).await;
    let mut identity = trusted(&certs);
    identity.client_cert_path = Some(certs.client_cert.clone());
    identity.client_key_path = Some(certs.client_key.clone());
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)
        .unwrap()
        .with_kcp(KcpConfig {
            idle_timeout_secs: 1,
            ..Default::default()
        })
        .with_tls(identity)
        .with_connect_timeout_secs(&1)
        .with_reconnect_max_attempts(&2)
        .with_reconnect_interval_secs(&1)
        .with_server_check_heart_interval(&3600)
        .build();
    let peer = LynnClient::new_with_config(config).await;
    assert!(peer.subscribe_connection_state().is_none());
    let mut peer = timeout(Duration::from_secs(3), peer.start()).await.unwrap();
    let mut state = peer.subscribe_connection_state().unwrap();
    let mut slow = state.clone();
    assert_eq!(
        *state.borrow_and_update(),
        ConnectionState::Connected { generation: 1 }
    );
    slow.borrow_and_update();
    let sender = peer.get_sender().await.unwrap();
    exchange(&mut peer, 1, b"before secure reconnect".to_vec()).await;

    cleanup(task).await;
    timeout(
        Duration::from_secs(4),
        state.wait_for(|value| *value == ConnectionState::Reconnecting { generation: 1 }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!peer.is_connected());
    let (_, _, replacement) = server(addr, Some(tls), calls.clone()).await;
    timeout(
        Duration::from_secs(5),
        state.wait_for(|value| *value == ConnectionState::Connected { generation: 2 }),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        *slow.borrow_and_update(),
        ConnectionState::Connected { generation: 2 }
    );
    sender
        .send(
            HandlerResult::new_with_send_to_server(1, b"after secure reconnect".as_slice().into())
                .unwrap(),
        )
        .await
        .unwrap();
    let reply = timeout(Duration::from_secs(3), peer.get_receive_data())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply.get_all_bytes().as_ref(), b"after secure reconnect");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(peer.is_connected());

    cleanup(replacement).await;
    timeout(
        Duration::from_secs(4),
        state.wait_for(|value| *value == ConnectionState::Reconnecting { generation: 2 }),
    )
    .await
    .unwrap()
    .unwrap();
    timeout(
        Duration::from_secs(6),
        state.wait_for(|value| *value == ConnectionState::Stopped { generation: 2 }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(sender.is_closed());
    assert!(
        sender
            .try_send(HandlerResult::new_with_send_to_server(1, Vec::new().into()).unwrap())
            .is_err()
    );
    assert!(!peer.is_connected());
    assert!(
        timeout(Duration::from_secs(1), peer.get_receive_data())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        timeout(Duration::from_secs(1), state.changed())
            .await
            .unwrap()
            .is_err()
    );
    assert_eq!(*slow.borrow(), ConnectionState::Stopped { generation: 2 });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mode_mismatch_fails_without_fallback_and_owner_cancels_half_handshakes() {
    let _lock = LOCK.lock().await;
    let certs = Certificates::new();
    for secure in [true, false] {
        let calls = Arc::new(AtomicUsize::new(0));
        let weak = Arc::downgrade(&calls);
        let tls = secure.then(|| TlsServerConfig::new(&certs.server_cert, &certs.server_key));
        let (addr, broadcast, task) = server("127.0.0.1:0", tls, calls).await;
        let mut peer = client(addr, (!secure).then(|| trusted(&certs))).await;
        assert!(!peer.is_connected());
        assert!(peer.subscribe_connection_state().is_none());
        assert!(peer.get_sender().await.is_none());
        assert_eq!(
            broadcast.broadcast(3, Vec::new().into()).unwrap().enqueued,
            0
        );
        drop(peer);
        if secure {
            let mut established = client(addr, Some(trusted(&certs))).await;
            assert!(established.is_connected());
            let mut sockets = Vec::new();
            for conv in 1..=6 {
                let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
                let hello = b"\0lynn-kcp/2";
                let mut bytes = kcp_ovo::helper::KcpPacketHeader::new(
                    conv,
                    kcp_ovo::KcpCmd::Push,
                    0,
                    128,
                    0,
                    0,
                    0,
                    hello.len() as u32,
                )
                .to_bytes()
                .unwrap();
                bytes.extend_from_slice(hello);
                socket.send_to(&bytes, addr).await.unwrap();
                sockets.push(socket);
            }
            sleep(Duration::from_millis(50)).await;
            assert_eq!(
                METRICS.connections.active.get(),
                3.0,
                "two half handshakes plus one authenticated connection"
            );
            assert_eq!(
                broadcast.broadcast(3, Vec::new().into()).unwrap().enqueued,
                1
            );
            let pushed = timeout(Duration::from_secs(2), established.get_receive_data())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(pushed.get_all_bytes().len(), 0);
            timeout(
                Duration::from_secs(2),
                exchange(&mut established, 1, b"worker remains available".to_vec()),
            )
            .await
            .expect("half handshakes must not occupy business workers");
            drop(established);
        }
        cleanup(task).await;
        timeout(Duration::from_secs(2), async {
            while weak.upgrade().is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn encrypted_udp_capture_with_loss_duplication_reordering_and_delay() {
    let _lock = LOCK.lock().await;
    let certs = Certificates::new();
    let (addr, _, task) = server(
        "127.0.0.1:0",
        Some(TlsServerConfig::new(&certs.server_cert, &certs.server_key)),
        Arc::default(),
    )
    .await;
    let relay = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let relay_addr = relay.local_addr().unwrap();
    let observed = Arc::new(AtomicUsize::new(0));
    let counter = observed.clone();
    let marker = b"unique plaintext business frame must not appear on UDP";
    let proxy = tokio::spawn(async move {
        let mut client = None;
        let mut bytes = [0; 65536];
        let mut count = 0;
        let mut delayed: Vec<(Vec<u8>, SocketAddr)> = Vec::new();
        let mut tick = tokio::time::interval(Duration::from_millis(3));
        loop {
            tokio::select! {
                _ = tick.tick() => { for (packet,target) in delayed.drain(..).rev() { relay.send_to(&packet,target).await.unwrap(); } },
                result = relay.recv_from(&mut bytes) => {
                    let (n,source) = result.unwrap();
                    assert!(n <= 1400);
                    assert!(!bytes[..n].windows(marker.len()).any(|window| window == marker));
                    counter.fetch_add(1, Ordering::SeqCst);
                    let target = if source == addr {client.unwrap()} else {client=Some(source);addr};
                    count += 1;
                    if count % 13 == 0 {continue;}
                    if count % 5 == 0 { assert!(delayed.len() < 512); delayed.push((bytes[..n].to_vec(),target)); }
                    else {relay.send_to(&bytes[..n],target).await.unwrap();}
                    if count % 11 == 0 {relay.send_to(&bytes[..n],target).await.unwrap();}
                },
            }
        }
    });
    let mut peer = client(relay_addr, Some(trusted(&certs))).await;
    assert!(peer.is_connected());
    for i in 0..12 {
        exchange(
            &mut peer,
            1,
            if i % 2 == 0 {
                marker.to_vec()
            } else {
                vec![i; 65536]
            },
        )
        .await;
    }
    assert!(observed.load(Ordering::SeqCst) > 100);
    assert!(!proxy.is_finished());
    drop(peer);
    proxy.abort();
    let _ = proxy.await;
    cleanup(task).await;
}
