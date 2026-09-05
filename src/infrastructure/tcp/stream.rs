//! Transport stream abstraction.
//!
//! The reactor pipeline originally worked directly on `tokio::net::TcpStream`.
//! To support optional TLS while keeping the plain path unchanged, all
//! connection-handling code operates on `LynnStream`, a thin enum over the
//! plain TCP stream and the TLS-wrapped stream.

use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpStream,
    time,
};
use tracing::warn;

use crate::const_config::TLS_HANDSHAKE_TIMEOUT_SECS;

/// A boxed dynamic write half used by `LynnUser`, so the clients map does not
/// depend on the concrete transport type.
pub(crate) type BoxedWriteHalf = Box<dyn AsyncWrite + Send + Unpin>;

/// A boxed dynamic read half handed to the per-connection read loop.
pub(crate) type BoxedReadHalf = Box<dyn AsyncRead + Send + Unpin>;

/// The transport stream of a single connection.
pub(crate) enum LynnStream {
    /// Plain TCP stream (default, TLS feature disabled or not configured).
    Plain(TcpStream),
    /// TLS 1.3 wrapped stream (requires the `tls` feature).
    #[cfg(feature = "tls")]
    Tls(tokio_rustls::TlsStream<TcpStream>),
}

impl AsyncRead for LynnStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_read(cx, buf),
            #[cfg(feature = "tls")]
            Self::Tls(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for LynnStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_write(cx, buf),
            #[cfg(feature = "tls")]
            Self::Tls(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_flush(cx),
            #[cfg(feature = "tls")]
            Self::Tls(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Plain(stream) => Pin::new(stream).poll_shutdown(cx),
            #[cfg(feature = "tls")]
            Self::Tls(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

/// Splits a transport into a boxed read half (for the per-connection read
/// loop) and a boxed write half (for the clients-map writer task).
pub(crate) fn split_transport(stream: LynnStream) -> (BoxedReadHalf, BoxedWriteHalf) {
    let (read_half, write_half) = tokio::io::split(stream);
    (Box::new(read_half), Box::new(write_half))
}

/// Wraps a freshly accepted TCP stream into a [`LynnStream`], performing the
/// TLS handshake when the server was configured with TLS.
pub(crate) enum StreamAcceptor {
    /// Accept connections without TLS.
    Plain,
    /// Accept connections after a TLS 1.3 handshake.
    #[cfg(feature = "tls")]
    Tls(Arc<tokio_rustls::TlsAcceptor>),
}

impl StreamAcceptor {
    /// Wraps the given raw TCP stream, returning the ready-to-use transport.
    ///
    /// Returns `None` when the TLS handshake failed or timed out; the caller
    /// must drop the connection in that case.
    pub(crate) async fn accept(&self, stream: TcpStream, addr: SocketAddr) -> Option<LynnStream> {
        match self {
            Self::Plain => Some(LynnStream::Plain(stream)),
            #[cfg(feature = "tls")]
            Self::Tls(acceptor) => {
                let handshake = time::timeout(
                    Duration::from_secs(TLS_HANDSHAKE_TIMEOUT_SECS),
                    acceptor.accept(stream),
                );
                match handshake.await {
                    Ok(Ok(tls_stream)) => Some(LynnStream::Tls(tls_stream.into())),
                    Ok(Err(e)) => {
                        warn!(
                            "TLS handshake with {} failed: {}, closing connection",
                            addr, e
                        );
                        None
                    },
                    Err(_) => {
                        warn!(
                            "TLS handshake with {} timed out after {}s, closing connection",
                            addr, TLS_HANDSHAKE_TIMEOUT_SECS
                        );
                        None
                    },
                }
            },
        }
    }
}
