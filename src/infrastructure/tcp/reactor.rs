use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime},
};

use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::{
        RwLock, Semaphore,
        mpsc::{self, Sender},
    },
    task::yield_now,
};
use tracing::{error, info, warn};

use crate::application::server::lynn_server::{ReactorEventSender, TaskBodyOutChannel};
use crate::application::server::server_common::{add_client, check_handler_result, push_read_half};
use crate::domain::model::lynn_user::ClientsStructType;
use crate::domain::routing::router::LynnRouter;
use crate::domain::state::state_registry::StateRegistry;
use crate::infrastructure::connection::connection_limiter::ConnectionLimiter;
use crate::infrastructure::tcp::stream::{BoxedReadHalf, LynnStream, StreamAcceptor};
use crate::infrastructure::tcp::tcp_socket_config::TcpSocketConfig;

/// Reactor event type
enum EventType {
    NewSocket((TcpStream, core::net::SocketAddr)),
    ExcuteTask(TaskBodyOutChannel),
}

/// Event for the reactor system
pub(crate) struct ReactorEvent {
    event_type: EventType,
}

impl ReactorEvent {
    #[inline(always)]
    fn new_with_event_type(event_type: EventType) -> Self {
        Self { event_type }
    }

    #[inline(always)]
    pub(crate) fn crate_new_socket_event(socket: TcpStream, addr: core::net::SocketAddr) -> Self {
        ReactorEvent::new_with_event_type(EventType::NewSocket((socket, addr)))
    }

    #[inline(always)]
    pub(crate) fn crate_excute_task_event(task_body: TaskBodyOutChannel) -> Self {
        ReactorEvent::new_with_event_type(EventType::ExcuteTask(task_body))
    }
}

/// Per-connection setup payload handed from the event workers to the
/// read-loop spawner task.
pub(crate) struct NewSocketTask {
    /// The boxed read half of the accepted (and possibly TLS-wrapped) stream.
    pub(crate) read_half: BoxedReadHalf,
    pub(crate) process_permit: Arc<Semaphore>,
    pub(crate) addr: SocketAddr,
    pub(crate) clients: ClientsStructType,
    pub(crate) message_header_mark: u16,
    pub(crate) message_tail_mark: u16,
    pub(crate) lynn_router: Arc<LynnRouter>,
    pub(crate) reactor_event_sender: ReactorEventSender,
    pub(crate) last_communicate_time: Arc<RwLock<SystemTime>>,
}

/// New socket event sender type
type NewSocketEventSender = Sender<NewSocketTask>;

/// Event manager - manages the work-stealing event queue
pub(crate) struct EventManager {
    global_queue: ReactorEventSender,
}

impl EventManager {
    pub(crate) fn new() -> Self {
        let global_queue = Arc::new(Injector::<ReactorEvent>::new());
        EventManager { global_queue }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn run(
        &self,
        clients: ClientsStructType,
        server_single_processs_permit: &usize,
        message_header_mark: u16,
        message_tail_mark: u16,
        lynn_router: Arc<LynnRouter>,
        reactor_event_sender: ReactorEventSender,
        tx: NewSocketEventSender,
        server_max_reactor_taskpool_size: &usize,
        stream_acceptor: Arc<StreamAcceptor>,
    ) {
        let mut local_queues: Vec<Worker<ReactorEvent>> =
            Vec::with_capacity(*server_max_reactor_taskpool_size);
        let mut stealers: Vec<Stealer<ReactorEvent>> =
            Vec::with_capacity(*server_max_reactor_taskpool_size);
        for _ in 0..*server_max_reactor_taskpool_size {
            let worker = Worker::new_fifo();
            stealers.push(worker.stealer());
            local_queues.push(worker);
        }
        let global_queue = self.global_queue.clone();
        let stealers_arc = Arc::new(stealers);
        for (index, local_queue) in local_queues.into_iter().enumerate() {
            let global_queue_clone = global_queue.clone();
            let stealers_arc_clone = stealers_arc.clone();
            let clients_clone = clients.clone();
            let server_single_processs_permit = *server_single_processs_permit;
            let reactor_event_sender = reactor_event_sender.clone();
            let tx = tx.clone();
            let lynn_router = lynn_router.clone();
            let stream_acceptor = stream_acceptor.clone();
            let mut idle_count: u16 = 0;

            tokio::spawn(async move {
                let local_queue = local_queue;
                let global_queue = global_queue_clone;
                let stealers_arc = stealers_arc_clone;
                let clients = clients_clone;
                loop {
                    if let Some(event) =
                        get_event(&local_queue, &global_queue, &stealers_arc, index)
                    {
                        idle_count = 0;
                        match event.event_type {
                            EventType::NewSocket((socket, addr)) => {
                                // Perform the (optional) TLS handshake here, off the
                                // accept loop, so handshakes never serialize accepts.
                                let Some(stream) = stream_acceptor.accept(socket, addr).await
                                else {
                                    continue;
                                };
                                let last_communicate_time =
                                    Arc::new(RwLock::new(SystemTime::now()));
                                let read_half = add_client(
                                    clients.clone(),
                                    stream,
                                    addr,
                                    last_communicate_time.clone(),
                                )
                                .await;
                                let process_permit =
                                    Arc::new(Semaphore::new(server_single_processs_permit));
                                let _ = tx
                                    .send(NewSocketTask {
                                        read_half,
                                        process_permit,
                                        addr,
                                        clients: clients.clone(),
                                        message_header_mark,
                                        message_tail_mark,
                                        lynn_router: lynn_router.clone(),
                                        reactor_event_sender: reactor_event_sender.clone(),
                                        last_communicate_time,
                                    })
                                    .await;
                            },
                            EventType::ExcuteTask(task_body) => {
                                let (task, context, clients) = task_body;
                                // Run the handler in a nested task so a panicking
                                // handler (e.g. a missing AppState) cannot take
                                // down this worker.
                                match tokio::spawn(async move { task.handler(context).await }).await
                                {
                                    Ok(result) => {
                                        check_handler_result(result, clients.clone()).await;
                                    },
                                    Err(join_error) => {
                                        error!(
                                            "Handler task panicked or was cancelled: {}",
                                            join_error
                                        );
                                    },
                                }
                            },
                        }
                    } else {
                        idle_count += 1;
                        if idle_count < 32 {
                            // Temporarily relinquish control
                            yield_now().await;
                        } else if idle_count < 10_240 {
                            // A slightly longer wait
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        } else if idle_count < 32767 {
                            // A slightly longer wait
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        } else {
                            // Longer waiting time reduces CPU usage
                            tokio::time::sleep(Duration::from_millis(25)).await;
                        }
                    }
                }
            });
        }
    }

    pub(crate) fn get_global_queue(&self) -> ReactorEventSender {
        self.global_queue.clone()
    }
}

#[inline(always)]
fn get_event(
    local_queue: &Worker<ReactorEvent>,
    global_queue: &ReactorEventSender,
    stealers_arc: &Arc<Vec<Stealer<ReactorEvent>>>,
    worker_index: usize,
) -> Option<ReactorEvent> {
    // 1. local
    if let Some(event) = local_queue.pop() {
        return Some(event);
    }

    // 2. global
    if let Steal::Success(event) = global_queue.steal_batch_and_pop(local_queue) {
        return Some(event);
    }

    // 3. stealers
    let stealers_len = stealers_arc.len();

    if stealers_len > 1 {
        let start_index = (worker_index + 1) % stealers_len;

        for i in 0..stealers_len {
            let steal_index = (start_index + i) % stealers_len;

            match stealers_arc[steal_index].steal() {
                Steal::Success(event) => return Some(event),

                Steal::Empty | Steal::Retry => continue,
            }
        }
    } else if stealers_len == 1 {
        match stealers_arc[0].steal() {
            Steal::Success(event) => return Some(event),

            Steal::Empty | Steal::Retry => {},
        }
    }

    None
}

/// Core reactor - handles TCP connections
pub(crate) struct CoreReactor {
    pub(crate) tx: NewSocketEventSender,
}

impl CoreReactor {
    pub(crate) fn new(states: Arc<StateRegistry>) -> Self {
        let (tx, mut rx) = mpsc::channel::<NewSocketTask>(64);
        tokio::spawn(async move {
            while let Some(task) = rx.recv().await {
                push_read_half(
                    task.read_half,
                    task.process_permit,
                    task.addr,
                    task.clients,
                    task.message_header_mark,
                    task.message_tail_mark,
                    task.lynn_router,
                    task.reactor_event_sender,
                    task.last_communicate_time,
                    states.clone(),
                )
                .await;
            }
        });

        Self { tx }
    }

    pub(crate) async fn run(
        &self,
        tcp_listener: TcpListener,
        clients: ClientsStructType,
        alow_max_connections: Option<&usize>,
        global_queue: ReactorEventSender,
        connection_limiter: Option<(
            &u64,   // rate_limit
            &usize, // max_connections_per_ip
            Arc<ConnectionLimiter>,
        )>,
        tcp_config: TcpSocketConfig,
    ) {
        loop {
            // Waiting for a new link
            let clinet_result = tcp_listener.accept().await;
            if let Ok((socket, addr)) = clinet_result {
                let mut socket_permit = true;
                let mut socket = Some(socket);

                // Apply TCP socket configuration
                if let Some(ref s) = socket {
                    if let Err(e) = s.set_nodelay(tcp_config.nodelay) {
                        warn!(
                            "Failed to set TCP_NODELAY for {}: {}, using default",
                            addr, e
                        );
                    }
                }

                // Apply TCP keep-alive and buffer sizes using socket2
                if tcp_config.keepalive_enabled
                    || tcp_config.recv_buffer_size > 0
                    || tcp_config.send_buffer_size > 0
                {
                    if let Some(s) = socket.take() {
                        // Convert TcpStream to std::net::TcpStream to access socket2
                        match s.into_std() {
                            Ok(std_socket) => {
                                use socket2::Socket;
                                let socket2 = Socket::from(std_socket);

                                // Apply TCP keep-alive if enabled
                                if tcp_config.keepalive_enabled {
                                    use socket2::TcpKeepalive;
                                    let ka = TcpKeepalive::new().with_time(
                                        std::time::Duration::from_secs(
                                            tcp_config.keepalive_time_secs,
                                        ),
                                    );
                                    if let Err(e) = socket2.set_tcp_keepalive(&ka) {
                                        warn!(
                                            "Failed to set keep-alive for {}: {}, using default",
                                            addr, e
                                        );
                                    }
                                }

                                // Set buffer sizes if specified (non-zero)
                                if tcp_config.recv_buffer_size > 0 {
                                    if let Err(e) =
                                        socket2.set_recv_buffer_size(tcp_config.recv_buffer_size)
                                    {
                                        warn!(
                                            "Failed to set recv buffer size for {}: {}, using default",
                                            addr, e
                                        );
                                    }
                                }

                                if tcp_config.send_buffer_size > 0 {
                                    if let Err(e) =
                                        socket2.set_send_buffer_size(tcp_config.send_buffer_size)
                                    {
                                        warn!(
                                            "Failed to set send buffer size for {}: {}, using default",
                                            addr, e
                                        );
                                    }
                                }

                                // Convert back to TcpStream
                                let std_socket: std::net::TcpStream = socket2.into();
                                match TcpStream::from_std(std_socket) {
                                    Ok(new_s) => {
                                        socket = Some(new_s);
                                    },
                                    Err(e) => {
                                        warn!(
                                            "Failed to convert back to TcpStream for {}: {}, closing connection",
                                            addr, e
                                        );
                                        socket_permit = false;
                                    },
                                }
                            },
                            Err(e) => {
                                warn!(
                                    "Failed to convert TcpStream for {}: {}, using default settings",
                                    addr, e
                                );
                                // Note: We lost the socket here, but this is a very rare error case
                                // In practice, this conversion should never fail
                                socket_permit = false;
                            },
                        }
                    }
                }

                // Check global max connections
                if let Some(max_connections) = alow_max_connections {
                    if clients.len() >= *max_connections {
                        socket_permit = false;
                        warn!(
                            "Server socket's count is more than MAX_CONNECTIONS, can not accept new client:{}",
                            addr
                        );
                    }
                }

                // Check connection limiter (rate limit and per-IP limit)
                if socket_permit {
                    if let Some((_, _, limiter)) = &connection_limiter {
                        let ip = addr.ip();
                        if !limiter.check_connection(ip).await {
                            socket_permit = false;
                            warn!(
                                "Connection from {} rejected by connection limiter (rate limit or per-IP limit exceeded)",
                                addr
                            );
                        }
                    }
                }

                if socket_permit {
                    if let Some(s) = socket {
                        info!("Accepted connection from: {}", addr);
                        global_queue.push(ReactorEvent::crate_new_socket_event(s, addr));
                    }
                } else {
                    if let Some(mut s) = socket {
                        let _ = s.shutdown().await;
                    }
                }
            } else {
                if let Err(e) = clinet_result {
                    warn!(
                        "Failed to accept connection, server run next, e :{}",
                        e.to_string()
                    );
                }
            }
        }
    }
}

/// TcpReactor - the main reactor struct
pub(crate) struct TcpReactor {
    core_reactor: CoreReactor,
    event_manager: EventManager,
}

impl TcpReactor {
    pub(crate) fn new(states: Arc<StateRegistry>) -> Self {
        Self {
            core_reactor: CoreReactor::new(states),
            event_manager: EventManager::new(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn start(
        &self,
        clients: ClientsStructType,
        server_single_processs_permit: &usize,
        message_header_mark: u16,
        message_tail_mark: u16,
        lynn_router: Arc<LynnRouter>,
        tcp_listener: TcpListener,
        alow_max_connections: Option<&usize>,
        server_max_reactor_taskpool_size: &usize,
        connection_limiter: Option<(
            &u64,   // rate_limit
            &usize, // max_connections_per_ip
            Arc<ConnectionLimiter>,
        )>,
        tcp_config: TcpSocketConfig,
        stream_acceptor: Arc<StreamAcceptor>,
    ) {
        self.event_manager.run(
            clients.clone(),
            server_single_processs_permit,
            message_header_mark,
            message_tail_mark,
            lynn_router,
            self.event_manager.get_global_queue(),
            self.core_reactor.tx.clone(),
            server_max_reactor_taskpool_size,
            stream_acceptor,
        );

        // Spawn cleanup task for connection limiter if rate limiting is enabled
        if let Some((rate_limit, _, limiter)) = &connection_limiter {
            if **rate_limit > 0 {
                limiter.clone().spawn_cleanup_task();
            }
        }

        self.core_reactor
            .run(
                tcp_listener,
                clients,
                alow_max_connections,
                self.event_manager.get_global_queue(),
                connection_limiter,
                tcp_config,
            )
            .await;
    }
}
