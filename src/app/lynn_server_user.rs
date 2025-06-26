use std::{sync::Arc, time::SystemTime};

use bytes::BytesMut;
use tokio::{
    io::{AsyncWriteExt, WriteHalf},
    net::TcpStream,
    sync::{Mutex, RwLock},
};
use tracing::error;

/// Represents a user in the Lynn system.
///
/// This struct holds information about a user, including their sender channel, user ID,
/// process permit, last communicate time, and associated thread.
pub(crate) struct LynnUser {
    /// The write_half used to send data to the client.
    write_half: *mut WriteHalf<TcpStream>,
    /// The last time the user communicated.
    last_communicate_time: Arc<RwLock<SystemTime>>,
    mutex: Mutex<()>,
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
        Self {
            write_half: Box::into_raw(Box::new(write_half)),
            last_communicate_time,
            mutex: Mutex::new(()),
        }
    }

    /// Gets a clone of the last communicate time.
    ///
    /// # Returns
    ///
    /// A clone of the last communicate time.
    pub(crate) fn get_last_communicate_time(&self) -> Arc<RwLock<SystemTime>> {
        self.last_communicate_time.clone()
    }

    pub(crate) async fn send_response(&self, response: &BytesMut) {
        let _lock = self.mutex.lock().await;
        if !self.write_half.is_null() {
            if let Some(write_half) = unsafe { self.write_half.as_mut() } {
                if let Err(e) = write_half.write_all(&response).await {
                    error!("Failed to write to socket: {}", e);
                } else {
                    let _ = write_half.flush().await;
                }
            }
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
    fn drop(&mut self) {}
}
