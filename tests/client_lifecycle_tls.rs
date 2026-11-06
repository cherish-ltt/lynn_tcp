#![cfg(all(feature = "client", feature = "tls"))]

use std::{sync::Arc, time::Duration};

use lynn_tcp::{
    lynn_client::{ConnectionState, LynnClient, LynnClientConfigBuilder},
    lynn_tls::TlsClientConfigBuilder,
};
use tokio::{io::AsyncReadExt, net::TcpListener, time::timeout};

#[tokio::test]
async fn state_tracks_authenticated_tls_reconnect_and_owner_drop() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert_path = std::env::temp_dir().join(format!(
        "lynn_client_drop_ca_{}_{}.pem",
        std::process::id(),
        listener.local_addr().unwrap().port()
    ));
    std::fs::write(&cert_path, cert.pem()).unwrap();
    let server_config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
    )
    .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server_config));
    let client_config = LynnClientConfigBuilder::new()
        .with_server_addr(listener.local_addr().unwrap())
        .unwrap()
        .with_tls(
            TlsClientConfigBuilder::new()
                .with_ca_cert_path(cert_path.to_string_lossy())
                .with_server_name("localhost")
                .build(),
        )
        .build();
    let mut client = tokio::spawn(async move {
        LynnClient::new_with_config(client_config)
            .await
            .start()
            .await
    });
    let (socket, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(
        timeout(Duration::from_millis(50), &mut client)
            .await
            .is_err(),
        "TCP connect must not publish readiness before the TLS handshake"
    );
    let mut peer = timeout(Duration::from_secs(2), acceptor.accept(socket))
        .await
        .unwrap()
        .unwrap();
    let mut client = timeout(Duration::from_secs(2), client)
        .await
        .unwrap()
        .unwrap();
    std::fs::remove_file(cert_path).unwrap();
    assert!(client.is_connected());
    let mut state = client.subscribe_connection_state().unwrap();
    assert_eq!(
        *state.borrow_and_update(),
        ConnectionState::Connected { generation: 1 }
    );
    let sender = client.get_sender().await.unwrap();
    let mut heartbeat = [0; 15];
    timeout(Duration::from_secs(2), peer.read_exact(&mut heartbeat))
        .await
        .unwrap()
        .unwrap();

    drop(peer);
    timeout(
        Duration::from_secs(2),
        state.wait_for(|value| *value == ConnectionState::Reconnecting { generation: 1 }),
    )
    .await
    .unwrap()
    .unwrap();
    let (socket, _) = timeout(Duration::from_secs(3), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(
        timeout(
            Duration::from_millis(50),
            state.wait_for(|value| matches!(value, ConnectionState::Connected { .. })),
        )
        .await
        .is_err(),
        "a replacement TCP socket must remain Reconnecting until TLS is ready"
    );
    assert!(!client.is_connected());
    let mut peer = timeout(Duration::from_secs(2), acceptor.accept(socket))
        .await
        .unwrap()
        .unwrap();
    timeout(
        Duration::from_secs(2),
        state.wait_for(|value| *value == ConnectionState::Connected { generation: 2 }),
    )
    .await
    .unwrap()
    .unwrap();

    drop(client);

    timeout(Duration::from_secs(2), sender.closed())
        .await
        .expect("TLS owner drop must close its sender");
    assert!(
        timeout(Duration::from_secs(2), state.changed())
            .await
            .unwrap()
            .is_err()
    );
    let closed = timeout(Duration::from_secs(2), peer.read(&mut heartbeat))
        .await
        .expect("TLS transport must close promptly");
    match closed {
        Ok(n) => assert_eq!(n, 0),
        // Owner cancellation does not promise close_notify or drain TLS tickets.
        Err(error) => assert!(matches!(
            error.kind(),
            std::io::ErrorKind::UnexpectedEof | std::io::ErrorKind::ConnectionReset
        )),
    }
}
