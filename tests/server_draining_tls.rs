#![cfg(all(feature = "server", feature = "tls"))]

use lynn_tcp::{
    lynn_server::SessionSendOutcome,
    lynn_server::{LynnServer, LynnServerConfigBuilder, ShutdownOutcome},
    lynn_state::AppState,
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
    lynn_tls::TlsServerConfig,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Notify, mpsc, oneshot},
    time::{sleep, timeout},
};

struct Gate {
    started: Notify,
    released: Notify,
}

async fn delayed(input: InputBufVO, gate: AppState<Arc<Gate>>) -> HandlerResult {
    gate.started.notify_one();
    gate.released.notified().await;
    HandlerResult::new_with_send(1, "tls-done".into(), vec![input.get_input_addr().unwrap()])
        .unwrap()
}

#[tokio::test]
async fn tls_drain_sends_close_notify_and_cancels_pending_handshakes() {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let base = std::env::temp_dir().join(format!("lynn-drain-tls-{}", std::process::id()));
    let cert_path = base.with_extension("pem");
    let key_path = base.with_extension("key");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, signing_key.serialize_pem()).unwrap();
    let gate = Arc::new(Gate {
        started: Notify::new(),
        released: Notify::new(),
    });
    let weak = Arc::downgrade(&gate);
    let config = LynnServerConfigBuilder::new()
        .with_addr("127.0.0.1:0")
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .with_server_max_connections(Some(&2))
        .with_max_connections_per_ip(&2)
        .with_tls(TlsServerConfig::new(
            cert_path.to_str().unwrap(),
            key_path.to_str().unwrap(),
        ))
        .build();
    let (registered, mut sessions) = mpsc::unbounded_channel();
    let server = LynnServer::new_with_config(config)
        .await
        .with_state(gate.clone())
        .add_router(1, delayed)
        .with_middleware(move |request, next| {
            registered.send(request.session().clone()).unwrap();
            next.run(request)
        });
    let ready = server.startup_handle();
    let (stop, signal) = oneshot::channel();
    let task = tokio::spawn(server.start_with_shutdown(
        async {
            let _ = signal.await;
        },
        Duration::from_secs(3),
    ));
    let addr = timeout(Duration::from_secs(3), ready.wait())
        .await
        .unwrap()
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.der().clone()).unwrap();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
    let mut peer = connector
        .connect(
            "localhost".try_into().unwrap(),
            TcpStream::connect(addr).await.unwrap(),
        )
        .await
        .unwrap();
    let mut frame = 9177u16.to_le_bytes().to_vec();
    frame.extend_from_slice(&5u64.to_le_bytes());
    frame.extend_from_slice(&[1, 1, 0]);
    frame.extend_from_slice(&7719u16.to_le_bytes());
    peer.write_all(&frame).await.unwrap();
    peer.flush().await.unwrap();
    timeout(Duration::from_secs(3), gate.started.notified())
        .await
        .unwrap();
    let session = timeout(Duration::from_secs(3), sessions.recv())
        .await
        .unwrap()
        .unwrap();
    let mut pending_handshake = TcpStream::connect(addr).await.unwrap();
    pending_handshake.write_all(&[22, 3, 3]).await.unwrap();
    sleep(Duration::from_millis(20)).await;
    stop.send(()).unwrap();
    timeout(Duration::from_secs(3), async {
        while TcpStream::connect(addr).await.is_ok() {
            sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        session.try_send(2, "rejected".into()).unwrap(),
        SessionSendOutcome::Closed
    );
    assert!(!session.observer().is_closed());
    let result = timeout(Duration::from_secs(2), pending_handshake.read(&mut [0]))
        .await
        .unwrap();
    assert!(matches!(result, Ok(0)) || result.is_err());
    assert!(!task.is_finished());
    gate.released.notify_one();
    drop(gate);
    let mut received = Vec::new();
    timeout(Duration::from_secs(3), peer.read_to_end(&mut received))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&received[13..received.len() - 2], b"tls-done");
    assert_eq!(
        timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        ShutdownOutcome::Drained
    );
    assert!(session.observer().is_closed());
    assert!(weak.upgrade().is_none());
    drop(TcpListener::bind(addr).await.unwrap());
    std::fs::remove_file(cert_path).unwrap();
    std::fs::remove_file(key_path).unwrap();
}
