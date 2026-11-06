use super::{
    Certificates, KcpConfig,
    frames::{connect, frame},
    tls_configs, tls_pair,
};
use crate::{
    application::server::server_tasks::ServerTasks,
    infrastructure::{
        tcp::stream::{LynnStream, split_transport},
        tls::tls_provider::build_client_tls_mode,
    },
    lynn_middleware::RequestContext,
    lynn_server::{LynnServer, LynnServerConfigBuilder, SessionHandle, SessionSendOutcome},
    lynn_state::AppState,
    lynn_tcp_dependents::HandlerResult,
    lynn_tls::{TlsClientConfig, TlsServerConfig},
};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::UdpSocket,
    sync::mpsc,
    time::timeout,
};

const LIMIT: Duration = Duration::from_secs(3);

async fn receive(peer: &mut (impl AsyncRead + Unpin), payload: &[u8]) {
    let expected = frame(payload);
    let mut actual = vec![0; expected.len()];
    timeout(LIMIT, peer.read_exact(&mut actual))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn secure_observer_waits_for_both_tls_halves_and_driver_destruction() {
    let certs = Certificates::new();
    let (connector, acceptor) = tls_configs(&certs);
    let (_owner, tasks) = ServerTasks::new();
    let (client, mut server, sockets) = tls_pair(&connector, &acceptor).await;
    server.get_mut().0.track_server_worker(&tasks);
    let driver = server.get_ref().0.completion();
    let (read, write, observer) = split_transport(
        LynnStream::KcpTls(Box::new(server.into())),
        tasks.connection(),
    );
    drop(read);
    assert!(!observer.is_closed());
    drop(write);
    // This single-thread runtime has not yet processed the driver's abort request.
    assert!(driver.has_changed().is_ok());
    assert!(!observer.is_closed());
    timeout(LIMIT, observer.closed()).await.unwrap();
    assert!(observer.is_closed());
    assert!(driver.has_changed().is_err());
    timeout(LIMIT, tasks.drained()).await.unwrap();
    timeout(LIMIT, tasks.finished()).await.unwrap();
    drop(client);
    timeout(LIMIT, async {
        while sockets.iter().any(|socket| socket.upgrade().is_some()) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("retained observers must not keep either KCP socket alive");
}

#[tokio::test]
async fn secure_private_push_and_close_observation_preserve_original_sessions() {
    async fn register(
        context: RequestContext,
        registrations: AppState<mpsc::UnboundedSender<SessionHandle>>,
    ) -> HandlerResult {
        registrations.send(context.session().clone()).unwrap();
        HandlerResult::new_with_send(1, "registered".into(), vec![context.peer_addr()]).unwrap()
    }
    let certs = Certificates::new();
    let connector = build_client_tls_mode(
        &TlsClientConfig {
            ca_cert_path: Some(certs.ca.clone()),
            client_cert_path: Some(certs.client_cert.clone()),
            client_key_path: Some(certs.client_key.clone()),
            ..Default::default()
        },
        "127.0.0.1:1",
        true,
    )
    .unwrap();
    let state = Arc::new(());
    let weak_state = Arc::downgrade(&state);
    let (registered, mut registrations) = mpsc::unbounded_channel::<SessionHandle>();
    let server = LynnServer::new_with_config(
        LynnServerConfigBuilder::new()
            .with_addr("127.0.0.1:0")
            .unwrap()
            .with_kcp(KcpConfig::default())
            .with_tls(
                TlsServerConfig::new(&certs.server_cert, &certs.server_key)
                    .with_client_ca(&certs.ca),
            )
            .with_server_max_taskpool_size(&2)
            .with_connection_rate_limit(&0)
            .with_max_connections_per_ip(&2)
            .with_send_buffer_size(&0)
            .build(),
    )
    .await
    .with_state(state)
    .with_state(registered)
    .add_router(1, register);
    let ready = server.startup_handle();
    let task = tokio::spawn(server.start());
    let addr = timeout(LIMIT, ready.wait()).await.unwrap().unwrap();
    let mut peers = Vec::new();
    let mut handles = Vec::new();
    for _ in 0..2 {
        let mut peer = connect(addr, &connector).await;
        timeout(LIMIT, async {
            peer.write_all(&frame(b"register")).await.unwrap();
            peer.flush().await.unwrap();
        })
        .await
        .unwrap();
        handles.push(timeout(LIMIT, registrations.recv()).await.unwrap().unwrap());
        receive(&mut peer, b"registered").await;
        peers.push(peer);
    }
    assert_ne!(handles[0].id(), handles[1].id());
    let first = handles[0].observer();
    let second = handles[1].observer();
    assert_eq!(first.id(), handles[0].id());
    assert!(!first.is_closed());
    assert!(!second.is_closed());
    assert_eq!(
        handles[0].try_send(1, "private A".into()).unwrap(),
        SessionSendOutcome::Enqueued
    );
    receive(&mut peers[0], b"private A").await;
    assert!(
        timeout(Duration::from_millis(25), peers[1].read(&mut [0]))
            .await
            .is_err()
    );
    assert!(
        timeout(Duration::from_millis(25), first.closed())
            .await
            .is_err()
    );
    timeout(LIMIT, peers[0].shutdown()).await.unwrap().unwrap();
    assert_eq!(
        timeout(LIMIT, peers[0].read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    timeout(LIMIT, first.closed()).await.unwrap();
    assert_eq!(
        handles[0].try_send(1, "closed A".into()).unwrap(),
        SessionSendOutcome::Closed
    );
    assert!(!second.is_closed());
    assert_eq!(
        handles[1].try_send(1, "private B".into()).unwrap(),
        SessionSendOutcome::Enqueued
    );
    receive(&mut peers[1], b"private B").await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    timeout(LIMIT, second.closed()).await.unwrap();
    assert_eq!(
        handles[1].try_send(1, "closed B".into()).unwrap(),
        SessionSendOutcome::Closed
    );
    timeout(LIMIT, async {
        while weak_state.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
        loop {
            match UdpSocket::bind(addr).await {
                Ok(socket) => break drop(socket),
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
                    tokio::task::yield_now().await;
                },
                Err(error) => panic!("listener rebind failed: {error}"),
            }
        }
    })
    .await
    .expect("retained handles and observers must release AppState and the UDP listener");
}
