mod lynn_server_config;
mod lynn_server_user;
mod server_thread_pool;

use std::{
    collections::HashMap,
    net::SocketAddr,
    ops::{Deref, DerefMut},
    sync::Arc,
    time::{Duration, SystemTime},
};

use lynn_server_config::{LynnServerConfig, LynnServerConfigBuilder};
use lynn_server_user::LynnUser;
use server_thread_pool::LynnServerThreadPool;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, Mutex, Semaphore},
    task::JoinHandle,
    time::interval,
};
use tracing::{error, info, warn, Level};
use tracing_subscriber::fmt;

use crate::{
    const_config::DEFAULT_MAX_RECEIVE_BYTES_SIZE,
    dto_factory::input_dto_build,
    service::IService,
    vo_factory::{input_vo::InputBufVO, InputBufVOTrait},
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
/// use lynn_tcp::app::LynnServer;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let _ = LynnServer::new().await.add_router(1, my_service).start().await;
///     Ok(())
/// }
/// pub fn my_service(input_buf_vo: &mut InputBufVO) -> HandlerResult {
///     println!("service read from :{}", input_buf_vo.get_input_addr());
///     HandlerResult::new_without_send()
/// }
/// ```
/// # Example
/// Use customized config
/// ```rust
/// use lynn_tcp::app::LynnServer;
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
/// .add_router(1, my_service)
/// .start()
/// .await;
/// Ok(())
/// }
/// pub fn my_service(input_buf_vo: &mut InputBufVO) -> HandlerResult {
///     println!("service read from :{}", input_buf_vo.get_input_addr());
///     HandlerResult::new_without_send()
/// }
/// ```
pub struct LynnServer<'a> {
    /// A map of connected clients, where the key is the client's address and the value is a `LynnUser` instance.
    clients: Arc<Mutex<HashMap<SocketAddr, LynnUser>>>,
    /// A map of routes, where the key is a method ID and the value is a service handler.
    router_map: Arc<Mutex<HashMap<u16, Arc<Box<dyn IService>>>>>,
    /// The configuration for the server.
    lynn_config: LynnServerConfig<'a>,
    /// The thread pool for the server.
    lynn_thread_pool: Arc<Mutex<LynnServerThreadPool>>,
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
        let server_single_processs_permit = lynn_config.get_server_single_processs_permit();
        let thread_pool =
            LynnServerThreadPool::new(server_max_threadpool_size, server_single_processs_permit).await;
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            router_map: Arc::new(Mutex::new(HashMap::new())),
            lynn_config,
            lynn_thread_pool: Arc::new(Mutex::new(thread_pool)),
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
    pub async fn new_with_ipv4(ipv4: &'a str) -> Self {
        let lynn_config = LynnServerConfigBuilder::new().with_server_ipv4(ipv4).build();
        let server_max_threadpool_size = lynn_config.get_server_max_threadpool_size();
        let server_single_processs_permit = lynn_config.get_server_single_processs_permit();
        let thread_pool =
            LynnServerThreadPool::new(server_max_threadpool_size, server_single_processs_permit).await;
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            router_map: Arc::new(Mutex::new(HashMap::new())),
            lynn_config,
            lynn_thread_pool: Arc::new(Mutex::new(thread_pool)),
        }
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
        let server_max_threadpool_size = lynn_config.get_server_max_threadpool_size();
        let server_single_processs_permit = lynn_config.get_server_single_processs_permit();
        let thread_pool =
            LynnServerThreadPool::new(server_max_threadpool_size, server_single_processs_permit).await;
        Self {
            clients: Arc::new(Mutex::new(HashMap::new())),
            router_map: Arc::new(Mutex::new(HashMap::new())),
            lynn_config,
            lynn_thread_pool: Arc::new(Mutex::new(thread_pool)),
        }
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
    pub fn add_router(self, method_id: u16, handler: impl IService + 'static) -> Self {
        let router_map = self.router_map.clone();
        tokio::spawn(async move {
            let mut router_map_mutex = router_map.lock().await;
            let router_map_guard = router_map_mutex.deref_mut();
            router_map_guard.insert(method_id, Arc::new(Box::new(handler)));
        });
        self
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
        sender: mpsc::Sender<Vec<u8>>,
        addr: SocketAddr,
        process_permit: Arc<Semaphore>,
        join_handle: JoinHandle<()>,
        last_communicate_time: Arc<Mutex<SystemTime>>,
    ) {
        let mut clients = self.clients.lock().await;
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
        let mut clients = self.clients.lock().await;
        let guard = clients.deref_mut();
        if guard.contains_key(&addr) {
            guard.remove(&addr);
        }
    }

    /// Checks the heartbeat of connected clients and removes those that have not sent messages for a long time.
    pub(crate) async fn check_heart(&self) {
        let clients = self.clients.clone();
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
                let mut clients_mutex = clients.lock().await;
                let guard = clients_mutex.deref_mut();
                for (addr, lynn_user) in guard.iter() {
                    let last_communicate_time = lynn_user.last_communicate_time.lock().await;
                    let time_old = last_communicate_time.deref().clone();
                    let time_now = SystemTime::now();
                    match time_old.partial_cmp(&time_now) {
                        Some(std::cmp::Ordering::Less) => match time_now.duration_since(time_old) {
                            Ok(duration) => {
                                if duration.as_secs() > server_check_heart_timeout_time {
                                    remove_list.push(addr.clone());
                                }
                            }
                            Err(e) => {
                                warn!("unable to compare time,{}", e)
                            }
                        },
                        Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater) | None => {}
                    }
                }
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

    pub async fn start(self: Self) {
        self.log_server().await;
        let server_arc = Arc::new(self);
        server_arc.run().await;
    }

    /// Starts the server and begins listening for client connections.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the server starts successfully, otherwise returns an error.
    async fn run(self: Arc<Self>) -> Result<(), Box<dyn std::error::Error>> {
        /// Binds a TCP listener to the local address.
        let listener = TcpListener::bind(self.lynn_config.get_server_ipv4()).await?;
        info!(
            "Server - [Main-LynnServer] start success!!! with [server_ipv4:{}]",
            self.lynn_config.get_server_ipv4()
        );

        self.check_heart().await;

        loop {
            /// Waits for a client to connect.
            let clinet_result = listener.accept().await;
            if let Ok((mut socket, addr)) = clinet_result {
                let mut socket_permit = true;
                {
                    if let Some(max_connections) = self.lynn_config.get_server_max_connections() {
                        let clients = self.clients.lock().await;
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

                    /// Creates a channel for sending data to the client.
                    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(
                        *self.lynn_config.get_server_single_channel_size(),
                    );
                    let process_permit = Arc::new(Semaphore::new(
                        *self.lynn_config.get_server_single_processs_permit(),
                    ));
                    let last_communicate_time = Arc::new(Mutex::new(SystemTime::now()));

                    let clients_clone = self.clients.clone();
                    let router_map = self.router_map.clone();
                    let addr = addr.clone();
                    let process_permit_clone = process_permit.clone();
                    let last_communicate_time_clone = last_communicate_time.clone();
                    let thread_pool_clone = self.lynn_thread_pool.clone();
                    /// Spawns a new asynchronous task to handle each client connection.
                    let join_handle = tokio::spawn(async move {
                        let mut stream = socket; // 获取TcpStream
                        let mut buf = [0; DEFAULT_MAX_RECEIVE_BYTES_SIZE];
                        let client_uuid = addr;
                        /// Reads data sent by the client in a loop.
                        loop {
                            tokio::select! {
                                result = stream.read(&mut buf) => {
                                    match result {
                                        Ok(n) if n < 3 => {
                                            continue
                                        },
                                        Ok(n) => {
                                            let last_communicate_time = last_communicate_time.clone();
                                            tokio::spawn(async move {
                                                let time_now = SystemTime::now();
                                                let mut mutex = last_communicate_time.lock().await;
                                                let guard = mutex.deref_mut();
                                                let time_old = guard.clone();
                                                match time_old.partial_cmp(&time_now) {
                                                    Some(std::cmp::Ordering::Less) => {
                                                        *guard=time_now;
                                                    },
                                                    Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater)|None => {},
                                                }
                                            });

                                            let mut input_buf_vo = InputBufVO::new(buf[..n].to_vec(),addr);
                                            if let Some(method_id) = input_buf_vo.get_method_id(){
                                                let mut mutex = router_map.lock().await;
                                                let guard = mutex.deref_mut();
                                                if guard.contains_key(&method_id) {
                                                    let a = guard.get(&method_id).unwrap();
                                                    input_dto_build(addr,input_buf_vo,process_permit.clone(),clients_clone.clone(),a.clone(),thread_pool_clone.clone()).await;
                                                }else{
                                                    warn!("router_map no method match,{}",method_id);
                                                }
                                            }else{
                                                warn!("input_buf_vo no method_id");
                                            }
                                        },
                                        Err(e) => {
                                            error!("Failed to read from socket: {}", e);
                                            break;
                                        }
                                    }
                                },
                                Some(response_data) = rx.recv() => {
                                    if let Err(e) = stream.write_all(&response_data).await {
                                        error!("Failed to write to socket: {}", e);
                                        break;
                                    }
                                }
                            }
                        }

                        /// Removes the client from the HashMap after the connection is closed.
                        {
                            let mut clients = clients_clone.lock().await;
                            let guard = clients.deref_mut();
                            if guard.contains_key(&client_uuid) {
                                guard.remove(&client_uuid);
                            }
                        }
                    });
                    /// Saves the client's ID and send channel to the HashMap.
                    self.add_client(
                        tx.clone(),
                        addr,
                        process_permit_clone,
                        join_handle,
                        last_communicate_time_clone,
                    )
                    .await;
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
        tracing::subscriber::set_global_default(subscriber)
            .expect("failed to set global subscribers");
    }
}
