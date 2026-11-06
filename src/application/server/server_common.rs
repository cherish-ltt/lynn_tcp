use std::{
    net::SocketAddr,
    sync::Arc,
    time::{Duration, SystemTime},
};

use bytes::BytesMut;
use tokio::{
    io::AsyncReadExt,
    sync::{RwLock, Semaphore},
    time::interval,
};
use tracing::{error, info, warn};

use crate::application::server::connection_session::ConnectionSession;
use crate::application::server::lynn_server::ReactorEventSender;
use crate::application::server::server_tasks::ServerTasks;
use crate::const_config::DEFAULT_MAX_RECEIVE_BYTES_SIZE;
use crate::domain::handler::handler_system::{ClientsContext, HandlerContext};
use crate::domain::handler::request_context::RequestContext;
use crate::domain::model::handler_result::HandlerResult;
use crate::domain::model::input_buf_vo::{InputBufVO, InputBufVOTrait};
use crate::domain::model::lynn_user::LynnUser;
use crate::domain::model::lynn_user::{ClientsStruct, ClientsStructType};
use crate::domain::model::session::SessionHandle;
use crate::domain::routing::router::LynnRouter;
use crate::domain::routing::router::Route;
use crate::domain::state::state_registry::StateRegistry;
use crate::infrastructure::connection::connection_limiter::ConnectionPermits;
use crate::infrastructure::metrics::metrics::METRICS;
use crate::infrastructure::protocol::big_buf_reader::BigBufReader;
use crate::infrastructure::tcp::io_timeout::with_io_timeout;
use crate::infrastructure::tcp::reactor::ReactorEvent;
use crate::infrastructure::tcp::stream::{BoxedReadHalf, LynnStream, split_transport};

#[inline(always)]
pub(super) async fn spawn_check_heart(
    server_check_heart_interval: u64,
    server_check_heart_timeout_time: u64,
    clients: ClientsStructType,
) {
    let mut interval = interval(Duration::from_secs(server_check_heart_interval));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    info!(
        "Server - [check heart] start success!!! with [server_check_heart_interval:{}s] [server_check_heart_timeout_time:{}s]",
        server_check_heart_interval, server_check_heart_timeout_time
    );

    let timeout_duration = Duration::from_secs(server_check_heart_timeout_time);

    loop {
        interval.tick().await;

        let mut remove_list = Vec::with_capacity(clients.len() / 10); // Assuming that up to 10% of clients may timeout
        let current_time = SystemTime::now();

        for entry in clients.iter() {
            let addr = *entry.key();
            let last_communicate_time = entry.value().get_last_communicate_time();

            if let Ok(last_time_guard) = last_communicate_time.try_read() {
                let time_old = *last_time_guard;

                if let Ok(duration) = current_time.duration_since(time_old)
                    && duration > timeout_duration
                {
                    remove_list.push((addr, last_communicate_time.clone()));
                }
            }
            // If unable to obtain a read lock, skip the client to avoid blocking
        }

        for (addr, identity) in remove_list {
            if clients
                .remove_if(&addr, |_, user| {
                    Arc::ptr_eq(&user.get_last_communicate_time(), &identity)
                })
                .is_some()
            {
                info!(
                    "Clean up addr:{}, that have not sent messages for a long time",
                    addr
                );
            }
        }

        info!("Server check online socket count:{}", clients.len());
    }
}

/// A function for checking and sending a HandlerResult instance.
///
/// This function checks the HandlerResult instance and sends it through a channel if the send flag is set to true.
#[inline(always)]
pub(crate) async fn check_handler_result(
    mut handler_result: HandlerResult,
    clients: ClientsStructType,
    message_header_mark: u16,
    message_tail_mark: u16,
    tasks: &ServerTasks,
) {
    // If the send flag of the HandlerResult instance is set to true, send the instance through the channel.
    if handler_result.get_is_send() {
        // Marks must be applied before the frame is encoded, otherwise the
        // response would fall back to the default marks even on servers
        // configured with custom header/tail marks.
        if !handler_result.is_with_mark() {
            handler_result.set_marks(message_header_mark, message_tail_mark);
        }
        let response = handler_result.get_response_data();
        if let Some(response) = response {
            if let Some(addrs) = handler_result.get_addrs() {
                if let Some(delay_socket) = send_response(&response, &addrs, &clients).await {
                    if !delay_socket.is_empty() {
                        tasks.record_failure();
                        delay_socket.iter().for_each(|addr|{
                            warn!("Failed to find the client correctly, message sending is invalid , target-addr:{}",addr);
                        });
                    }
                }
            }
        }
    }
}

#[inline(always)]
async fn send_response(
    response: &BytesMut,
    addrs: &[SocketAddr],
    clients: &ClientsStructType,
) -> Option<Vec<SocketAddr>> {
    if addrs.is_empty() {
        return None;
    }

    let mut delay_socket = Vec::with_capacity(addrs.len());

    for socket_addr in addrs {
        let send = clients
            .get(socket_addr)
            .map(|user| user.send_response(response));
        if let Some(send) = send {
            send.await;
        } else {
            METRICS.messages.dropped_total.inc();
            delay_socket.push(*socket_addr);
        }
    }

    if delay_socket.is_empty() {
        None
    } else {
        Some(delay_socket)
    }
}

#[inline(always)]
pub(crate) async fn input_dto_build(
    addr: SocketAddr,
    method_id: u16,
    input_buf_vo: InputBufVO,
    process_permit: Arc<Semaphore>,
    clients: ClientsStructType,
    handler_method: Arc<Route>,
    reactor_event_sender: ReactorEventSender,
    states: Arc<StateRegistry>,
    tasks: &ServerTasks,
    session: SessionHandle,
) {
    let Some(_admission) = tasks.try_admit() else {
        return;
    };
    // Attempt to acquire a permit from the semaphore.
    match process_permit.try_acquire_owned() {
        Ok(permit) => {
            reactor_event_sender.push(ReactorEvent::crate_execute_task_event((
                handler_method,
                HandlerContext::new(
                    input_buf_vo,
                    ClientsContext::new(ClientsStruct(clients.clone())),
                    states,
                    RequestContext::new(method_id, addr, session),
                ),
                clients,
                permit,
                tasks.request(),
            )));
        },
        Err(_) => {
            METRICS.messages.dropped_total.inc();
            // If the permit cannot be acquired, log a warning.
            warn!("addr:{} PROCESS_PERMIT_SIZE is full", addr)
        },
    }
}

#[inline(always)]
pub(crate) async fn add_client(
    clients: ClientsStructType,
    stream: LynnStream,
    addr: SocketAddr,
    last_communicate_time: Arc<RwLock<SystemTime>>,
    mut permit: ConnectionPermits,
    write_timeout_secs: u64,
    tasks: ServerTasks,
    marks: (u16, u16),
) -> Option<(BoxedReadHalf, ConnectionSession)> {
    let admission = tasks.try_admit()?;
    if let Some(observation) = &mut permit.observation {
        observation.established = true;
    }
    let (read_half, write_half, observer) = split_transport(stream, tasks.connection());
    let (lynn_user, writer, closed) = LynnUser::new(
        write_half,
        last_communicate_time.clone(),
        write_timeout_secs,
        tasks.clone(),
    );
    let handle = SessionHandle::new(&lynn_user, observer, closed.clone(), tasks.clone(), marks);
    clients.insert(addr, lynn_user);
    drop(admission);
    Some((
        read_half,
        ConnectionSession {
            handle,
            writer,
            closed,
            clients,
            addr,
            last_communicate_time,
            permit,
            tasks,
        },
    ))
}

#[inline(always)]
pub(crate) async fn read_connection(
    mut read_half: BoxedReadHalf,
    process_permit: Arc<Semaphore>,
    addr: SocketAddr,
    clients: ClientsStructType,
    message_header_mark: u16,
    message_tail_mark: u16,
    lynn_router: Arc<LynnRouter>,
    reactor_event_sender: ReactorEventSender,
    last_communicate_time: Arc<RwLock<SystemTime>>,
    states: Arc<StateRegistry>,
    mut session: ConnectionSession,
    process_limit: usize,
    read_timeout_secs: u64,
) {
    let tasks = session.tasks.clone();
    let read = async {
        let mut buf = [0; DEFAULT_MAX_RECEIVE_BYTES_SIZE];
        let mut big_buf = BigBufReader::new(message_header_mark, message_tail_mark);

        'read_loop: loop {
            let result = with_io_timeout(read_timeout_secs, read_half.read(&mut buf)).await;
            match result {
                Ok(0) => break true,
                Ok(n) => {
                    if let Err(error) = big_buf.extend_from_slice(&buf[..n]) {
                        METRICS.messages.invalid_total.inc();
                        warn!("Invalid frame from {addr}: {error}");
                        tasks.record_failure();
                        break false;
                    }
                    loop {
                        let payload = match big_buf.next_frame() {
                            Ok(Some(payload)) => payload,
                            Ok(None) => break,
                            Err(error) => {
                                METRICS.messages.invalid_total.inc();
                                warn!("Invalid frame from {addr}: {error}");
                                tasks.record_failure();
                                break 'read_loop false;
                            },
                        };
                        METRICS.messages.received_total.inc();
                        METRICS
                            .messages
                            .size_bytes
                            .observe((payload.len() + 12) as f64);
                        let mut input_buf_vo = InputBufVO::new(payload, addr);
                        if let Some(constructor_id) = input_buf_vo.get_constructor_id() {
                            match constructor_id {
                                2 => {
                                    if let Ok(mut time_guard) = last_communicate_time.try_write() {
                                        *time_guard = SystemTime::now();
                                    }
                                    continue;
                                },
                                1 => {
                                    if let Some(method_id) = input_buf_vo.get_method_id() {
                                        if let Some(handler_method) =
                                            lynn_router.get_handler_by_method_id(&method_id)
                                        {
                                            input_dto_build(
                                                addr,
                                                method_id,
                                                input_buf_vo,
                                                process_permit.clone(),
                                                clients.clone(),
                                                handler_method.clone(),
                                                reactor_event_sender.clone(),
                                                states.clone(),
                                                &tasks,
                                                session.handle.clone(),
                                            )
                                            .await;
                                        } else {
                                            METRICS.messages.dropped_total.inc();
                                            warn!("router_map_async no method match,{}", method_id);
                                        }
                                    } else {
                                        METRICS.messages.dropped_total.inc();
                                        warn!("router_map_async input_buf_vo no method_id");
                                    }
                                },
                                _ => {
                                    METRICS.messages.dropped_total.inc();
                                    warn!("Unknown constructor_id: {}", constructor_id);
                                },
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("Failed to read from socket: {}", e.to_string());
                    tasks.record_failure();
                    break false;
                },
            }
        }
    };
    let graceful = tokio::select! {
        biased;
        _ = tasks.draining() => true,
        graceful = read => graceful,
        _ = &mut session.writer => { tasks.record_failure(); false },
        _ = session.closed.changed() => { tasks.record_failure(); false },
    };
    if !graceful {
        return;
    }
    let drain = ConnectionSession::finish_requests(process_permit, process_limit);
    let drained = tokio::select! {
        biased;
        _ = tasks.draining() => true,
        _ = drain => true,
        _ = &mut session.writer => { tasks.record_failure(); false },
        _ = session.closed.changed() => { tasks.record_failure(); false },
    };
    if drained {
        session.close_writer().await;
    }
}
