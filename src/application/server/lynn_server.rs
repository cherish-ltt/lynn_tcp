use std::{
    net::{SocketAddr, ToSocketAddrs},
    sync::Arc,
};

use crossbeam_deque::Injector;
use dashmap::DashMap;
use tokio::net::TcpListener;
use tracing::{Level, error, info, warn};
use tracing_subscriber::fmt;

use crate::application::server::server_common::spawn_check_heart;
use crate::application::server::server_config::LynnServerConfig;
use crate::const_config::{SERVER_MESSAGE_HEADER_MARK, SERVER_MESSAGE_TAIL_MARK};
use crate::domain::handler::handler_system::{AsyncFunc, HandlerContext, IHandler, IntoSystem};
use crate::domain::model::lynn_user::{ClientsStruct, ClientsStructType, LynnUser};
use crate::domain::routing::router::LynnRouter;
use crate::infrastructure::connection::connection_limiter::ConnectionLimiter;
use crate::infrastructure::tcp::reactor::{ReactorEvent, TcpReactor};
use crate::infrastructure::tcp::stream::StreamAcceptor;
use crate::infrastructure::tcp::tcp_socket_config::TcpSocketConfig;

/// Task body sent through the channel: (handler, context, clients).
pub(crate) type TaskBodyOutChannel = (Arc<AsyncFunc>, HandlerContext, ClientsStructType);

/// Event sender for the reactor, backed by a crossbeam injector.
pub(crate) type ReactorEventSender = Arc<Injector<ReactorEvent>>;

/// Represents a server for the Lynn application.
///
/// The `LynnServer` struct holds information about the server, including its configuration,
/// client list, router map, and thread pool.
///
/// # Example
/// Use default config
/// ```rust,no_run
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
/// ```rust,no_run
/// use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // Initialize tracing or use app.logserver()
///     tracing_subscriber::fmt::init();
///     let _ = LynnServer::new_with_config(
///         LynnServerConfigBuilder::new()
///             .with_addr("0.0.0.0:9177").unwrap()
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

/// Implementation of methods for the LynnServer struct.
impl<'a> LynnServer<'a> {
    /// Creates a new instance of `LynnServer` with default configuration.
    ///
    /// # Returns
    ///
    /// A new instance of `LynnServer`.
    pub async fn new() -> Self {
        let lynn_config = LynnServerConfig::default();
        Self {
            clients: ClientsStruct(Arc::new(DashMap::new())),
            lynn_router: Arc::new(LynnRouter::new()),
            lynn_config,
            reactor: TcpReactor::new(),
        }
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
            },
            Err(e) => {
                error!("Failed to parse IPv4 address '{}': {}", ipv4, e);
                panic!("Failed to parse IPv4 address '{}': {}", ipv4, e);
            },
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
            },
            Err(e) => {
                error!("Failed to parse address: {}", e);
                panic!("Failed to parse address: {}", e);
            },
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
        let server_check_heart_interval = *self.lynn_config.get_server_check_heart_interval();
        let server_check_heart_timeout_time =
            *self.lynn_config.get_server_check_heart_timeout_time();
        spawn_check_heart(
            server_check_heart_interval,
            server_check_heart_timeout_time,
            clients,
        );
    }

    pub async fn start(mut self) {
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
            Some(Arc::new(ConnectionLimiter::new(
                rate_limit,
                max_connections_per_ip,
            )))
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
                Arc::new(StreamAcceptor::Plain),
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
            },
            Err(e) => {
                warn!("set_global_default failed - e: {:?}", e.to_string())
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_with_addr_resolves_the_address() {
        let app = LynnServer::new_with_addr("127.0.0.1:9199").await;
        assert_eq!(app.lynn_config.get_server_addr(), "127.0.0.1:9199");
    }

    #[tokio::test]
    async fn default_server_uses_the_default_address() {
        let app = LynnServer::new().await;
        assert_eq!(app.lynn_config.get_server_addr(), "0.0.0.0:9177");
    }

    #[tokio::test]
    #[allow(deprecated)]
    async fn new_with_ipv4_still_resolves() {
        let app = LynnServer::new_with_ipv4("127.0.0.1:9198").await;
        assert_eq!(app.lynn_config.get_server_addr(), "127.0.0.1:9198");
    }

    #[test]
    fn log_server_does_not_panic_on_repeated_init() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let app = rt.block_on(LynnServer::new());
        app.log_server(); // first call may succeed...
        app.log_server(); // ...second call must hit the warn branch, not panic
    }
}
