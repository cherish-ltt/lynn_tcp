use super::{Certificates, KcpConfig, KcpStream, Mode};
use kcp_ovo::helper::KcpPacketHeader;
use std::{sync::Arc, time::Duration};
use tokio::{net::UdpSocket, time::timeout};

#[cfg(feature = "server")]
#[tokio::test]
async fn active_half_handshake_has_total_deadline_and_recycles_admission() {
    use crate::{
        application::server::server_tasks::ServerTasks,
        infrastructure::{
            connection::connection_limiter::ConnectionLimiter,
            kcp::listener::KcpListener,
            tcp::stream::StreamAcceptor,
            tls::{tls_config::TlsServerConfig, tls_provider::build_server_acceptor_mode},
        },
    };
    let certs = Certificates::new();
    let listener = KcpListener::bind("127.0.0.1:0", KcpConfig::default(), Mode::Tls)
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let acceptor = build_server_acceptor_mode(
        &TlsServerConfig::new(&certs.server_cert, &certs.server_key),
        true,
    )
    .unwrap();
    let (owner, tasks) = ServerTasks::new();
    let queue = Arc::new(crossbeam_deque::Injector::new());
    let limiter = Arc::new(ConnectionLimiter::new(0, 1));
    let listener_task = tasks.spawn(listener.run(
        Some(1),
        Some(limiter.clone()),
        queue.clone(),
        Arc::new(StreamAcceptor::KcpTls(Arc::new(acceptor))),
        tasks.clone(),
        1,
    ));
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    socket.connect(addr).await.unwrap();
    let (stream, ready) = KcpStream::start(
        socket.clone(),
        addr,
        1,
        KcpConfig::default(),
        Mode::Tls,
        None,
    )
    .unwrap();
    timeout(Duration::from_secs(2), ready)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(limiter.acquire(addr.ip()).await.is_none());
    let mut hello = KcpPacketHeader::new(
        1,
        kcp_ovo::KcpCmd::Push,
        0,
        128,
        0,
        0,
        0,
        Mode::Tls.hello().len() as u32,
    )
    .to_bytes()
    .unwrap();
    hello.extend_from_slice(Mode::Tls.hello());
    let keepalive = tokio::spawn(async move {
        for _ in 0..90 {
            socket.send(&hello).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    let started = tokio::time::Instant::now();
    timeout(Duration::from_secs(12), async {
        loop {
            if let Some(permit) = limiter.acquire(addr.ip()).await {
                drop(permit);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("TLS deadline must expire even while valid KCP packets arrive");
    assert!(started.elapsed() >= Duration::from_secs(9));
    assert!(
        queue.is_empty(),
        "unauthenticated streams are never dispatched"
    );
    keepalive.abort();
    let _ = keepalive.await;
    drop(stream);

    let replacement = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    replacement.connect(addr).await.unwrap();
    let (stream, ready) =
        KcpStream::start(replacement, addr, 2, KcpConfig::default(), Mode::Tls, None).unwrap();
    timeout(Duration::from_secs(2), ready)
        .await
        .unwrap()
        .unwrap()
        .expect("deadline must recycle global, per-IP and handshake permits");
    assert!(limiter.acquire(addr.ip()).await.is_none());
    drop(owner);
    listener_task.await.unwrap();
    drop(stream);
    timeout(Duration::from_secs(2), async {
        while limiter.acquire(addr.ip()).await.is_none() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("owner cancellation must release half-handshake admission");
}

#[cfg(feature = "client")]
async fn accept_kcp(
    socket: Arc<UdpSocket>,
    peer: std::net::SocketAddr,
    first_packet: Vec<u8>,
) -> (KcpStream, tokio::task::JoinHandle<()>) {
    use super::PACKETS;
    let (header, _) = KcpPacketHeader::from_bytes(&mut first_packet.as_slice()).unwrap();
    let (tx, rx) = tokio::sync::mpsc::channel(PACKETS);
    tx.send(first_packet).await.unwrap();
    let (stream, ready) = KcpStream::start(
        socket.clone(),
        peer,
        header.conv,
        KcpConfig::default(),
        Mode::Tls,
        Some(rx),
    )
    .unwrap();
    let relay = tokio::spawn(async move {
        let mut bytes = [0; 1400];
        loop {
            tokio::select! {
                _ = tx.closed() => break,
                received = socket.recv_from(&mut bytes) => {
                    let (length, _) = received.unwrap();
                    if tx.send(bytes[..length].to_vec()).await.is_err() { break; }
                },
            }
        }
    });
    timeout(Duration::from_secs(2), ready)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    (stream, relay)
}

#[cfg(feature = "client")]
async fn assert_client_socket_released(peer: std::net::SocketAddr) {
    timeout(Duration::from_secs(2), async {
        loop {
            match UdpSocket::bind(peer).await {
                Ok(socket) => break drop(socket),
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                    tokio::task::yield_now().await;
                },
                Err(error) => panic!("unexpected UDP bind error after client stopped: {error}"),
            }
        }
    })
    .await
    .expect("stopping client startup must cancel its KCP driver and free the port");
}

#[cfg(feature = "client")]
#[tokio::test]
async fn cancelling_client_start_in_kcp_or_tls_handshake_releases_udp_socket() {
    use crate::{
        application::client::{client_config::LynnClientConfigBuilder, lynn_client::LynnClient},
        infrastructure::tls::tls_config::TlsClientConfig,
    };
    use tokio::io::AsyncReadExt;
    let certs = Certificates::new();
    for complete_kcp in [false, true] {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let addr = socket.local_addr().unwrap();
        let config = LynnClientConfigBuilder::new()
            .with_server_addr(addr)
            .unwrap()
            .with_kcp(KcpConfig::default())
            .with_tls(TlsClientConfig {
                ca_cert_path: Some(certs.ca.clone()),
                ..Default::default()
            })
            .with_connect_timeout_secs(&60)
            .build();
        let client = LynnClient::new_with_config(config).await;
        let start = tokio::spawn(client.start());
        let mut bytes = [0; 1400];
        let (length, peer) = timeout(Duration::from_secs(2), socket.recv_from(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        let mut stream = None;
        let mut relay = None;
        if complete_kcp {
            let (mut kcp, task) = accept_kcp(socket, peer, bytes[..length].to_vec()).await;
            timeout(Duration::from_secs(2), kcp.read_exact(&mut [0; 5]))
                .await
                .expect("client must reach the pending TLS handshake")
                .unwrap();
            relay = Some(task);
            stream = Some(kcp);
        }
        assert!(!start.is_finished());
        start.abort();
        assert!(matches!(start.await, Err(error) if error.is_cancelled()));
        assert_client_socket_released(peer).await;
        drop(stream);
        if let Some(relay) = relay {
            timeout(Duration::from_secs(2), relay)
                .await
                .unwrap()
                .unwrap();
        }
    }
}

#[cfg(feature = "client")]
#[tokio::test]
async fn client_state_waits_for_encrypted_confirmation_and_ends_on_owner_drop() {
    use crate::{
        infrastructure::{kcp::TLS_READY, tls::tls_config::TlsClientConfig},
        lynn_client::{ConnectionState, LynnClient, LynnClientConfigBuilder},
    };
    use tokio::io::AsyncWriteExt;
    let certs = Certificates::new();
    let (_, acceptor) = super::tls_configs(&certs);
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(socket.local_addr().unwrap())
        .unwrap()
        .with_kcp(KcpConfig::default())
        .with_tls(TlsClientConfig {
            ca_cert_path: Some(certs.ca.clone()),
            ..Default::default()
        })
        .with_server_check_heart_interval(&3600)
        .build();
    let client = LynnClient::new_with_config(config).await;
    assert!(client.subscribe_connection_state().is_none());
    let mut start = tokio::spawn(client.start());
    let mut bytes = [0; 1400];
    let (length, peer) = timeout(Duration::from_secs(2), socket.recv_from(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    let (kcp, relay) = accept_kcp(socket, peer, bytes[..length].to_vec()).await;
    let mut stream = timeout(Duration::from_secs(2), acceptor.accept(kcp))
        .await
        .unwrap()
        .unwrap();
    assert!(
        timeout(Duration::from_millis(25), &mut start)
            .await
            .is_err(),
        "TLS completion alone must not expose a Connected client"
    );
    timeout(Duration::from_secs(2), async {
        stream.write_all(TLS_READY).await.unwrap();
        stream.flush().await.unwrap();
    })
    .await
    .unwrap();
    let mut client = timeout(Duration::from_secs(2), start)
        .await
        .unwrap()
        .unwrap();
    let mut state = client.subscribe_connection_state().unwrap();
    assert_eq!(
        *state.borrow_and_update(),
        ConnectionState::Connected { generation: 1 }
    );
    assert!(client.is_connected());
    let sender = client.get_sender().await.unwrap();
    drop(client);
    assert!(
        timeout(Duration::from_secs(2), state.changed())
            .await
            .unwrap()
            .is_err()
    );
    timeout(Duration::from_secs(2), sender.closed())
        .await
        .unwrap();
    assert_client_socket_released(peer).await;
    drop(stream);
    timeout(Duration::from_secs(2), relay)
        .await
        .unwrap()
        .unwrap();
}

#[cfg(feature = "client")]
#[tokio::test]
async fn client_connect_deadline_is_shared_by_kcp_tls_and_confirmation() {
    use crate::{
        application::client::{client_config::LynnClientConfigBuilder, lynn_client::LynnClient},
        infrastructure::tls::tls_config::TlsClientConfig,
    };
    use tokio::{
        io::{AsyncBufReadExt, BufReader},
        time::{Instant, advance, pause, resume},
    };
    let certs = Certificates::new();
    let (_, acceptor) = super::tls_configs(&certs);
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(socket.local_addr().unwrap())
        .unwrap()
        .with_kcp(KcpConfig::default())
        .with_tls(TlsClientConfig {
            ca_cert_path: Some(certs.ca.clone()),
            ..Default::default()
        })
        .with_connect_timeout_secs(&10)
        .with_reconnect_max_attempts(&1)
        .build();
    let client = LynnClient::new_with_config(config).await;
    let started = Instant::now();
    let start = tokio::spawn(client.start());
    let mut bytes = [0; 1400];
    let (length, peer) = timeout(Duration::from_secs(2), socket.recv_from(&mut bytes))
        .await
        .unwrap()
        .unwrap();
    pause();
    advance(Duration::from_secs(4)).await;
    resume();
    assert!(!start.is_finished());

    let (kcp, relay) = accept_kcp(socket, peer, bytes[..length].to_vec()).await;
    let mut kcp = BufReader::new(kcp);
    let client_hello = timeout(Duration::from_secs(2), kcp.fill_buf())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(client_hello.first(), Some(&22));
    pause();
    advance(Duration::from_secs(4)).await;
    resume();
    assert!(!start.is_finished());

    let stream = timeout(Duration::from_secs(2), acceptor.accept(kcp))
        .await
        .unwrap()
        .unwrap();
    assert!(
        !start.is_finished(),
        "TLS completion still requires encrypted server confirmation"
    );
    // KCP and TLS have consumed eight seconds of the same ten-second attempt.
    pause();
    advance(Duration::from_secs(3)).await;
    resume();
    let mut client = timeout(Duration::from_secs(1), start)
        .await
        .expect("confirmation must use the original deadline, not a renewed stage timeout")
        .unwrap();
    assert!(started.elapsed() >= Duration::from_secs(10));
    assert!(!client.is_connected());
    assert!(client.subscribe_connection_state().is_none());
    assert!(client.get_sender().await.is_none());
    assert_client_socket_released(peer).await;
    drop(stream);
    timeout(Duration::from_secs(2), relay)
        .await
        .unwrap()
        .unwrap();
}
