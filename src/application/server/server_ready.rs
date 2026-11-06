use std::net::SocketAddr;

use tokio::sync::watch;

pub(crate) type StartupResult = Option<Result<SocketAddr, ServerStartupError>>;

/// The startup outcome; detailed initialization errors are returned by `LynnServer::start`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ServerStartupError {
    #[error("server initialization failed; inspect the result of start()")]
    Failed,
    #[error("server was cancelled before initialization completed")]
    Cancelled,
}

/// Observes one initialization result without keeping the server alive.
/// Success is historical readiness, not a live health check or a client handshake.
#[derive(Clone, Debug)]
pub struct ServerReady(pub(crate) watch::Receiver<StartupResult>);

impl ServerReady {
    /// Waits for initialization and returns the actual bound TCP or UDP address.
    /// The result remains available to late observers after the server stops.
    pub async fn wait(mut self) -> Result<SocketAddr, ServerStartupError> {
        loop {
            if let Some(result) = *self.0.borrow_and_update() {
                return result;
            }
            if self.0.changed().await.is_err() {
                return Err(ServerStartupError::Cancelled);
            }
        }
    }
}
