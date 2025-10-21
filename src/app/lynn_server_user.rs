use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};

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

            let mut buffer = Vec::with_capacity(4096);
            loop {
                match rx.recv().await {
                    Some(LynnUserSignal::SendResponse(response)) => {
                        if let Err(e) = write_half.write_all(&response).await {
                            error!("Failed to write to socket: {}", e);
                            break;
                        } else {
                            buffer.extend_from_slice(&response);
                            if buffer.len() >= 4096 {
                                if let Err(e) = write_half.flush().await {
                                    error!("Failed to flush socket: {}", e);
                                    break;
                                }
                                buffer.clear();
                            }
                        }
                    }
                    None => break,
                }
            }
            if !buffer.is_empty() {
                let _ = write_half.flush().await;
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
            error!("Send response error:{}", e);
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
