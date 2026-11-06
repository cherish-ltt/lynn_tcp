use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};

use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::watch,
};

/// The shared transport outlives both split halves. Field order ensures transport
/// destruction happens before observers receive the final close notification.
pub(crate) struct SessionIo<T> {
    inner: T,
    completion: watch::Sender<()>,
}

impl<T> SessionIo<T> {
    pub(crate) fn new(inner: T, completion: watch::Sender<()>) -> Self {
        Self { inner, completion }
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for SessionIo<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for SessionIo<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
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
    use crate::domain::model::session::SessionObserver;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tokio::io::AsyncWriteExt;

    struct PendingShutdown {
        observer: SessionObserver,
        destroyed: Arc<AtomicBool>,
    }

    impl Drop for PendingShutdown {
        fn drop(&mut self) {
            assert!(!self.observer.is_closed());
            self.destroyed.store(true, Ordering::Release);
        }
    }

    impl AsyncRead for PendingShutdown {
        fn poll_read(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    impl AsyncWrite for PendingShutdown {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(bytes.len()))
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Pending
        }
    }

    #[tokio::test]
    async fn notice_waits_for_both_halves_and_transport_destruction() {
        for poll_writer in [false, true] {
            let (completion, observer) = SessionObserver::new();
            let destroyed = Arc::new(AtomicBool::new(false));
            let stream = SessionIo::new(
                PendingShutdown {
                    observer: observer.clone(),
                    destroyed: destroyed.clone(),
                },
                completion,
            );
            let (read, mut write) = tokio::io::split(stream);
            let writer = tokio::spawn(async move { write.shutdown().await.unwrap() });
            if poll_writer {
                tokio::task::yield_now().await;
            }
            drop(read);
            assert!(!observer.is_closed());
            writer.abort();
            assert!(!observer.is_closed());
            assert!(writer.await.unwrap_err().is_cancelled());
            observer.closed().await;
            assert!(observer.is_closed());
            assert!(destroyed.load(Ordering::Acquire));
        }
    }
}
