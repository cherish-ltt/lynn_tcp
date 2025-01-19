use input_dto::{IHandlerCombinedTrait, MsgSelect};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::{Mutex, Semaphore};
use tracing::warn;

use crate::{
    app::{lynn_thread_pool_api::LynnThreadPool, lynn_user_api::LynnUser},
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

/// Builds an input DTO from the given parameters.
///
/// This function takes the address, input buffer, process permit, clients, handler method, and thread pool as parameters.
/// It spawns a new task to handle the input DTO based on the constructor ID.
/// If the constructor ID is 1 or 2, it creates a new `MsgSelect` instance and spawns a handler task.
/// If the constructor ID is not 1 or 2, it logs a warning and releases the permit.
///
/// # Parameters
///
/// * `addr` - The address of the client.
/// * `input_buf` - The input buffer containing the data.
/// * `process_permit` - The process permit for handling the input DTO.
/// * `clients` - The map of clients.
/// * `handler_method` - The handler method for processing the input DTO.
/// * `thread_pool` - The thread pool for executing the handler task.
pub(crate) async fn input_dto_build(
    addr: SocketAddr,
    mut input_buf: InputBufVO,
    process_permit: Arc<Semaphore>,
    clients: Arc<Mutex<HashMap<SocketAddr, LynnUser>>>,
    handler_method: Arc<Box<dyn IService>>,
    thread_pool: Arc<Mutex<LynnThreadPool>>,
) {
    match input_buf.get_constructor_id() {
        Some(value) => {
            match value {
                1 | 2 => {
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
                _ => {
                    tokio::spawn(async move {
                        // Attempt to acquire a permit from the semaphore.
                        let result_permit = process_permit.try_acquire();
                        match result_permit {
                            Ok(permit) => {
                                // If the permit is acquired successfully, log a warning indicating that the constructor ID does not match.
                                warn!("input_buf.get_constructor_id is not match");
                                // Release the permit after logging the warning.
                                drop(permit);
                            }
                            Err(_) => {
                                // If the permit cannot be acquired, log a warning.
                                warn!("addr:{} PROCESS_PERMIT_SIZE is full", addr)
                            }
                        }
                    });
                }
            }
        }
        None => {
            warn!("input_buf.get_constructor_id is None")
        }
    };
}

/// Spawns a handler task to process the given `MsgSelect` instance.
///
/// This function takes the `MsgSelect` instance, clients, handler method, and thread pool as parameters.
/// It calls the `execute` method of the `MsgSelect` instance to process the input DTO.
///
/// # Parameters
///
/// * `result` - The `MsgSelect` instance to be processed.
/// * `clients` - The map of clients.
/// * `handler_method` - The handler method for processing the input DTO.
/// * `thread_pool` - The thread pool for executing the handler task.
async fn spawn_handler(
    mut result: MsgSelect,
    clients: Arc<Mutex<HashMap<SocketAddr, LynnUser>>>,
    handler_method: Arc<Box<dyn IService>>,
    thread_pool: Arc<Mutex<LynnThreadPool>>,
) {
    result.execute(clients, handler_method, thread_pool).await;
}
