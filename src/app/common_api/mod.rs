use std::{
    collections::HashMap,
    net::SocketAddr,
    ops::{Deref, DerefMut},
    sync::Arc,
    time::{Duration, SystemTime},
};

use tokio::{
    io::{split, AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{mpsc, RwLock, Semaphore},
    time::interval,
};
use tracing::{error, info, warn};

use crate::{
    const_config::{DEFAULT_MAX_RECEIVE_BYTES_SIZE, DEFAULT_SYSTEM_CHANNEL_SIZE},
    dto_factory::input_dto_build,
    lynn_tcp_dependents::InputBufVO,
    vo_factory::big_buf::BigBufReader,
};

use super::{lynn_server_user::LynnUser, AsyncFunc, ClientsStructType, TaskBody};

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
            server_check_heart_interval,
            server_check_heart_timeout_time
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
                                warn!("unable to compare time,{}", e)
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

#[inline(always)]
pub(super) fn spawn_socket_server(
    addr: SocketAddr,
    process_permit: Arc<Semaphore>,
    message_header_mark: u16,
    message_tail_mark: u16,
    socket: TcpStream,
    router_map_async: Arc<Option<HashMap<u16, Arc<AsyncFunc>>>>,
    clients: ClientsStructType,
    thread_pool_task_body_sender: TaskBody,
) {
    tokio::spawn(async move {
        // Creates a channel for sending data to the client.
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(DEFAULT_SYSTEM_CHANNEL_SIZE);
        let last_communicate_time = Arc::new(RwLock::new(SystemTime::now()));
        let process_permit_clone = process_permit.clone();
        let last_communicate_time_clone = last_communicate_time.clone();
        // Spawns a new asynchronous task to handle each client connection.
        let clients_clone = clients.clone();
        let join_handle = tokio::spawn(async move {
            let stream = socket;
            let (mut read_half, mut write_half) = split(stream);
            let mut buf = [0; DEFAULT_MAX_RECEIVE_BYTES_SIZE];
            let mut big_buf = BigBufReader::new(message_header_mark, message_tail_mark);
            let addr = addr;
            // Write data to the client in a loop.
            let join_handle = tokio::spawn(async move {
                while let Some(response) = rx.recv().await {
                    if let Err(e) = write_half.write_all(&response).await {
                        error!("Failed to write to socket: {}", e);
                        continue;
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
                            let mut mutex = last_communicate_time.write().await;
                            let guard = mutex.deref_mut();
                            let time_old = guard.clone();
                            match time_old.partial_cmp(&time_now) {
                                Some(std::cmp::Ordering::Less) => {
                                    *guard = time_now;
                                }
                                Some(std::cmp::Ordering::Equal | std::cmp::Ordering::Greater)
                                | None => {}
                            }
                        });
                        big_buf.extend_from_slice(&buf[..n]);
                        while big_buf.is_complete() {
                            let mut input_buf_vo = InputBufVO::new(big_buf.get_data(), addr);
                            if let Some(constructor_id) = input_buf_vo.get_constructor_id() {
                                if constructor_id == 2 {
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
                                                    thread_pool_task_body_sender.clone(),
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
                let mut clients_mutex = clients.write().await;
                let guard = clients_mutex.deref_mut();
                if guard.contains_key(&addr) {
                    guard.remove(&addr);
                }
                join_handle.abort();
            }
        });
        // Saves the client's ID and send channel to the HashMap.
        {
            let mut clients_mutex = clients_clone.write().await;
            let guard = clients_mutex.deref_mut();
            let lynn_user = LynnUser::new(
                tx.clone(),
                process_permit_clone,
                join_handle,
                last_communicate_time_clone,
            );
            guard.insert(addr, lynn_user);
        }
    });
}
