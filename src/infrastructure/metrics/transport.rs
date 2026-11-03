use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use prometheus::Counter;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

use super::metrics::METRICS;

pub(crate) struct ConnectionObservation {
    pub(crate) established: bool,
}

impl ConnectionObservation {
    pub(crate) fn accepted() -> Self {
        METRICS.connections.total.inc();
        METRICS.connections.active.inc();
        Self { established: false }
    }
}

impl Drop for ConnectionObservation {
    fn drop(&mut self) {
        METRICS.connections.closed_total.inc();
        if !self.established {
            METRICS.connections.failed_total.inc();
        }
        METRICS.connections.active.dec();
    }
}

pub(crate) struct MeasuredIo<T> {
    inner: T,
    received: Counter,
    sent: Counter,
}

impl<T> MeasuredIo<T> {
    pub(crate) fn new(inner: T) -> Self {
        Self {
            inner,
            received: METRICS.network.bytes_received_total.clone(),
            sent: METRICS.network.bytes_sent_total.clone(),
        }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for MeasuredIo<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let before = buf.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(cx, buf);
        if matches!(result, Poll::Ready(Ok(()))) {
            this.received.inc_by((buf.filled().len() - before) as f64);
        }
        result
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for MeasuredIo<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(written)) = &result {
            this.sent.inc_by(*written as f64);
        }
        result
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn partial_io_pending_cancellation_and_eof_count_only_completed_bytes() {
        let (inner, mut peer) = tokio::io::duplex(3);
        let mut measured = MeasuredIo {
            inner,
            received: Counter::new("test_received", "test").unwrap(),
            sent: Counter::new("test_sent", "test").unwrap(),
        };
        let sent = measured.sent.clone();
        assert_eq!(measured.write(b"abcdef").await.unwrap(), 3);
        assert_eq!(sent.get(), 3.0);
        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(10),
                measured.write_all(b"x")
            )
            .await
            .is_err()
        );
        assert_eq!(sent.get(), 3.0);
        peer.write_all(b"xy").await.unwrap();
        let mut bytes = [0; 8];
        assert_eq!(measured.read(&mut bytes).await.unwrap(), 2);
        assert_eq!(measured.received.get(), 2.0);
        drop(peer);
        assert_eq!(measured.read(&mut bytes).await.unwrap(), 0);
        assert_eq!(measured.received.get(), 2.0);
        assert!(measured.write(b"z").await.is_err());
        assert_eq!(sent.get(), 3.0);
    }
}
