use super::*;
use std::{
    io,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};
use tokio::{io::AsyncWrite, time::timeout};

struct FlushWriter {
    fail: bool,
    flushed: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
}

impl AsyncWrite for FlushWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Ok(data.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.flushed.store(true, Ordering::SeqCst);
        if self.fail {
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "flush failed",
            )))
        } else {
            Poll::Pending
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

impl Drop for FlushWriter {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn flush_error_ends_writer_and_pending_flush_is_cancellable() {
    for fail in [true, false] {
        let flushed = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicBool::new(false));
        let writer = FlushWriter {
            fail,
            flushed: flushed.clone(),
            dropped: dropped.clone(),
        };
        let (tx, mut rx) = mpsc::channel(1);
        tx.send(HandlerResult::new_with_send_to_server(1, "message".into()))
            .await
            .unwrap();
        let pump = tokio::spawn(async move { write_pump(writer, 9177, 7719, &mut rx).await });
        timeout(Duration::from_secs(2), async {
            while !flushed.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("each frame must flush");
        if fail {
            timeout(Duration::from_secs(2), pump)
                .await
                .unwrap()
                .unwrap();
        } else {
            assert!(!pump.is_finished());
            pump.abort();
            assert!(pump.await.unwrap_err().is_cancelled());
        }
        assert!(dropped.load(Ordering::SeqCst));
        assert!(tx.is_closed());
    }
}

#[cfg(feature = "tls")]
#[tokio::test]
async fn tls_single_frame_flushes_after_backpressure_without_another_write() {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
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
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(server));
    let connector = tokio_rustls::TlsConnector::from(Arc::new(client));
    let (client_io, server_io) = tokio::io::duplex(256);
    let (client, peer) = tokio::join!(
        connector.connect("localhost".try_into().unwrap(), client_io),
        acceptor.accept(server_io)
    );
    let (_, writer) = tokio::io::split(client.unwrap());
    let mut peer = peer.unwrap();
    let (tx, mut rx) = mpsc::channel(1);
    let message = HandlerResult::new_with_send_to_server(1, vec![7; 1024].into());
    let expected = message.get_response_data().unwrap();
    tx.send(message).await.unwrap();
    let pump = tokio::spawn(async move { write_pump(writer, 9177, 7719, &mut rx).await });
    let mut received = vec![0; expected.len()];
    let result = timeout(Duration::from_millis(500), peer.read_exact(&mut received)).await;
    pump.abort();
    let _ = pump.await;
    result
        .expect("single TLS frame must arrive without later writes")
        .unwrap();
    assert_eq!(received, expected);
    drop(tx);
}
