#![cfg(all(feature = "client", feature = "tls"))]

use std::{sync::Arc, time::Duration};

use lynn_tcp::{
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_tls::TlsClientConfigBuilder,
};
use tokio::{io::AsyncReadExt, net::TcpListener, time::timeout};

#[tokio::test]
async fn drop_closes_authenticated_tls_connection() {
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
    let client = tokio::spawn(async move {
        LynnClient::new_with_config(client_config)
            .await
            .start()
            .await
    });
    let (socket, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
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
    let sender = client.get_sender().await.unwrap();
    let mut heartbeat = [0; 15];
    timeout(Duration::from_secs(2), peer.read_exact(&mut heartbeat))
        .await
        .unwrap()
        .unwrap();

    drop(client);

    timeout(Duration::from_secs(2), sender.closed())
        .await
        .expect("TLS owner drop must close its sender");
    let closed = timeout(Duration::from_secs(2), peer.read(&mut heartbeat))
        .await
        .expect("TLS transport must close promptly");
    match closed {
        Ok(n) => assert_eq!(n, 0),
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof),
    }
}
