use std::{net::ToSocketAddrs, sync::Arc};

use tokio::{sync::mpsc, sync::watch, task::JoinHandle};
use tracing::{Level, error, info, warn};
use tracing_subscriber::fmt;

use crate::application::client::client_common::{SharedWriteReceiver, spawn_check_heart};
use crate::application::client::client_config::{LynnClientConfig, LynnClientConfigBuilder};
use crate::application::client::client_connection::{ConnectionParams, connection_supervisor};
use crate::domain::model::handler_result::HandlerResult;
use crate::domain::model::input_buf_vo::InputBufVO;

/// A client for communicating with a server over TCP.
///
/// The `LynnClient` struct represents a client that can connect to a server,
/// send data, and receive data. It uses a configuration object to specify the
/// server's IP address and other settings.
///
/// The connection is supervised: whenever it drops, the client automatically
/// attempts to reconnect (default: 3 attempts, 1s apart — configurable via
/// `LynnClientConfigBuilder::with_reconnect_max_attempts` /
/// `with_reconnect_interval_secs`). The user-facing channels survive
/// reconnections; [`LynnClient::is_connected`] reports the live state.
/// # Example
/// Use default config (If you want to use custom configuration, please use `LynnClientConfigBuilder`)
/// ```rust,no_run
/// use lynn_tcp::{
///     lynn_client::LynnClient,
///     lynn_tcp_dependents::*,
/// };
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Initialize tracing or use app.logserver()
///     tracing_subscriber::fmt::init();
///     let mut client = LynnClient::new_with_addr("127.0.0.1:9177")
///             .await
///             .start()
///             .await;
///     if !client.is_connected() {
///         eprintln!("server unreachable");
///     }
///     let _ = client.send_data(HandlerResult::new_with_send_to_server(1, "hello".into())).await;
///     let input_buf_vo = client.get_receive_data().await.unwrap();
///     Ok(())
/// }
/// ```
#[cfg(feature = "client")]
pub struct LynnClient<'a> {
    /// The configuration for the client.
    lynn_client_config: LynnClientConfig<'a>,
    /// The handle for the connection supervisor task (reconnects on drop).
    supervisor_join_handle: Option<JoinHandle<()>>,
    /// The sender for the write channel, valid across reconnections.
    tx_write: Option<mpsc::Sender<HandlerResult>>,
    /// The receiver for the read channel, valid across reconnections.
    rx_read: Option<mpsc::Receiver<InputBufVO>>,
    /// Live connection state, driven by the connection supervisor.
    connection_state: Option<watch::Receiver<bool>>,
}

impl<'a> LynnClient<'a> {
    /// Creates a new `LynnClient` instance with the given configuration.
    ///
    /// # Parameters
    ///
    /// - `lynn_client_config`: The configuration for the client.
    ///
    /// # Returns
    ///
    /// A new `LynnClient` instance.
    pub async fn new_with_config(lynn_client_config: LynnClientConfig<'a>) -> Self {
        Self {
            lynn_client_config,
            supervisor_join_handle: None,
            tx_write: None,
            rx_read: None,
            connection_state: None,
        }
    }

    /// Creates a new `LynnClient` instance with the given IPv4 address.
    ///
    /// # Parameters
    ///
    /// - `server_ipv4`: The IPv4 address of the server.
    ///
    /// # Returns
    ///
    /// A new `LynnClient` instance.
    ///
    /// # Panics
    ///
    /// Panics if the address is invalid.
    #[deprecated(since = "1.1.7", note = "use `new_with_addr` instead")]
    pub async fn new_with_ipv4(server_ipv4: &'a str) -> Self {
        let config = LynnClientConfigBuilder::new()
            .with_server_addr(server_ipv4)
            .expect("Invalid server address")
            .build();
        Self {
            lynn_client_config: config,
            supervisor_join_handle: None,
            tx_write: None,
            rx_read: None,
            connection_state: None,
        }
    }

    /// Creates a new `LynnClient` instance with the given address.
    ///
    /// # Parameters
    ///
    /// - `server_addr`: The address of the server (IPV4,IPV6).
    ///
    /// # Returns
    ///
    /// A new `LynnClient` instance.
    ///
    /// # Panics
    ///
    /// Panics if the address is invalid.
    pub async fn new_with_addr<T>(server_addr: T) -> Self
    where
        T: ToSocketAddrs,
    {
        let config = LynnClientConfigBuilder::new()
            .with_server_addr(server_addr)
            .expect("Invalid server address")
            .build();
        Self {
            lynn_client_config: config,
            supervisor_join_handle: None,
            tx_write: None,
            rx_read: None,
            connection_state: None,
        }
    }

    /// Starts the client and returns the instance.
    ///
    /// Waits for the initial connection session to finish: on success the
    /// client is usable; on failure the error is logged and the client stays
    /// disconnected.
    ///
    /// # Returns
    ///
    /// The `LynnClient` instance.
    pub async fn start(mut self) -> Self {
        match self.run().await {
            Ok(_) => self,
            Err(e) => {
                error!("{}", e);
                self
            },
        }
    }

    /// Returns whether the client currently holds a live connection.
    pub fn is_connected(&self) -> bool {
        self.connection_state
            .as_ref()
            .map(|state| *state.borrow())
            .unwrap_or(false)
    }

    /// Runs the client: creates the user-facing channels once, then spawns
    /// the connection supervisor which owns connect/reconnect attempts.
    ///
    /// # Returns
    ///
    /// A `Result` indicating whether the initial connection was successful.
    async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let channel_size = *self.lynn_client_config.get_client_single_channel_size();
        let (tx_read, rx_read) = mpsc::channel::<InputBufVO>(channel_size);
        let (tx_write, rx_write) = mpsc::channel::<HandlerResult>(channel_size);
        let (state_tx, state_rx) = watch::channel(false);
        let (init_tx, init_rx) = tokio::sync::oneshot::channel();

        // A bad configuration (e.g. TLS cert files) fails fast, before any
        // connect attempt.
        let params = ConnectionParams::from_config(&self.lynn_client_config)?;
        let shared_rx_write: SharedWriteReceiver = Arc::new(tokio::sync::Mutex::new(rx_write));

        self.supervisor_join_handle = Some(tokio::spawn(connection_supervisor(
            params,
            tx_read,
            shared_rx_write,
            state_tx,
            Some(init_tx),
        )));

        match init_rx.await {
            Ok(Ok(())) => {
                self.tx_write = Some(tx_write);
                self.rx_read = Some(rx_read);
                self.connection_state = Some(state_rx);
                self.check_heart().await;
                Ok(())
            },
            Ok(Err(message)) => Err(message.into()),
            Err(_) => Err("connection supervisor exited before reporting".into()),
        }
    }

    /// Logs the server information.
    /// since v1.1.8 Users need to manually activate it
    #[cfg(feature = "client")]
    pub fn log_server(&self) {
        let subscriber = fmt::Subscriber::builder()
            .with_max_level(Level::INFO)
            .finish();
        match tracing::subscriber::set_global_default(subscriber) {
            Ok(_) => {
                info!("Client - [log server] start sucess!!!")
            },
            Err(e) => {
                warn!("set_global_default failed - e: {:?}", e.to_string())
            },
        }
    }

    /// Gets the received data from the server.
    ///
    /// Returns `None` while the client is disconnected and reconnecting
    /// (the read half is idle until the connection is re-established), or
    /// permanently after the client was dropped or gave up reconnecting.
    ///
    /// # Returns
    ///
    /// An `Option` containing the received data, or `None` if the client is not connected.
    pub async fn get_receive_data(&mut self) -> Option<InputBufVO> {
        match self.rx_read.as_mut() {
            Some(rx) => rx.recv().await,
            None => {
                error!("Client is not connected. Call start() first.");
                None
            },
        }
    }

    /// Gets the sender for the write channel.
    ///
    /// The sender stays valid across automatic reconnections.
    ///
    /// # Returns
    ///
    /// An `Option` containing the sender.
    pub async fn get_sender(&mut self) -> Option<mpsc::Sender<HandlerResult>> {
        self.tx_write.clone()
    }

    /// Sends data to the server.
    ///
    /// While the connection is down and reconnecting, messages are queued in
    /// the (bounded) write channel and flushed on reconnect; stale frames are
    /// dropped by the supervisor when the disconnect is detected.
    pub async fn send_data(
        &mut self,
        handler_result: HandlerResult,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match &self.tx_write {
            Some(sender) => {
                if let Err(e) = sender.send(handler_result).await {
                    error!("send to server failed - e: {:?}", e);
                    Err(e.into())
                } else {
                    Ok(())
                }
            },
            None => Err("tx_write is None , No linked server".into()),
        }
    }

    /// Checks the heart.
    pub(crate) async fn check_heart(&mut self) {
        let interval_time = *self.lynn_client_config.get_server_check_heart_interval();
        if let Some(sender) = self.get_sender().await {
            spawn_check_heart(interval_time, sender);
        } else {
            warn!("Client - [check heart] start failed!!!");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_with_addr_resolves_the_address() {
        let client = LynnClient::new_with_addr("127.0.0.1:9197").await;
        assert_eq!(
            client.lynn_client_config.get_server_ipv4(),
            "127.0.0.1:9197"
        );
        assert!(!client.is_connected(), "unstarted client is not connected");
    }

    #[tokio::test]
    #[allow(deprecated)]
    async fn new_with_ipv4_still_resolves() {
        let client = LynnClient::new_with_ipv4("127.0.0.1:9196").await;
        assert_eq!(
            client.lynn_client_config.get_server_ipv4(),
            "127.0.0.1:9196"
        );
    }

    #[test]
    fn log_client_does_not_panic_on_repeated_init() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let client = rt.block_on(LynnClient::new_with_addr("127.0.0.1:9195"));
        client.log_server(); // first call may succeed...
        client.log_server(); // ...second call must hit the warn branch, not panic
    }
}
