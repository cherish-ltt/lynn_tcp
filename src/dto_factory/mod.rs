use input_dto::{IHandlerCombinedTrait, MsgSelect};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, warn};

use crate::{
    app::{lynn_thread_pool_api::LynnServerThreadPool, lynn_user_api::LynnUser, AsyncFunc},
    service::IService,
    vo_factory::{input_vo::InputBufVO, InputBufVOTrait},
};

mod msg_select;
mod router_handler;

pub mod input_dto {
    pub(crate) use super::msg_select::*;
    pub(crate) use super::router_handler::check_handler_result;
    pub(crate) use super::router_handler::HandlerData;
    pub use super::router_handler::HandlerResult;
    pub(crate) use super::router_handler::IHandlerCombinedTrait;
}

pub(crate) async fn input_dto_build(
    addr: SocketAddr,
    mut input_buf: InputBufVO,
    process_permit: Arc<Semaphore>,
    clients: Arc<Mutex<HashMap<SocketAddr, LynnUser>>>,
    handler_method: Arc<AsyncFunc>,
    thread_pool: Arc<Mutex<LynnServerThreadPool>>,
) {
    tokio::spawn(async move {
        // Attempt to acquire a permit from the semaphore.
        let result_permit = process_permit.try_acquire();
        match result_permit {
            Ok(permit) => {
                // If the permit is acquired successfully, create a new `MsgSelect` instance and spawn a handler task.
                let result = MsgSelect::new(addr, input_buf);
                spawn_handler(result, clients, handler_method, thread_pool).await;
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

async fn spawn_handler(
    mut result: MsgSelect,
    clients: Arc<Mutex<HashMap<SocketAddr, LynnUser>>>,
    handler_method: Arc<AsyncFunc>,
    thread_pool: Arc<Mutex<LynnServerThreadPool>>,
) {
    result.execute(clients, handler_method, thread_pool).await;
}