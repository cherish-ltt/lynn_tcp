mod common_api;
mod lynn_client_config;

use std::{net::ToSocketAddrs, time::Duration};

use common_api::{spawn_check_heart, spawn_handle};
use lynn_client_config::{LynnClientConfig, LynnClientConfigBuilder};
use tokio::{net::TcpStream, sync::mpsc, task::JoinHandle, time};
use tracing::{Level, error, info, warn};
use tracing_subscriber::fmt;

use crate::lynn_tcp_dependents::{HandlerResult, InputBufVO};

pub mod client_config {
    pub use super::lynn_client_config::LynnClientConfig;
    pub use super::lynn_client_config::LynnClientConfigBuilder;
}

/// A client for communicating with a server over TCP.
///
/// The `LynnClient` struct represents a client that can connect to a server, send data, and receive data.
/// It uses a configuration object to specify the server's IP address and other settings.
/// The client runs in a separate task and uses channels to communicate with the main task.
/// # Example
/// Use default config (If you want to use custom configuration, please use `LynnClientConfigBuilder`)
/// ```rust
/// use lynn_tcp::{
///     lynn_client::LynnClient,
///     lynn_tcp_dependents::*,
/// };
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Initialize tracing or use app.logserver()
///     tracing_subscriber::fmt::init();
///     let client = LynnClient::new_with_addr("127.0.0.1:9177")
///             .await
///             .start()
///             .await;
///     let _ = client.send_data(HandlerResult::new_with_send_to_server(1, "hello".into())).await;
///     let input_buf_vo = client.get_receive_data().await.unwrap();
///     Ok(())
/// }
/// ```
#[cfg(feature = "client")]
pub struct LynnClient<'a> {
    /// The configuration for the client.
    lynn_client_config: LynnClientConfig<'a>,
    /// The handle for the connection task.
    connection_join_handle: Option<JoinHandle<()>>,
    /// The sender for the write channel.
    tx_write: Option<mpsc::Sender<HandlerResult>>,
    /// The receiver for the read channel.
    rx_read: Option<mpsc::Receiver<InputBufVO>>,
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
        let client = Self {
            lynn_client_config,
            connection_join_handle: None,
            tx_write: None,
            rx_read: None,
        };
        client
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
    #[deprecated(since = "1.1.7", note = "use `new_with_addr` instead")]
    pub async fn new_with_ipv4(server_ipv4: &'a str) -> Self {
        let client = Self {
            lynn_client_config: LynnClientConfigBuilder::new()
                .with_server_addr(server_ipv4)
                .build(),
            connection_join_handle: None,
            tx_write: None,
            rx_read: None,
        };
        client
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
    pub async fn new_with_addr<T>(server_addr: T) -> Self
    where
        T: ToSocketAddrs,
    {
        let client = Self {
            lynn_client_config: LynnClientConfigBuilder::new()
                .with_server_addr(server_addr)
                .build(),
            connection_join_handle: None,
            tx_write: None,
            rx_read: None,
        };
        client
    }

    /// Starts the client and returns the instance.
    ///
    /// # Returns
    ///
    /// The `LynnClient` instance.
    pub async fn start(mut self: Self) -> Self {
        match self.run().await {
            Ok(_) => self,
            Err(e) => {
                error!("{}", e);
                self
            }
        }
    }

    /// Runs the client and connects to the server.
    ///
    /// # Returns
    ///
    /// A `Result` indicating whether the connection was successful.
    async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let retry_count = 3;
        let timeout = Duration::from_secs(3);
        let ip_v4 = self.lynn_client_config.get_server_ipv4().to_string();
        let channel_size = self
            .lynn_client_config
            .get_client_single_channel_size()
            .clone();
        let message_header_mark = self.lynn_client_config.get_message_header_mark().clone();
        let message_tail_mark = self.lynn_client_config.get_message_tail_mark().clone();
        for _ in 0..retry_count {
            match time::timeout(timeout, TcpStream::connect(ip_v4.clone())).await {
                Ok(stream) => {
                    if let Ok(stream) = stream {
                        let (tx_write, rx_read, join_handle) = spawn_handle(
                            stream,
                            channel_size,
                            message_header_mark,
                            message_tail_mark,
                        );
                        self.tx_write = Some(tx_write);
                        self.rx_read = Some(rx_read);
                        self.connection_join_handle = Some(join_handle);
                        self.check_heart().await;
                        info!(
                            "Client - [Main-LynnClient] connection to [server_ipv4:{}] success!!! ",
                            { ip_v4 }
                        );
                        return Ok(());
                    } else if let Err(e) = stream {
                        warn!(
                            "connect to server failed - TcpStream e: {:?}",
                            e.to_string()
                        );
                        continue;
                    }
                }
                Err(e) => {
                    warn!("connect to server failed - timeout e: {:?}", e.to_string());
                    continue;
                }
            }
        }
        Err("connect to server failed".into())
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
            }
            Err(e) => {
                warn!("set_global_default failed - e: {:?}", e.to_string())
            }
        }
    }

    /// Gets the received data from the server.
    ///
    /// # Returns
    ///
    /// An `Option` containing the received data.
    pub async fn get_receive_data(&mut self) -> Option<InputBufVO> {
        self.rx_read.as_mut().unwrap().recv().await
    }

    /// Gets the sender for the write channel.
    ///
    /// # Returns
    ///
    /// An `Option` containing the sender.
    pub async fn get_sender(&mut self) -> Option<mpsc::Sender<HandlerResult>> {
        self.tx_write.clone()
    }

    /// Sends data to the server.
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
            }
            None => Err("tx_write is None , No linked server".into()),
        }
    }

    /// Checks the heart.
    pub(crate) async fn check_heart(&mut self) {
        let interval_time = self
            .lynn_client_config
            .get_server_check_heart_interval()
            .clone();
        if let Some(sender) = self.get_sender().await {
            spawn_check_heart(interval_time, sender);
        } else {
            warn!("Client - [check heart] start failed!!!");
        }
    }
}
