use std::{
    collections::HashMap,
    net::SocketAddr,
    ops::{Deref, DerefMut},
    sync::Arc,
    time::{Duration, SystemTime},
};

use bytes::Bytes;
use tokio::{
    io::{AsyncReadExt, ReadHalf, split},
    net::TcpStream,
    sync::{RwLock, Semaphore},
    time::interval,
};
use tracing::{error, info, warn};

use crate::{
    app::ReactorEventSender,
    const_config::{
        DEFAULT_MAX_RECEIVE_BYTES_SIZE, DEFAULT_MESSAGE_HEADER_MARK, DEFAULT_MESSAGE_TAIL_MARK,
        SERVER_MESSAGE_HEADER_MARK, SERVER_MESSAGE_TAIL_MARK,
    },
    dto_factory::input_dto::{IHandlerCombinedTrait, MsgSelect},
    handler::{ClientsContext, HandlerContext},
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
    vo_factory::big_buf::BigBufReader,
};

use super::{AsyncFunc, ClientsStruct, ClientsStructType, lynn_server_user::LynnUser};

use crate::vo_factory::InputBufVOTrait;

#[inline(always)]
pub(super) fn spawn_check_heart(
    server_check_heart_interval: u64,
    server_check_heart_timeout_time: u64,
    clients: ClientsStructType,
) {
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(server_check_heart_interval));
        info!(
            "Server - [check heart] start sucess!!! with [server_check_heart_interval:{}s] [server_check_heart_timeout_time:{}s]",
            server_check_heart_interval, server_check_heart_timeout_time
        );
        loop {
            interval.tick().await;
            let mut remove_list = vec![];
            {
                let clients_mutex = clients.read().await;
                let guard = clients_mutex.deref();
                for (addr, lynn_user) in guard.iter() {
                    let last_communicate_time = lynn_user.get_last_communicate_time();
                    let last_communicate_time = last_communicate_time.read().await;
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
                                warn!("unable to compare time,{}", e.to_string())
                            }
                        },
                        Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater) | None => {}
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

/// A function for checking and sending a HandlerResult instance.
///
/// This function checks the HandlerResult instance and sends it through a channel if the send flag is set to true.
#[inline(always)]
pub(crate) async fn check_handler_result(
    mut handler_result: HandlerResult,
    clients: ClientsStructType,
) {
    // If the send flag of the HandlerResult instance is set to true, send the instance through the channel.
    if handler_result.get_is_send() {
        let response = handler_result.get_response_data();
        if response.is_some() && handler_result.get_addrs().is_some() {
            if !handler_result.is_with_mark() {
                handler_result.set_marks(
                    *SERVER_MESSAGE_HEADER_MARK
                        .get()
                        .unwrap_or(&DEFAULT_MESSAGE_HEADER_MARK),
                    *SERVER_MESSAGE_TAIL_MARK
                        .get()
                        .unwrap_or(&DEFAULT_MESSAGE_TAIL_MARK),
                );
            }
            let response = response.unwrap();
            {
                if let Some(addrs) = handler_result.get_addrs() {
                    if let Some(delay_socket) = send_response(&response, &addrs, &clients).await {
                        if let Some(delay_socket) =
                            send_response(&response, &delay_socket, &clients).await
                        {
                            delay_socket.iter().for_each(|addr|{
                                    warn!("Failed to find the client correctly, message sending is invalid , target-addr:{}",addr);
                                });
                        }
                    }
                }
            }
        }
    }
}

#[inline(always)]
async fn send_response(
    response: &Bytes,
    addrs: &Vec<SocketAddr>,
    clients: &ClientsStructType,
) -> Option<Vec<SocketAddr>> {
    {
        let mut delay_socket = Vec::new();
        let mutex = clients.read().await;
        let guard = mutex.deref();
        for socket_addr in addrs {
            if guard.contains_key(socket_addr) {
                if let Some(socket) = guard.get(socket_addr) {
                    socket.send_response(response).await;
                }
            } else {
                delay_socket.push(socket_addr.clone());
            }
        }
        if !delay_socket.is_empty() {
            Some(delay_socket)
        } else {
            None
        }
    }
}

#[inline(always)]
pub(crate) async fn input_dto_build(
    addr: SocketAddr,
    input_buf_vo: InputBufVO,
    process_permit: Arc<Semaphore>,
    clients: ClientsStructType,
    handler_method: Arc<AsyncFunc>,
    reactor_event_sender: ReactorEventSender,
) {
    tokio::spawn(async move {
        // Attempt to acquire a permit from the semaphore.
        let result_permit = process_permit.try_acquire();
        match result_permit {
            Ok(permit) => {
                // If the permit is acquired successfully, create a new `MsgSelect` instance and spawn a handler task.
                let result = MsgSelect::new(
                    addr,
                    HandlerContext::new(
                        input_buf_vo,
                        ClientsContext::new(ClientsStruct(clients.clone())),
                    ),
                );
                spawn_handler(result, clients, handler_method, reactor_event_sender).await;
                // Release the permit after the handler task is completed.
                drop(permit);
            }
            Err(_) => {
                // If the permit cannot be acquired, log a warning.
                warn!("addr:{} PROCESS_PERMIT_SIZE is full", addr)
            }
        }
    });
}

#[inline(always)]
async fn spawn_handler(
    mut result: MsgSelect,
    clients: ClientsStructType,
    handler_method: Arc<AsyncFunc>,
    reactor_event_sender: ReactorEventSender,
) {
    result
        .execute(clients, handler_method, reactor_event_sender)
        .await;
}

#[inline(always)]
pub(crate) async fn add_client(
    clients: ClientsStructType,
    socket: TcpStream,
    addr: SocketAddr,
    last_communicate_time: Arc<RwLock<SystemTime>>,
) -> ReadHalf<TcpStream> {
    let mut clients_mutex = clients.write().await;
    let guard = clients_mutex.deref_mut();
    let (read_half, write_half) = split(socket);
    let lynn_user = LynnUser::new(write_half, last_communicate_time);
    guard.insert(addr, lynn_user);
    read_half
}

#[inline(always)]
pub(crate) async fn push_read_half(
    mut read_half: ReadHalf<TcpStream>,
    process_permit: Arc<Semaphore>,
    addr: SocketAddr,
    clients: ClientsStructType,
    message_header_mark: u16,
    message_tail_mark: u16,
    router_map_async: Arc<Option<HashMap<u16, Arc<AsyncFunc>>>>,
    reactor_event_sender: ReactorEventSender,
    last_communicate_time: Arc<RwLock<SystemTime>>,
) {
    tokio::spawn(async move {
        let mut buf = [0; DEFAULT_MAX_RECEIVE_BYTES_SIZE];
        let mut big_buf = BigBufReader::new(message_header_mark, message_tail_mark);
        loop {
            let result = read_half.read(&mut buf).await;
            match result {
                Ok(n) if n <= 0 => break,
                Ok(n) => {
                    big_buf.extend_from_slice(&buf[..n]);
                    while big_buf.is_complete() {
                        let mut input_buf_vo = InputBufVO::new(big_buf.get_data(), addr);
                        if let Some(constructor_id) = input_buf_vo.get_constructor_id() {
                            if constructor_id == 2 {
                                let last_communicate_time = last_communicate_time.clone();
                                tokio::spawn(async move {
                                    let time_now = SystemTime::now();
                                    let mut mutex = last_communicate_time.write().await;
                                    let guard = mutex.deref_mut();
                                    let time_old = guard.clone();
                                    match time_old.partial_cmp(&time_now) {
                                        Some(std::cmp::Ordering::Less) => {
                                            *guard = time_now;
                                        }
                                        Some(
                                            std::cmp::Ordering::Equal | std::cmp::Ordering::Greater,
                                        )
                                        | None => {}
                                    }
                                });
                                continue;
                            } else if constructor_id == 1 {
                                if let Some(method_id) = input_buf_vo.get_method_id() {
                                    let guard = router_map_async.deref();
                                    if let Some(map) = guard {
                                        if map.contains_key(&method_id) {
                                            let a = map.get(&method_id).unwrap();
                                            input_dto_build(
                                                addr,
                                                input_buf_vo,
                                                process_permit.clone(),
                                                clients.clone(),
                                                a.clone(),
                                                reactor_event_sender.clone(),
                                            )
                                            .await;
                                        } else {
                                            warn!("router_map_async no method match,{}", method_id);
                                        }
                                    } else {
                                        warn!("server router is none");
                                    }
                                } else {
                                    warn!("router_map_async input_buf_vo no method_id");
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to read from socket: {}", e.to_string());
                    break;
                }
            }
        }
    });
}
