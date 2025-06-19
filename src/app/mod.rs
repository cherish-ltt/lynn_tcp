mod common_api;
mod lynn_server_config;
mod lynn_server_user;
mod tcp_reactor;

use std::{
    collections::HashMap,
    net::{SocketAddr, ToSocketAddrs},
    ops::DerefMut,
    sync::Arc,
};

use common_api::spawn_check_heart;
use crossbeam_deque::Injector;
use lynn_server_config::LynnServerConfig;
use lynn_server_user::LynnUser;
use tokio::{net::TcpListener, sync::RwLock};
use tracing::{Level, error, info, warn};
use tracing_subscriber::fmt;

#[cfg(feature = "server")]
use crate::app::tcp_reactor::TcpReactor;
use crate::{
    app::tcp_reactor::event_api::ReactorEvent,
    const_config::{SERVER_MESSAGE_HEADER_MARK, SERVER_MESSAGE_TAIL_MARK},
    handler::{HandlerContext, IHandler, IntoSystem},
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
///             // Suggestion 300-500
///             .with_server_max_taskpool_size(&300)
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
    router_map_async: RouterMapAsyncStruct,
    router_maps: RouterMapsStruct,
    /// The configuration for the server.
    lynn_config: LynnServerConfig<'a>,
    /// reactor
    reactor: TcpReactor,
}

pub(crate) type ClientsStructType = Arc<RwLock<HashMap<SocketAddr, LynnUser>>>;
#[derive(Clone)]
pub(crate) struct ClientsStruct(pub(crate) ClientsStructType);
struct RouterMapAsyncStruct(Arc<Option<HashMap<u16, Arc<AsyncFunc>>>>);
struct RouterMapsStruct(Option<HashMap<u16, Arc<AsyncFunc>>>);

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
            clients: ClientsStruct(Arc::new(RwLock::new(HashMap::new()))),
            router_map_async: RouterMapAsyncStruct(Arc::new(None)),
            router_maps: RouterMapsStruct(None),
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
        app.lynn_config.server_addr = ipv4.to_socket_addrs().unwrap().next().unwrap();
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
        app.lynn_config.server_addr = addr.to_socket_addrs().unwrap().next().unwrap();
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
        if let Some(ref mut map) = self.router_maps.0 {
            map.insert(method_id, Arc::new(Box::new(handler.to_system())));
        } else {
            let mut map: HashMap<u16, Arc<Box<dyn IHandler>>> = HashMap::new();
            map.insert(method_id, Arc::new(Box::new(handler.to_system())));
            self.router_maps.0 = Some(map);
        }
        self
    }

    async fn synchronous_router(&mut self) {
        self.router_map_async.0 = Arc::new(self.router_maps.0.clone());
        self.router_maps.0 = None;
    }

    /// Removes a client from the server.
    ///
    /// # Parameters
    ///
    /// * `addr` - The address of the client to remove.
    async fn remove_client(&mut self, addr: SocketAddr) {
        let mut clients = self.clients.0.write().await;
        let guard = clients.deref_mut();
        if guard.contains_key(&addr) {
            guard.remove(&addr);
        }
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
        self.synchronous_router().await;
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

        self.reactor
            .start(
                self.clients.0.clone(),
                self.lynn_config.get_server_single_processs_permit(),
                *self.lynn_config.get_message_header_mark(),
                *self.lynn_config.get_message_tail_mark(),
                self.router_map_async.0.clone(),
                listener,
                self.lynn_config.get_server_max_connections(),
                self.lynn_config.get_server_max_reactor_taskpool_size(),
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
