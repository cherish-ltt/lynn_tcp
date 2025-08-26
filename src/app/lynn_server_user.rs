use std::{sync::Arc, time::SystemTime};

use bytes::BytesMut;
use tokio::{
    io::{AsyncWriteExt, WriteHalf},
    net::TcpStream,
    sync::{
        RwLock,
        mpsc::{Sender, channel},
    },
    task::JoinHandle,
};
use tracing::error;

use crate::const_config::DEFAULT_SYSTEM_CHANNEL_SIZE;

pub(crate) enum LynnUserSignal {
    SendResponse(BytesMut),
}

/// Represents a user in the Lynn system.
///
/// This struct holds information about a user, including their sender channel, user ID,
/// process permit, last communicate time, and associated thread.
pub(crate) struct LynnUser {
    /// The last time the user communicated.
    last_communicate_time: Arc<RwLock<SystemTime>>,
    sender: Sender<LynnUserSignal>,

    main_join_handle: JoinHandle<()>,
}

unsafe impl Send for LynnUser {}
unsafe impl Sync for LynnUser {}

/// Implementation of methods for the LynnUser struct.
impl LynnUser {
    /// Creates a new instance of LynnUser with the specified sender channel.
    ///
    /// # Parameters
    ///
    /// * `sender` - The sender channel for sending data to the client.
    /// * `process_permit` - The process permit for the user.
    /// * `join_handle` - The join handle for the user's thread.
    /// * `last_communicate_time` - The last time the user communicated.
    ///
    /// # Returns
    ///
    /// A new instance of LynnUser.
    pub(crate) fn new(
        write_half: WriteHalf<TcpStream>,
        last_communicate_time: Arc<RwLock<SystemTime>>,
    ) -> Self {
        let (tx, mut rx) = channel(DEFAULT_SYSTEM_CHANNEL_SIZE);
        let main_join_handle = tokio::spawn(async move {
            let mut write_half = write_half;
            loop {
                if let Some(signal) = rx.recv().await {
                    match signal {
                        LynnUserSignal::SendResponse(response) => {
                            if let Err(e) = write_half.write_all(&response).await {
                                error!("Failed to write to socket: {}", e);
                            } else {
                                let _ = write_half.flush().await;
                            }
                        }
                    }
                }
            }
        });
        Self {
            last_communicate_time,
            sender: tx,
            main_join_handle,
        }
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
    pub(crate) async fn send_response(&self, response: &BytesMut) {
        if let Err(e) = self
            .sender
            .send(LynnUserSignal::SendResponse(response.clone()))
            .await
        {
            error!("lynn_user-send_response err: {}", e.to_string());
        }
    }
}

/// Implementation of the Drop trait for the LynnUser struct.
impl Drop for LynnUser {
    /// Drops the LynnUser instance and aborts the associated thread if it exists.
    ///
    /// # Parameters
    ///
    /// * `self` - The mutable reference to the LynnUser instance.
    fn drop(&mut self) {
        self.main_join_handle.abort();
    }
}
