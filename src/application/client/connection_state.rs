/// Latest connection snapshot within one client supervisor lifecycle.
///
/// Subscriptions can coalesce intermediate states. Channel closure ends the
/// lifecycle even if the last cached snapshot is [`Self::Connected`]. A new
/// `start()` creates a separate lifecycle whose generations start over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionState {
    /// Transport initialization completed; business authentication is separate.
    Connected {
        /// First connection is 1; each successful reconnect increments it.
        generation: u64,
    },
    /// Waiting to reconnect or attempting to establish a connection.
    Reconnecting {
        /// Last connected generation, or 0 before the first success.
        generation: u64,
    },
    /// The supervisor ended normally, including exhausted connection attempts.
    Stopped {
        /// Last connected generation, or 0 if no connection succeeded.
        generation: u64,
    },
}
