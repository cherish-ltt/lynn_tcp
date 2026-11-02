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
    ///
    /// # Returns
    ///
    /// The user, its writer task, and the connection-close notification receiver.
    pub(crate) fn new(
        write_half: BoxedWriteHalf,
        last_communicate_time: Arc<RwLock<SystemTime>>,
    ) -> (Self, JoinHandle<()>, watch::Receiver<bool>) {
        let (tx, mut rx) = channel(DEFAULT_SYSTEM_CHANNEL_SIZE);
        let (close, closed) = watch::channel(false);
        let main_join_handle = tokio::spawn(async move {
            let mut write_half = write_half;

            let mut unflushed_bytes = 0;
            while let Some(LynnUserSignal::SendResponse(response)) = rx.recv().await {
                if let Err(e) = write_half.write_all(&response).await {
                    error!("Failed to write to socket: {}", e);
                    return;
                } else {
                    unflushed_bytes += response.len();
                    if unflushed_bytes >= 4096 {
                        if let Err(e) = write_half.flush().await {
                            error!("Failed to flush socket: {}", e);
                            return;
                        }
                        unflushed_bytes = 0;
                    }
                }
            }
            if let Err(e) = write_half.shutdown().await {
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

    #[inline(always)]
    pub(crate) fn send_response(
        &self,
        response: &BytesMut,
    ) -> impl Future<Output = ()> + Send + use<> {
        let sender = self.sender.clone();
        let response = response.clone();
        async move {
            if let Err(e) = sender.send(LynnUserSignal::SendResponse(response)).await {
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
