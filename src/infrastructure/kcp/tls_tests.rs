use super::{KcpConfig, Mode, PACKETS, stream::KcpStream};
use crate::infrastructure::tls::{
    tls_config::{TlsClientConfig, TlsServerConfig},
    tls_provider::{build_client_tls_mode, build_server_acceptor_mode},
};
use std::{
    io,
    pin::Pin,
    sync::{Arc, Mutex, Weak},
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    net::UdpSocket,
    sync::mpsc,
    time::timeout,
};
#[path = "../../../tests/support/tls_certificates.rs"]
mod certificates;
use certificates::Certificates;

#[path = "tls_lifecycle_tests.rs"]
mod lifecycle;

#[cfg(feature = "server")]
#[path = "tls_frame_tests.rs"]
mod frames;

#[cfg(feature = "server")]
#[path = "tls_session_tests.rs"]
mod sessions;

struct RecordingIo {
    inner: KcpStream,
    written: Arc<Mutex<Vec<u8>>>,
    corrupt: bool,
}
impl AsyncRead for RecordingIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}
impl AsyncWrite for RecordingIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut modified = Vec::new();
        let buf = if self.corrupt && !buf.is_empty() {
            modified.extend_from_slice(buf);
            *modified.last_mut().unwrap() ^= 1;
            modified.as_slice()
        } else {
            buf
        };
        let result = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = result {
            self.written.lock().unwrap().extend_from_slice(&buf[..n]);
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

async fn pair() -> (KcpStream, KcpStream, [Weak<UdpSocket>; 2]) {
    let a = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let b = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let sockets = [Arc::downgrade(&a), Arc::downgrade(&b)];
    a.connect(b.local_addr().unwrap()).await.unwrap();
    let (tx, rx) = mpsc::channel(PACKETS);
    let (left, ready) = KcpStream::start(
        a.clone(),
        b.local_addr().unwrap(),
        42,
        KcpConfig::default(),
        Mode::Tls,
        None,
    )
    .unwrap();
    let (right, other) = KcpStream::start(
        b.clone(),
        a.local_addr().unwrap(),
        42,
        KcpConfig::default(),
        Mode::Tls,
        Some(rx),
    )
    .unwrap();
    tokio::spawn(async move {
        let mut bytes = [0; 65536];
        loop {
            tokio::select! {
                _ = tx.closed() => break,
                packet = b.recv_from(&mut bytes) => {
                    let (n, _) = packet.unwrap();
                    if tx.send(bytes[..n].to_vec()).await.is_err() { break; }
                },
            }
        }
    });
    timeout(Duration::from_secs(3), async {
        ready.await.unwrap().unwrap();
        other.await.unwrap().unwrap();
    })
    .await
    .unwrap();
    (left, right, sockets)
}

fn tls_configs(
    certs: &Certificates,
) -> (
    crate::infrastructure::tls::tls_provider::ClientTls,
    tokio_rustls::TlsAcceptor,
) {
    let client = build_client_tls_mode(
        &TlsClientConfig {
            ca_cert_path: Some(certs.ca.clone()),
            ..Default::default()
        },
        "127.0.0.1:1",
        true,
    )
    .unwrap();
    let acceptor = build_server_acceptor_mode(
        &TlsServerConfig::new(&certs.server_cert, &certs.server_key),
        true,
    )
    .unwrap();
    (client, acceptor)
}

async fn tls_pair(
    client: &crate::infrastructure::tls::tls_provider::ClientTls,
    acceptor: &tokio_rustls::TlsAcceptor,
) -> (
    tokio_rustls::client::TlsStream<RecordingIo>,
    tokio_rustls::server::TlsStream<KcpStream>,
    [Weak<UdpSocket>; 2],
) {
    let (left, right, sockets) = pair().await;
    let (left, right) = timeout(Duration::from_secs(3), async {
        tokio::join!(
            client.connector.connect(
                client.server_name.clone(),
                RecordingIo {
                    inner: left,
                    written: Arc::default(),
                    corrupt: false,
                }
            ),
            acceptor.accept(right)
        )
    })
    .await
    .unwrap();
    (left.unwrap(), right.unwrap(), sockets)
}

#[tokio::test]
async fn tls13_full_handshakes_small_flush_clean_close_and_truncated_eof() {
    let certs = Certificates::new();
    let (connector, acceptor) = tls_configs(&certs);
    for _ in 0..2 {
        let (mut client, mut server, _) = tls_pair(&connector, &acceptor).await;
        assert_eq!(
            client.get_ref().1.protocol_version(),
            Some(rustls::ProtocolVersion::TLSv1_3)
        );
        assert_eq!(
            client.get_ref().1.handshake_kind(),
            Some(rustls::HandshakeKind::Full)
        );
        timeout(Duration::from_secs(3), async {
            client.write_all(b"one").await.unwrap();
            client.flush().await.unwrap();
            let mut bytes = [0; 3];
            server.read_exact(&mut bytes).await.unwrap();
            assert_eq!(&bytes, b"one");
            client.shutdown().await.unwrap();
            assert_eq!(server.read(&mut bytes).await.unwrap(), 0);
            server.shutdown().await.unwrap();
            assert_eq!(client.read(&mut bytes).await.unwrap(), 0);
        })
        .await
        .unwrap();
    }
    let (mut client, mut server, _) = tls_pair(&connector, &acceptor).await;
    client.get_mut().0.inner.shutdown().await.unwrap();
    let error = timeout(Duration::from_secs(3), server.read(&mut [0]))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
}

#[tokio::test]
async fn ciphertext_modification_and_cross_connection_replay_never_yield_plaintext() {
    let certs = Certificates::new();
    let (connector, acceptor) = tls_configs(&certs);
    let (mut client, mut server, _) = tls_pair(&connector, &acceptor).await;
    client.get_ref().0.written.lock().unwrap().clear();
    client
        .write_all(b"authenticated-business-frame")
        .await
        .unwrap();
    client.flush().await.unwrap();
    let mut bytes = [0; 28];
    server.read_exact(&mut bytes).await.unwrap();
    let ciphertext = client.get_ref().0.written.lock().unwrap().clone();
    assert!(ciphertext.len() > bytes.len());
    drop((client, server));
    for corrupt in [false, true] {
        let (mut client, mut server, _) = tls_pair(&connector, &acceptor).await;
        if corrupt {
            client.get_mut().0.corrupt = true;
            client.write_all(b"modified business data").await.unwrap();
            client.flush().await.unwrap();
        } else {
            client
                .get_mut()
                .0
                .inner
                .write_all(&ciphertext)
                .await
                .unwrap();
            client.get_mut().0.inner.flush().await.unwrap();
        }
        let mut output = Vec::new();
        assert!(
            timeout(Duration::from_secs(3), server.read_to_end(&mut output))
                .await
                .unwrap()
                .is_err()
        );
        assert!(
            output.is_empty(),
            "unauthenticated TLS records must yield no business bytes"
        );
    }
}

#[tokio::test]
async fn secure_config_disables_tickets_and_rejects_insecure_and_partial_identity() {
    let certs = Certificates::new();
    let server = build_server_acceptor_mode(
        &TlsServerConfig::new(&certs.server_cert, &certs.server_key),
        true,
    )
    .unwrap();
    assert_eq!(server.config().send_tls13_tickets, 0);
    assert_eq!(server.config().max_tls13_tickets, 0);
    assert_eq!(server.config().max_early_data_size, 0);
    assert!(!server.config().send_half_rtt_data);
    assert!(!server.config().session_storage.can_cache());
    let mut config = TlsClientConfig {
        ca_cert_path: Some(certs.ca.clone()),
        ..Default::default()
    };
    let client = build_client_tls_mode(&config, "127.0.0.1:1", true).unwrap();
    assert!(!client.connector.config().enable_early_data);
    config.danger_accept_invalid_certs = true;
    assert!(build_client_tls_mode(&config, "127.0.0.1:1", true).is_err());
    config.danger_accept_invalid_certs = false;
    config.client_cert_path = Some(certs.client_cert.clone());
    assert!(build_client_tls_mode(&config, "127.0.0.1:1", true).is_err());
    config.client_cert_path = None;
    config.client_key_path = Some(certs.client_key.clone());
    assert!(build_client_tls_mode(&config, "127.0.0.1:1", true).is_err());
    config.client_key_path = None;
    config.ca_cert_path = None;
    assert!(build_client_tls_mode(&config, "127.0.0.1:1", true).is_err());
}

#[tokio::test]
async fn stalled_tls_reader_applies_bounded_backpressure_and_cancel_releases_driver() {
    let certs = Certificates::new();
    let (connector, acceptor) = tls_configs(&certs);
    let (mut client, server, sockets) = tls_pair(&connector, &acceptor).await;
    assert!(
        timeout(Duration::from_millis(150), async {
            client.write_all(&vec![17; 10 * 1024 * 1024]).await.unwrap();
            client.flush().await.unwrap();
        })
        .await
        .is_err()
    );
    drop((client, server));
    timeout(Duration::from_secs(3), async {
        while sockets.iter().any(|socket| socket.upgrade().is_some()) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancelled TLS streams must release both drivers and the UDP relay");
}
