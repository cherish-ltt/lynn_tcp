use std::{net::SocketAddr, sync::Arc, time::SystemTime};

use dashmap::DashMap;

use bytes::BytesMut;
use tokio::{
    io::AsyncWriteExt,
    sync::{
        RwLock,
        mpsc::{Sender, channel},
        watch,
    },
    task::JoinHandle,
};
use tracing::error;

use crate::infrastructure::metrics::metrics::METRICS;
use crate::infrastructure::tcp::io_timeout::with_io_timeout;
use crate::{
    const_config::DEFAULT_SYSTEM_CHANNEL_SIZE, infrastructure::tcp::stream::BoxedWriteHalf,
};

pub(crate) enum LynnUserSignal {
    SendResponse(BytesMut),
    Shutdown,
}

/// Represents a user in the Lynn system.
///
/// This struct holds information about a user, including their sender channel, user ID,
/// process permit, last communicate time, and associated thread.
pub(crate) struct LynnUser {
    /// The last time the user communicated.
    last_communicate_time: Arc<RwLock<SystemTime>>,
    sender: Sender<LynnUserSignal>,

    close: watch::Sender<bool>,
}

/// Implementation of methods for the LynnUser struct.
impl LynnUser {
    /// Creates a new instance of LynnUser with the specified sender channel.
    ///
    /// # Parameters
    ///
    /// * `write_half` - The boxed write half of the connection transport.
    /// * `last_communicate_time` - The last time the user communicated.
    /// * `write_timeout_secs` - The response-write and shutdown timeout; zero disables it.
    ///
    /// # Returns
    ///
    /// The user, its writer task, and the connection-close notification receiver.
    pub(crate) fn new(
        write_half: BoxedWriteHalf,
        last_communicate_time: Arc<RwLock<SystemTime>>,
        write_timeout_secs: u64,
    ) -> (Self, JoinHandle<()>, watch::Receiver<bool>) {
        let (tx, mut rx) = channel(DEFAULT_SYSTEM_CHANNEL_SIZE);
        let (close, closed) = watch::channel(false);
        let main_join_handle = tokio::spawn(async move {
            let mut write_half = write_half;

            while let Some(LynnUserSignal::SendResponse(response)) = rx.recv().await {
                let write = async {
                    write_half.write_all(&response).await?;
                    write_half.flush().await
                };
                if let Err(e) = with_io_timeout(write_timeout_secs, write).await {
                    METRICS.messages.dropped_total.inc();
                    error!("Failed to write to socket: {}", e);
                    return;
                }
                METRICS.messages.sent_total.inc();
            }
            if let Err(e) = with_io_timeout(write_timeout_secs, write_half.shutdown()).await {
                error!("Failed to shut down socket: {}", e);
            }
        });
        (
            Self {
                last_communicate_time,
                sender: tx,
                close,
            },
            main_join_handle,
            closed,
        )
    }

    /// Gets a clone of the last communicate time.
    ///
    /// # Returns
    ///
    /// A clone of the last communicate time.
    #[inline(always)]
    pub(crate) fn get_last_communicate_time(&self) -> Arc<RwLock<SystemTime>> {
        self.last_communicate_time.clone()
    }

    pub(crate) fn response_sender(&self) -> Sender<LynnUserSignal> {
        self.sender.clone()
    }

    #[inline(always)]
    pub(crate) fn send_response(
        &self,
        response: &BytesMut,
    ) -> impl Future<Output = ()> + Send + use<> {
        let sender = self.sender.clone();
        let response = response.clone();
        async move {
            if let Err(e) = sender.send(LynnUserSignal::SendResponse(response)).await {
                METRICS.messages.dropped_total.inc();
                error!("Send response error:{}", e);
            }
        }
    }

    pub(crate) fn shutdown(&self) -> impl Future<Output = ()> + Send + use<> {
        let sender = self.sender.clone();
        async move {
            if let Err(e) = sender.send(LynnUserSignal::Shutdown).await {
                error!("Send shutdown error:{}", e);
            }
        }
    }
}

/// Implementation of the Drop trait for the LynnUser struct.
/// Type alias for the clients collection: a concurrent map from socket address to LynnUser.
pub(crate) type ClientsStructType = Arc<DashMap<SocketAddr, LynnUser>>;

/// Newtype wrapper for `ClientsStructType`.
#[derive(Clone)]
pub(crate) struct ClientsStruct(pub(crate) ClientsStructType);

impl Drop for LynnUser {
    /// Notifies the connection task that the user has been removed.
    ///
    /// # Parameters
    ///
    /// * `self` - The mutable reference to the LynnUser instance.
    fn drop(&mut self) {
        let _ = self.close.send(true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io,
        pin::Pin,
        sync::atomic::{AtomicUsize, Ordering},
        task::{Context, Poll},
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWrite},
        time::{sleep, timeout},
    };

    #[derive(Clone, Copy)]
    enum BlockedOperation {
        PartialWrite,
        Flush,
        Shutdown,
    }

    struct BlockedWriter {
        operation: BlockedOperation,
        written: Arc<AtomicUsize>,
    }

    impl AsyncWrite for BlockedWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            bytes: &[u8],
        ) -> Poll<io::Result<usize>> {
            let count = if matches!(self.operation, BlockedOperation::PartialWrite) {
                if self.written.load(Ordering::SeqCst) != 0 {
                    return Poll::Pending;
                }
                bytes.len().min(3)
            } else {
                bytes.len()
            };
            self.written.fetch_add(count, Ordering::SeqCst);
            Poll::Ready(Ok(count))
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            if matches!(self.operation, BlockedOperation::Flush) {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            }
        }

        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            if matches!(self.operation, BlockedOperation::Shutdown) {
                Poll::Pending
            } else {
                Poll::Ready(Ok(()))
            }
        }
    }

    async fn assert_write_timeout(
        operation: BlockedOperation,
        response_len: usize,
        expected_written: usize,
    ) {
        let written = Arc::new(AtomicUsize::new(0));
        let transport = BlockedWriter {
            operation,
            written: written.clone(),
        };
        let (user, writer, _) = LynnUser::new(
            Box::new(transport),
            Arc::new(RwLock::new(SystemTime::now())),
            1,
        );
        user.send_response(&BytesMut::from(vec![1; response_len].as_slice()))
            .await;
        if matches!(operation, BlockedOperation::Shutdown) {
            user.shutdown().await;
        } else {
            user.send_response(&BytesMut::from(&b"never sent"[..]))
                .await;
        }
        timeout(Duration::from_secs(2), writer)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(written.load(Ordering::SeqCst), expected_written);
    }

    #[tokio::test]
    async fn partial_write_times_out_without_retrying_or_sending_next_response() {
        assert_write_timeout(BlockedOperation::PartialWrite, 4096, 3).await;
    }

    #[tokio::test]
    async fn blocked_flush_times_out_before_next_response() {
        assert_write_timeout(BlockedOperation::Flush, 4096, 4096).await;
    }

    #[tokio::test]
    async fn small_response_flush_times_out_without_waiting_for_a_batch() {
        assert_write_timeout(BlockedOperation::Flush, 8, 8).await;
    }

    #[tokio::test]
    async fn blocked_shutdown_times_out() {
        assert_write_timeout(BlockedOperation::Shutdown, 4096, 4096).await;
    }

    #[tokio::test]
    async fn zero_write_timeout_preserves_backpressured_response() {
        let (transport, mut peer) = tokio::io::duplex(1);
        let (user, writer, _) = LynnUser::new(
            Box::new(transport),
            Arc::new(RwLock::new(SystemTime::now())),
            0,
        );
        user.send_response(&BytesMut::from(&b"response"[..])).await;
        user.shutdown().await;
        sleep(Duration::from_millis(1300)).await;
        assert!(!writer.is_finished());
        let mut received = Vec::new();
        timeout(Duration::from_secs(1), peer.read_to_end(&mut received))
            .await
            .unwrap()
            .unwrap();
        writer.await.unwrap();
        assert_eq!(received, b"response");
    }
}
