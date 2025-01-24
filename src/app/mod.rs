mod lynn_server_config;
mod lynn_server_user;
mod server_thread_pool;

use std::{
    collections::HashMap,
    future::Future,
    net::SocketAddr,
    ops::{Deref, DerefMut},
    pin::Pin,
    sync::Arc,
    time::{Duration, SystemTime},
};

use lynn_server_config::{LynnServerConfig, LynnServerConfigBuilder};
use lynn_server_user::LynnUser;
use server_thread_pool::LynnServerThreadPool;
use tokio::{
    io::{split, AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, Mutex, RwLock, Semaphore},
    task::JoinHandle,
    time::interval,
};
use tracing::{debug, error, info, warn, Level};
use tracing_subscriber::fmt;

use crate::{
    const_config::{DEFAULT_MAX_RECEIVE_BYTES_SIZE, DEFAULT_SYSTEM_CHANNEL_SIZE},
    dto_factory::input_dto_build,
    lynn_tcp_dependents::HandlerResult,
    service::IService,
    vo_factory::{big_buf::BigBufReader, input_vo::InputBufVO, InputBufVOTrait},
};

pub(crate) mod lynn_user_api {
    pub(crate) use super::lynn_server_user::LynnUser;
}

pub mod lynn_config_api {
    pub use super::lynn_server_config::LynnServerConfig;
    pub use super::lynn_server_config::LynnServerConfigBuilder;
}

pub(crate) mod lynn_thread_pool_api {
    pub(crate) use super::server_thread_pool::LynnServerThreadPool;
}

/// Represents a server for the Lynn application.
///
/// The `LynnServer` struct holds information about the server, including its configuration,
/// client list, router map, and thread pool.
///
/// # Example
/// Use default config
/// ```rust
/// use lynn_tcp::{async_func_wrapper, server::*};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let _ = LynnServer::new().await.add_router(1, async_func_wrapper!(my_service)).start().await;
///     Ok(())
/// }
/// pub async fn my_service(input_buf_vo: InputBufVO) -> HandlerResult {
///     println!("service read from :{}", input_buf_vo.get_input_addr());
///     HandlerResult::new_without_send()
/// }
/// ```
/// # Example
/// Use customized config
/// ```rust
/// use lynn_tcp::{async_func_wrapper, server::*};
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let _ = LynnServer::new_with_config(
///     LynnServerConfigBuilder::new()
///         .with_server_ipv4("0.0.0.0:9177")
///         .with_server_max_connections(Some(&200))
///         .with_server_max_threadpool_size(&10)
///         .build(),
/// )
/// .await
/// .add_router(1, async_func_wrapper!(my_service))
/// .start()
/// .await;
/// Ok(())
/// }
/// pub fn async my_service(input_buf_vo: InputBufVO) -> HandlerResult {
///     println!("service read from :{}", input_buf_vo.get_input_addr());
///     HandlerResult::new_without_send()
/// }
/// ```
#[cfg(feature = "server")]
pub struct LynnServer<'a> {
    /// A map of connected clients, where the key is the client's address and the value is a `LynnUser` instance.
    clients: ClientsStruct,
    client_channel_maps: ClientChannelMapsStruct,
    /// A map of routes, where the key is a method ID and the value is a service handler.
    router_map_async: RouterMapAsyncStruct,
    router_maps: RouterMapsStruct,
    /// The configuration for the server.
    lynn_config: LynnServerConfig<'a>,
    /// The thread pool for the server.
    lynn_thread_pool: LynnServerThreadPool,
}

type ClientsStructType = Arc<RwLock<HashMap<SocketAddr, LynnUser>>>;
struct ClientsStruct(ClientsStructType);
struct ClientChannelMapsStruct(Arc<Option<HashMap<SocketAddr, mpsc::Sender<Vec<u8>>>>>);
struct RouterMapAsyncStruct(Arc<Option<HashMap<u16, Arc<AsyncFunc>>>>);
struct RouterMapsStruct(Option<HashMap<u16, Arc<AsyncFunc>>>);

pub(crate) type AsyncFunc = Box<
    dyn Fn(InputBufVO) -> Pin<Box<(dyn Future<Output = HandlerResult> + Send + 'static)>>
        + Send
        + Sync,
>;
#[deprecated(since = "v1.0.0", note = "use AsyncFunc instead")]
pub(crate) type SyncFunc = Arc<Box<dyn IService>>;
pub(crate) type TaskBody = mpsc::Sender<(Arc<AsyncFunc>, InputBufVO, ClientsStructType)>;

#[macro_export]
macro_rules! async_func_wrapper {
    ($async_func:ident) => {{
        type AsyncFunc = Box<
            dyn Fn(InputBufVO) -> Pin<Box<(dyn Future<Output = HandlerResult> + Send + 'static)>>
                + Send
                + Sync,
        >;
        let wrapper = move |input_buf_vo: InputBufVO| {
            let fut: Pin<Box<dyn Future<Output = HandlerResult> + Send + 'static>> =
                Box::pin($async_func(input_buf_vo));
            fut
        };
        Box::new(wrapper) as AsyncFunc
    }};
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
        let server_max_threadpool_size = lynn_config.get_server_max_threadpool_size();
        let thread_pool = LynnServerThreadPool::new(server_max_threadpool_size).await;
        let app = Self {
            clients: ClientsStruct(Arc::new(RwLock::new(HashMap::new()))),
            client_channel_maps: ClientChannelMapsStruct(Arc::new(None)),
            router_map_async: RouterMapAsyncStruct(Arc::new(None)),
            router_maps: RouterMapsStruct(None),
            lynn_config,
            lynn_thread_pool: thread_pool,
        };
        app.log_server().await;
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
    pub async fn new_with_ipv4(ipv4: &'a str) -> Self {
        let lynn_config = LynnServerConfigBuilder::new()
            .with_server_ipv4(ipv4)
            .build();
        let server_max_threadpool_size = lynn_config.get_server_max_threadpool_size();
        let thread_pool = LynnServerThreadPool::new(server_max_threadpool_size).await;
        let app = Self {
            clients: ClientsStruct(Arc::new(RwLock::new(HashMap::new()))),
            client_channel_maps: ClientChannelMapsStruct(Arc::new(None)),
            router_map_async: RouterMapAsyncStruct(Arc::new(None)),
            router_maps: RouterMapsStruct(None),
            lynn_config,
            lynn_thread_pool: thread_pool,
        };
        app.log_server().await;
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
        let server_max_threadpool_size = lynn_config.get_server_max_threadpool_size();
        let thread_pool = LynnServerThreadPool::new(server_max_threadpool_size).await;
        let app = Self {
            clients: ClientsStruct(Arc::new(RwLock::new(HashMap::new()))),
            client_channel_maps: ClientChannelMapsStruct(Arc::new(None)),
            router_map_async: RouterMapAsyncStruct(Arc::new(None)),
            router_maps: RouterMapsStruct(None),
            lynn_config,
            lynn_thread_pool: thread_pool,
        };
        app.log_server().await;
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
    pub fn add_router(mut self, method_id: u16, handler: AsyncFunc) -> Self {
        if let Some(ref mut map) = self.router_maps.0 {
            map.insert(method_id, Arc::new(handler));
        } else {
            let mut map = HashMap::new();
            map.insert(method_id, Arc::new(handler));
            self.router_maps.0 = Some(map);
        }
        self
    }

    pub(crate) async fn synchronous_router(&mut self) {
        self.router_map_async.0 = Arc::new(self.router_maps.0.clone());
        self.router_maps.0 = None;
    }

    /// Adds a new client to the server.
    ///
    /// # Parameters
    ///
    /// * `sender` - The sender channel for the client.
    /// * `addr` - The address of the client.
    /// * `process_permit` - The process permit for the client.
    /// * `join_handle` - The join handle for the client's task.
    /// * `last_communicate_time` - The last time the client communicated.
    pub(crate) async fn add_client(
        &self,
        sender: mpsc::Sender<HandlerResult>,
        addr: SocketAddr,
        process_permit: Arc<Semaphore>,
        join_handle: JoinHandle<()>,
        last_communicate_time: Arc<Mutex<SystemTime>>,
    ) {
        let mut clients = self.clients.0.write().await;
        let guard = clients.deref_mut();
        let lynn_user = LynnUser::new(sender, process_permit, join_handle, last_communicate_time);
        guard.insert(addr, lynn_user);
    }

    /// Removes a client from the server.
    ///
    /// # Parameters
    ///
    /// * `addr` - The address of the client to remove.
    pub(crate) async fn remove_client(&mut self, addr: SocketAddr) {
        let mut clients = self.clients.0.write().await;
        let guard = clients.deref_mut();
        if guard.contains_key(&addr) {
            guard.remove(&addr);
        }
    }

    /// Checks the heartbeat of connected clients and removes those that have not sent messages for a long time.
    pub(crate) async fn check_heart(&self) {
        let clients = self.clients.0.clone();
        let server_check_heart_interval =
            self.lynn_config.get_server_check_heart_interval().clone();
        let server_check_heart_timeout_time = self
            .lynn_config
            .get_server_check_heart_timeout_time()
            .clone();
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(server_check_heart_interval));
            info!(
                "Server - [check heart] start sucess!!! with [server_check_heart_interval:{}s] [server_check_heart_timeout_time:{}s]",
                server_check_heart_interval,
                server_check_heart_timeout_time
            );
            loop {
                interval.tick().await;
                let mut remove_list = vec![];
                {
                    let mut clients_mutex = clients.read().await;
                    let guard = clients_mutex.deref();
                    for (addr, lynn_user) in guard.iter() {
                        let last_communicate_time = lynn_user.last_communicate_time.lock().await;
                        let time_old = last_communicate_time.deref().clone();
                        let time_now = SystemTime::now();
                        match time_old.partial_cmp(&time_now) {
                            Some(std::cmp::Ordering::Less) => {
                                match time_now.duration_since(time_old) {
                                    Ok(duration) => {
                                        if duration.as_secs() > server_check_heart_timeout_time {
                                            remove_list.push(addr.clone());
                                        }
                                    }
                                    Err(e) => {
                                        warn!("unable to compare time,{}", e)
                                    }
                                }
                            }
                            Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater)
                            | None => {}
                        }
                    }
                }
                let mut clients_mutex = clients.write().await;
                let guard = clients_mutex.deref_mut();
                for i in remove_list {
                    if guard.contains_key(&i) {
                        guard.remove(&i);
                        info!(
                            "Clean up addr:{}, that have not sent messages for a long time",
                            i
                        )
                    }
                }
                info!("Server check online socket count:{}", guard.len());
            }
        });
    }

    pub async fn start(mut self: Self) {
        self.synchronous_router().await;
        let server_arc = Arc::new(self);
        server_arc.run().await;
    }

    /// Starts the server and begins listening for client connections.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the server starts successfully, otherwise returns an error.
    async fn run(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error>> {
        // Binds a TCP listener to the local address.
        let listener = TcpListener::bind(self.lynn_config.get_server_ipv4()).await?;
        info!(
            "Server - [Main-LynnServer] start success!!! with [server_ipv4:{}]",
            self.lynn_config.get_server_ipv4()
        );

        self.check_heart().await;

        loop {
            // Waits for a client to connect.
            let clinet_result = listener.accept().await;
            if let Ok((mut socket, addr)) = clinet_result {
                let mut socket_permit = true;
                {
                    if let Some(max_connections) = self.lynn_config.get_server_max_connections() {
                        let clients = self.clients.0.write().await;
                        let guard = clients.deref();
                        if guard.len() < *max_connections {
                            socket_permit = true;
                        } else {
                            socket_permit = false;
                        }
                    }
                }
                if socket_permit {
                    info!("Accepted connection from: {}", addr);
                    let process_permit = Arc::new(Semaphore::new(
                        *self.lynn_config.get_server_single_processs_permit(),
                    ));
                    let clients_clone = self.clients.0.clone();
                    let clients_clone_alone = self.clients.0.clone();
                    let router_map_async = self.router_map_async.0.clone();
                    let thread_pool_task_body_sender_clone =
                        self.lynn_thread_pool.task_body_sender.0.clone();
                    let message_header_mark = self.lynn_config.get_message_header_mark().clone();
                    let message_tail_mark = self.lynn_config.get_message_tail_mark().clone();
                    let message_header_mark_clone = message_header_mark.clone();
                    let message_tail_mark_clone = message_tail_mark.clone();
                    tokio::spawn(async move {
                        // Creates a channel for sending data to the client.
                        let (tx, mut rx) =
                            mpsc::channel::<HandlerResult>(DEFAULT_SYSTEM_CHANNEL_SIZE);
                        let last_communicate_time = Arc::new(Mutex::new(SystemTime::now()));
                        let addr = addr.clone();
                        let process_permit_clone = process_permit.clone();
                        let last_communicate_time_clone = last_communicate_time.clone();
                        // Spawns a new asynchronous task to handle each client connection.
                        let join_handle = tokio::spawn(async move {
                            let stream = socket; // 获取TcpStream
                            let (mut read_half, mut write_half) = split(stream);
                            let mut buf = [0; DEFAULT_MAX_RECEIVE_BYTES_SIZE];
                            let mut big_buf =
                                BigBufReader::new(message_header_mark, message_tail_mark);
                            let addr = addr;
                            // Write data to the client in a loop.
                            let join_handle = tokio::spawn(async move {
                                while let Some(mut handler_result) = rx.recv().await {
                                    if !handler_result.is_with_mark() {
                                        handler_result.set_marks(
                                            message_header_mark_clone.clone(),
                                            message_tail_mark_clone.clone(),
                                        );
                                    }
                                    if let Some(response_data) = handler_result.get_response_data()
                                    {
                                        if let Err(e) = write_half.write_all(&response_data).await {
                                            error!("Failed to write to socket: {}", e);
                                            continue;
                                        }
                                    }
                                }
                            });
                            // Reads data sent by the client in a loop.
                            loop {
                                let result = read_half.read(&mut buf).await;
                                match result {
                                    Ok(n) if n <= 0 => continue,
                                    Ok(n) => {
                                        let last_communicate_time = last_communicate_time.clone();
                                        tokio::spawn(async move {
                                            let time_now = SystemTime::now();
                                            let mut mutex = last_communicate_time.lock().await;
                                            let guard = mutex.deref_mut();
                                            let time_old = guard.clone();
                                            match time_old.partial_cmp(&time_now) {
                                                Some(std::cmp::Ordering::Less) => {
                                                    *guard = time_now;
                                                }
                                                Some(
                                                    std::cmp::Ordering::Equal
                                                    | std::cmp::Ordering::Greater,
                                                )
                                                | None => {}
                                            }
                                        });
                                        big_buf.extend_from_slice(&buf[..n]);
                                        while big_buf.is_complete() {
                                            let mut input_buf_vo =
                                                InputBufVO::new(big_buf.get_data(), addr);
                                            if let Some(method_id) = input_buf_vo.get_method_id() {
                                                let guard = router_map_async.deref();
                                                if let Some(map) = guard {
                                                    if map.contains_key(&method_id) {
                                                        let a = map.get(&method_id).unwrap();
                                                        input_dto_build(
                                                            addr,
                                                            input_buf_vo,
                                                            process_permit.clone(),
                                                            clients_clone.clone(),
                                                            a.clone(),
                                                            thread_pool_task_body_sender_clone
                                                                .clone(),
                                                        )
                                                        .await;
                                                    } else {
                                                        warn!(
                                                            "router_map_async no method match,{}",
                                                            method_id
                                                        );
                                                    }
                                                } else {
                                                    warn!("server router is none");
                                                }
                                            } else {
                                                warn!("router_map_async input_buf_vo no method_id");
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to read from socket: {}", e);
                                        break;
                                    }
                                }
                            }

                            // Removes the client from the HashMap after the connection is closed.
                            {
                                let mut clients = clients_clone.write().await;
                                let guard = clients.deref_mut();
                                if guard.contains_key(&addr) {
                                    guard.remove(&addr);
                                }
                                join_handle.abort();
                            }
                        });
                        // Saves the client's ID and send channel to the HashMap.
                        {
                            let clients_clone = clients_clone_alone.clone();
                            let mut clients = clients_clone.write().await;
                            let guard = clients.deref_mut();
                            let lynn_user = LynnUser::new(
                                tx.clone(),
                                process_permit_clone,
                                join_handle,
                                last_communicate_time_clone,
                            );
                            guard.insert(addr, lynn_user);
                        }
                    });
                } else {
                    let _ = socket.shutdown().await;
                    warn!("Server socket's count is more than MAX_CONNECTIONS ,can not accept new client:{}",addr);
                }
            } else {
                warn!("Failed to accept connection , server run next");
            }
        }
    }

    /// Logs server information.
    pub(crate) async fn log_server(&self) {
        let subscriber = fmt::Subscriber::builder()
            .with_max_level(Level::DEBUG)
            .finish();
        match tracing::subscriber::set_global_default(subscriber) {
            Ok(_) => {
                info!("Server - [log server] start sucess!!!")
            },
            Err(e) => {
                warn!("set_global_default failed - e: {:?}", e)
            },
        }
    }
}
