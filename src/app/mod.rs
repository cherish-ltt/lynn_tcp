mod common_api;
mod connection_limiter;
mod lynn_server_config;
mod lynn_server_user;
mod router;
mod tcp_reactor;

use std::{
    net::{SocketAddr, ToSocketAddrs},
    sync::Arc,
};

use common_api::spawn_check_heart;
use connection_limiter::ConnectionLimiter;
use crossbeam_deque::Injector;
use dashmap::DashMap;
use lynn_server_config::LynnServerConfig;
use lynn_server_user::LynnUser;
use tokio::net::TcpListener;
use tracing::{error, info, warn, Level};
use tracing_subscriber::fmt;

#[cfg(feature = "server")]
use crate::app::{router::LynnRouter, tcp_reactor::{TcpReactor, TcpSocketConfig}};
use crate::{
    app::tcp_reactor::event_api::ReactorEvent,
    const_config::{SERVER_MESSAGE_HEADER_MARK, SERVER_MESSAGE_TAIL_MARK},
    handler::{HandlerContext, IHandler, IntoSystem},
    LynnError,
};

pub mod lynn_config_api {
    pub use super::lynn_server_config::LynnServerConfig;
    pub use super::lynn_server_config::LynnServerConfigBuilder;
}

pub(crate) mod event_api {
    pub(crate) use super::tcp_reactor::*;
}

/// Represents a server for the Lynn application.
///
/// The `LynnServer` struct holds information about the server, including its configuration,
/// client list, router map, and thread pool.
///
/// # Example
/// Use default config
/// ```rust
/// use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Initialize tracing or use app.logserver()
///     tracing_subscriber::fmt::init();
///     let _ = LynnServer::new()
///         .await
///         .add_router(1, my_service)
///         .add_router(2, my_service_with_buf)
///         .add_router(3, my_service_with_clients)
///         .start()
///         .await;
///     Ok(())
/// }
///
/// pub async fn my_service() -> HandlerResult {
///     HandlerResult::new_without_send()
/// }
/// pub async fn my_service_with_buf(input_buf_vo: InputBufVO) -> HandlerResult {
///     println!(
///         "service read from :{}",
///         input_buf_vo.get_input_addr().unwrap()
///     );
///     HandlerResult::new_without_send()
/// }
/// pub async fn my_service_with_clients(clients_context: ClientsContext) -> HandlerResult {
///     HandlerResult::new_with_send(1, "hello lynn".into(), clients_context.get_all_clients_addrs().await)
/// }
/// ```
/// # Example
/// Use customized config
/// ```rust
/// use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Initialize tracing or use app.logserver()
///     tracing_subscriber::fmt::init();
///     let _ = LynnServer::new_with_config(
///         LynnServerConfigBuilder::new()
///             .with_addr("0.0.0.0:9177")
///             .with_server_max_connections(Some(&200))
///             // Suggestion 256-512
///             .with_server_max_taskpool_size(&512)
///             // ...more
///             .build(),
///         )
///         .await
///         .add_router(1, my_service)
///         .add_router(2, my_service_with_buf)
///         .add_router(3, my_service_with_clients)
///         .start()
///         .await;
///     Ok(())
/// }
///
/// pub async fn my_service() -> HandlerResult {
///     HandlerResult::new_without_send()
/// }
/// pub async fn my_service_with_buf(input_buf_vo: InputBufVO) -> HandlerResult {
///     println!(
///         "service read from :{}",
///         input_buf_vo.get_input_addr().unwrap()
///     );
///     HandlerResult::new_without_send()
/// }
/// pub async fn my_service_with_clients(clients_context: ClientsContext) -> HandlerResult {
///     HandlerResult::new_with_send(1, "hello lynn".into(), clients_context.get_all_clients_addrs().await)
/// }
/// ```
#[cfg(feature = "server")]
pub struct LynnServer<'a> {
    /// A map of connected clients, where the key is the client's address and the value is a `LynnUser` instance.
    clients: ClientsStruct,
    /// A map of routes, where the key is a method ID and the value is a service handler.
    lynn_router: Arc<LynnRouter>,
    /// The configuration for the server.
    lynn_config: LynnServerConfig<'a>,
    /// reactor
    reactor: TcpReactor,
}

pub(crate) type ClientsStructType = Arc<DashMap<SocketAddr, LynnUser>>;
#[derive(Clone)]
pub(crate) struct ClientsStruct(pub(crate) ClientsStructType);

pub(crate) type AsyncFunc = Box<dyn IHandler>;
type TaskBodyOutChannel = (Arc<AsyncFunc>, HandlerContext, ClientsStructType);
pub(crate) type ReactorEventSender = Arc<Injector<ReactorEvent>>;
// pub(crate) type TaskBody = Arc<Injector<TaskBodyOutChannel>>;
// pub(crate) type TaskBody = mpsc::Sender<(Arc<AsyncFunc>, HandlerContext, ClientsStructType)>;

/// Implementation of methods for the LynnServer struct.
impl<'a> LynnServer<'a> {
    /// Creates a new instance of `LynnServer` with default configuration.
    ///
    /// # Returns
    ///
    /// A new instance of `LynnServer`.
    pub async fn new() -> Self {
        let lynn_config = LynnServerConfig::default();
        let app = Self {
            clients: ClientsStruct(Arc::new(DashMap::new())),
            lynn_router: Arc::new(LynnRouter::new()),
            lynn_config,
            reactor: TcpReactor::new(),
        };
        app
    }

    /// Creates a new instance of `LynnServer` with a specified IPv4 address.
    ///
    /// # Parameters
    ///
    /// * `ipv4` - The IPv4 address to bind the server to.
    ///
    /// # Returns
    ///
    /// A new instance of `LynnServer`.
    #[deprecated(note = "use `new_with_addr`", since = "1.1.7")]
    pub async fn new_with_ipv4(ipv4: &'a str) -> Self {
        let mut app = Self::new().await;
        match ipv4.to_socket_addrs() {
            Ok(mut addrs) => {
                if let Some(addr) = addrs.next() {
                    app.lynn_config.server_addr = addr;
                } else {
                    error!("Invalid IPv4 address: {}", ipv4);
                    panic!("Invalid IPv4 address: {}", ipv4);
                }
            }
            Err(e) => {
                error!("Failed to parse IPv4 address '{}': {}", ipv4, e);
                panic!("Failed to parse IPv4 address '{}': {}", ipv4, e);
            }
        }
        app
    }

    /// Creates a new instance of `LynnServer` with a specified address.
    ///
    /// # Parameters
    ///
    /// * `addr` - The address to bind the server to(IPV4,IPV6).
    ///
    /// # Returns
    ///
    /// A new instance of `LynnServer`.
    pub async fn new_with_addr<T>(addr: T) -> Self
    where
        T: ToSocketAddrs,
    {
        let mut app = Self::new().await;
        match addr.to_socket_addrs() {
            Ok(mut addrs) => {
                if let Some(socket_addr) = addrs.next() {
                    app.lynn_config.server_addr = socket_addr;
                } else {
                    error!("No valid addresses found");
                    panic!("No valid addresses found");
                }
            }
            Err(e) => {
                error!("Failed to parse address: {}", e);
                panic!("Failed to parse address: {}", e);
            }
        }
        app
    }

    /// Creates a new instance of `LynnServer` with a specified configuration.
    ///
    /// # Parameters
    ///
    /// * `lynn_config` - The configuration for the server.
    ///
    /// # Returns
    ///
    /// A new instance of `LynnServer`.
    pub async fn new_with_config(lynn_config: LynnServerConfig<'a>) -> Self {
        let mut app = Self::new().await;
        app.lynn_config = lynn_config;
        app
    }

    /// Adds a route to the server.
    ///
    /// # Parameters
    ///
    /// * `method_id` - The ID of the method to route.
    /// * `handler` - The service handler for the method.
    ///
    /// # Returns
    ///
    /// The modified `LynnServer` instance.
    pub fn add_router<Param>(mut self, method_id: u16, handler: impl IntoSystem<Param>) -> Self {
        self.lynn_router.add_router(method_id, handler);
        self
    }

    /// Checks the heartbeat of connected clients and removes those that have not sent messages for a long time.
    async fn check_heart(&self) {
        let clients = self.clients.0.clone();
        let server_check_heart_interval =
            self.lynn_config.get_server_check_heart_interval().clone();
        let server_check_heart_timeout_time = self
            .lynn_config
            .get_server_check_heart_timeout_time()
            .clone();
        spawn_check_heart(
            server_check_heart_interval,
            server_check_heart_timeout_time,
            clients,
        );
    }

    pub async fn start(mut self: Self) {
        self.init_marks().await;
        let server_arc = Arc::new(self);
        if let Err(e) = server_arc.run().await {
            error!("{}", e);
        }
    }

    /// Starts the server and begins listening for client connections.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the server starts successfully, otherwise returns an error.
    async fn run(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error>> {
        // Binds a TCP listener to the local address.
        let listener = TcpListener::bind(self.lynn_config.get_server_addr()).await?;
        info!(
            "Server - [Main-LynnServer] start success!!! with [server_addr:{}]",
            self.lynn_config.get_server_addr()
        );

        self.check_heart().await;

        // Create connection limiter if rate limiting or per-IP limiting is enabled
        let rate_limit = *self.lynn_config.get_server_connection_rate_limit();
        let max_connections_per_ip = *self.lynn_config.get_server_max_connections_per_ip();
        let connection_limiter = if rate_limit > 0 || max_connections_per_ip > 0 {
            Some(Arc::new(ConnectionLimiter::new(rate_limit, max_connections_per_ip)))
        } else {
            None
        };

        // Create TCP socket configuration
        let tcp_config = TcpSocketConfig {
            nodelay: *self.lynn_config.get_tcp_nodelay(),
            keepalive_enabled: *self.lynn_config.get_tcp_keepalive_enabled(),
            keepalive_time_secs: *self.lynn_config.get_tcp_keepalive_time_secs(),
            recv_buffer_size: *self.lynn_config.get_recv_buffer_size(),
            send_buffer_size: *self.lynn_config.get_send_buffer_size(),
        };

        self.reactor
            .start(
                self.clients.0.clone(),
                self.lynn_config.get_server_single_processs_permit(),
                *self.lynn_config.get_message_header_mark(),
                *self.lynn_config.get_message_tail_mark(),
                self.lynn_router.clone(),
                listener,
                self.lynn_config.get_server_max_connections(),
                self.lynn_config.get_server_max_reactor_taskpool_size(),
                connection_limiter.as_ref().map(|limiter| {
                    (
                        self.lynn_config.get_server_connection_rate_limit(),
                        self.lynn_config.get_server_max_connections_per_ip(),
                        limiter.clone(),
                    )
                }),
                tcp_config,
            )
            .await;
        Ok(())
    }

    async fn init_marks(&self) {
        SERVER_MESSAGE_HEADER_MARK.get_or_init(|| *self.lynn_config.get_message_header_mark());
        SERVER_MESSAGE_TAIL_MARK.get_or_init(|| *self.lynn_config.get_message_tail_mark());
    }

    /// Logs server information.
    /// since v1.1.8 Users need to manually activate it
    #[cfg(feature = "server")]
    pub fn log_server(&self) {
        let subscriber = fmt::Subscriber::builder()
            .with_max_level(Level::INFO)
            .finish();
        match tracing::subscriber::set_global_default(subscriber) {
            Ok(_) => {
                info!("Server - [log server] start sucess!!!")
            }
            Err(e) => {
                warn!("set_global_default failed - e: {:?}", e.to_string())
            }
        }
    }
}
