use std::{sync::Arc, time::SystemTime};

use bytes::Bytes;
use tokio::{
    sync::{RwLock, Semaphore, mpsc},
    task::JoinHandle,
};

/// Represents a user in the Lynn system.
///
/// This struct holds information about a user, including their sender channel, user ID,
/// process permit, last communicate time, and associated thread.
pub(crate) struct LynnUser {
    /// The sender channel used to send data to the client.
    sender: mpsc::Sender<Bytes>,
    /// An optional user ID.
    user_id: Option<u64>,
    /// The process permit for the user.
    process_permit: Arc<Semaphore>,
    /// The last time the user communicated.
    last_communicate_time: Arc<RwLock<SystemTime>>,
    /// The thread associated with the user.
    thread: Option<JoinHandle<()>>,
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
        sender: mpsc::Sender<Bytes>,
        process_permit: Arc<Semaphore>,
        join_handle: JoinHandle<()>,
        last_communicate_time: Arc<RwLock<SystemTime>>,
    ) -> Self {
        Self {
            sender,
            user_id: None,
            process_permit,
            last_communicate_time,
            thread: Some(join_handle),
        }
    }

    /// Gets a clone of the process permit.
    ///
    /// # Returns
    ///
    /// A clone of the process permit.
    pub(crate) fn get_process_permit(&self) -> Arc<Semaphore> {
        self.process_permit.clone()
    }

    /// Gets a clone of the last communicate time.
    ///
    /// # Returns
    ///
    /// A clone of the last communicate time.
    pub(crate) fn get_last_communicate_time(&self) -> Arc<RwLock<SystemTime>> {
        self.last_communicate_time.clone()
    }

    pub(crate) async fn send_response(&self, response: &Bytes) {
        self.sender.send(response.clone()).await;
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
        if let Some(thread) = self.thread.take() {
            let _ = thread.abort();
        }
        self.thread = None;
    }
}
