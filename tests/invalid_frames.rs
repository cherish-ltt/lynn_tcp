#![cfg(all(feature = "server", feature = "client"))]

use std::time::Duration;

use lynn_tcp::{
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_server::{LynnServer, LynnServerConfigBuilder},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};

fn invalid_frame() -> Vec<u8> {
    let mut frame = 9177u16.to_le_bytes().to_vec();
    frame.extend_from_slice(&5u64.to_le_bytes());
    frame.extend_from_slice(&[1, 1, 0, 0, 0]);
    frame
}

async fn assert_closed(stream: &mut (impl AsyncRead + Unpin)) {
    timeout(Duration::from_secs(3), async {
        let mut buf = [0; 1024];
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {},
            }
        }
    })
    .await
    .expect("invalid frame must close the transport, not wait for more bytes");
}

async fn send_invalid(stream: &mut (impl AsyncRead + AsyncWrite + Unpin)) {
    let frame = invalid_frame();
    stream.write_all(&frame[..10]).await.unwrap();
    stream.write_all(&frame[10..]).await.unwrap();
    assert_closed(stream).await;
}

async fn connect_when_ready(addr: &str) -> TcpStream {
    timeout(Duration::from_secs(3), async {
        loop {
            match TcpStream::connect(addr).await {
                Ok(stream) => return stream,
                Err(_) => sleep(Duration::from_millis(10)).await,
            }
        }
    })
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_closes_invalid_frame_transport() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    drop(listener);
    let config = LynnServerConfigBuilder::new()
        .with_addr(&addr)
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .with_max_connections_per_ip(&0)
        .build();
    let server = tokio::spawn(LynnServer::new_with_config(config).await.start());
    let mut stream = connect_when_ready(&addr).await;
    send_invalid(&mut stream).await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_closes_invalid_frame_transport() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(listener.local_addr().unwrap())
        .unwrap()
        .with_reconnect_interval_secs(&60)
        .build();
    let client = LynnClient::new_with_config(config).await.start().await;
    let (mut peer, _) = listener.accept().await.unwrap();
    send_invalid(&mut peer).await;
    drop(client);
}

#[tokio::test]
async fn client_invalid_frame_cancels_backpressured_write() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(listener.local_addr().unwrap())
        .unwrap()
        .with_server_single_channel_size(&1)
        .with_reconnect_interval_secs(&60)
        .build();
    let mut client = LynnClient::new_with_config(config).await.start().await;
    let sender = client.get_sender().await.unwrap();
    let (mut peer, _) = listener.accept().await.unwrap();
    drop(listener);
    let data = lynn_tcp::bytes::Bytes::from(vec![0; 4 * 1024 * 1024]);
    timeout(Duration::from_secs(3), async {
        loop {
            let send = sender.send(
                lynn_tcp::lynn_tcp_dependents::HandlerResult::new_with_send_to_server(
                    1,
                    data.clone(),
                ),
            );
            if timeout(Duration::from_millis(100), send).await.is_err() {
                break;
            }
        }
    })
    .await
    .expect("the non-reading peer must cause backpressure");
    peer.write_all(&invalid_frame()).await.unwrap();
    timeout(Duration::from_secs(3), async {
        while client.is_connected() {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("protocol error must finish the blocked writer before reconnecting");
    assert_closed(&mut peer).await;
}

#[cfg(feature = "tls")]
fn tls_configs() -> (rustls::ServerConfig, rustls::ClientConfig, String, String) {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
    let server = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der()).into(),
        )
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.der().clone()).unwrap();
    let client = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    (server, client, cert.pem(), signing_key.serialize_pem())
}

#[cfg(feature = "tls")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_closes_invalid_frame_tls_transport() {
    let (_, client_tls, cert, key) = tls_configs();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let port = listener.local_addr().unwrap().port();
    let dir =
        std::env::temp_dir().join(format!("lynn-invalid-frame-{}-{port}", std::process::id()));
    std::fs::create_dir(&dir).unwrap();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, cert).unwrap();
    std::fs::write(&key_path, key).unwrap();
    drop(listener);
    let config = LynnServerConfigBuilder::new()
        .with_addr(&addr)
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .with_max_connections_per_ip(&0)
        .with_tls(lynn_tcp::lynn_tls::TlsServerConfig::new(
            cert_path.to_str().unwrap(),
            key_path.to_str().unwrap(),
        ))
        .build();
    let server = tokio::spawn(LynnServer::new_with_config(config).await.start());
    let tcp = connect_when_ready(&addr).await;
    let mut stream = tokio_rustls::TlsConnector::from(std::sync::Arc::new(client_tls))
        .connect("localhost".try_into().unwrap(), tcp)
        .await
        .unwrap();
    send_invalid(&mut stream).await;
    server.abort();
    std::fs::remove_file(cert_path).unwrap();
    std::fs::remove_file(key_path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[cfg(feature = "tls")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_closes_invalid_frame_tls_transport() {
    let (server_tls, _, cert, _) = tls_configs();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let cert_path = std::env::temp_dir().join(format!(
        "lynn-invalid-frame-ca-{}-{}.pem",
        std::process::id(),
        addr.port()
    ));
    std::fs::write(&cert_path, cert).unwrap();
    let peer = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut stream = tokio_rustls::TlsAcceptor::from(std::sync::Arc::new(server_tls))
            .accept(tcp)
            .await
            .unwrap();
        send_invalid(&mut stream).await;
    });
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)
        .unwrap()
        .with_reconnect_interval_secs(&60)
        .with_tls(
            lynn_tcp::lynn_tls::TlsClientConfigBuilder::new()
                .with_ca_cert_path(cert_path.to_str().unwrap())
                .with_server_name("localhost")
                .build(),
        )
        .build();
    let client = LynnClient::new_with_config(config).await.start().await;
    timeout(Duration::from_secs(5), peer)
        .await
        .unwrap()
        .unwrap();
    drop(client);
    std::fs::remove_file(cert_path).unwrap();
}
