use std::{collections::HashMap, net::SocketAddr, ops::Deref, sync::Arc};

use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::debug;

use crate::app::{lynn_thread_pool_api::LynnServerThreadPool, lynn_user_api::LynnUser};

use super::{AsyncFunc, TaskBody};

/// A struct representing the result of a handler.
///
/// This struct contains a boolean indicating whether data should be sent, optional result data, and optional addresses.
#[cfg(any(feature = "server", feature = "client"))]
pub struct HandlerResult {
    // A boolean indicating whether data should be sent.
    is_send: bool,
    // Optional result data, containing a u64 number and a byte vector.
    result_data: Option<(u16, Vec<u8>)>,
    // Optional vector of socket addresses.
    addrs: Option<Vec<SocketAddr>>,
}

impl HandlerResult {
    /// Creates a new HandlerResult instance with send flag set to true, containing result data and addresses.
    ///
    /// # Parameters
    ///
    /// * `result_data`: The result data as a tuple of a u64 number and a byte vector.
    /// * `addrs`: The vector of socket addresses.
    ///
    /// # Returns
    ///
    /// A new HandlerResult instance.
    pub fn new_with_send(
        method_id: u16,
        response_data: Vec<u8>,
        target_addrs: Vec<SocketAddr>,
    ) -> Self {
        Self {
            is_send: true,
            result_data: Some((method_id, response_data)),
            addrs: Some(target_addrs),
        }
    }

    /// Creates a new HandlerResult instance with send flag set to false, without result data and addresses.
    ///
    /// # Returns
    ///
    /// A new HandlerResult instance.
    pub fn new_without_send() -> Self {
        Self {
            is_send: false,
            result_data: None,
            addrs: None,
        }
    }

    /// Gets the value of the send flag.
    ///
    /// # Returns
    ///
    /// The value of the send flag.
    pub(crate) fn get_is_send(&self) -> bool {
        self.is_send
    }

    /// Gets the response data, converting the u64 number to a big-endian byte slice and inserting it at the beginning of the byte vector.
    ///
    /// # Returns
    ///
    /// The response data as an optional byte vector.
    pub(crate) fn get_response_data(&self) -> Option<Vec<u8>> {
        match self.result_data.clone() {
            Some((num, bytes)) => {
                let mut vec = Vec::new();
                // Convert the u64 to a big-endian (network byte order) byte slice.
                let num_bytes = num.to_be_bytes();
                vec.extend_from_slice(&num_bytes);
                // Insert num_bytes at the beginning of the byte vector.
                vec.extend_from_slice(&bytes);
                Some(vec)
            }
            None => None,
        }
    }
}

/// A struct representing the data for a handler.
///
/// This struct contains an optional HashMap with u64 keys and byte vector values.
pub(crate) struct HandlerData {
    // Optional HashMap with u64 keys and byte vector values.
    pub(crate) data_map: Option<HashMap<u64, Vec<u8>>>,
}

/// Implementation of methods for the HandlerData struct.
impl HandlerData {
    /// Creates a new HandlerData instance with a data map containing the provided data.
    ///
    /// # Parameters
    ///
    /// * `data`: The data as a HashMap with u64 keys and byte vector values.
    ///
    /// # Returns
    ///
    /// A new HandlerData instance.
    pub(crate) fn new_with_data(data: HashMap<u64, Vec<u8>>) -> Self {
        Self {
            data_map: Some(data),
        }
    }

    /// Creates a new HandlerData instance without a data map.
    ///
    /// # Returns
    ///
    /// A new HandlerData instance.
    pub(crate) fn new_without_data() -> Self {
        Self { data_map: None }
    }
}

/// A trait representing a combined handler method and data.
///
/// This trait combines the IHandlerMethod and IHandlerData traits.
pub(crate) trait IHandlerCombinedTrait: IHandlerMethod + IHandlerData {
    /// Executes the handler logic and sends a HandlerResult instance through a channel.
    ///
    /// # Parameters
    ///
    /// * `tx`: The channel for sending the HandlerResult instance.
    async fn execute(
        &mut self,
        clients: Arc<RwLock<HashMap<SocketAddr, LynnUser>>>,
        handler_method: Arc<AsyncFunc>,
        thread_pool: TaskBody,
    ) {
        // Business logic
        self.handler(handler_method, thread_pool, clients).await;
    }
}

/// A trait representing the data for a handler.
///
/// This trait provides methods for getting the handler data and method ID.
pub(crate) trait IHandlerData {
    /// Gets the handler data.
    ///
    /// # Returns
    ///
    /// The handler data as a HandlerData instance.
    fn get_data(&self) -> HandlerData;
}

/// A trait representing a handler method.
///
/// This trait provides a method for handling data and returning a HandlerResult instance.
pub(crate) trait IHandlerMethod {
    /// Handles the data and returns a HandlerResult instance.
    ///
    /// # Parameters
    ///
    /// * `handler_method`: The handler method as a boxed trait object.
    /// * `thread_pool`: The thread pool as a mutex-protected instance.
    /// * `clients`: The clients as a mutex-protected HashMap.
    async fn handler(
        &mut self,
        handler_method: Arc<AsyncFunc>,
        thread_pool: TaskBody,
        clients: std::sync::Arc<
            tokio::sync::RwLock<std::collections::HashMap<SocketAddr, LynnUser>>,
        >,
    );
}

/// A function for checking and sending a HandlerResult instance.
///
/// This function checks the HandlerResult instance and sends it through a channel if the send flag is set to true.
pub(crate) async fn check_handler_result(
    handler_result: HandlerResult,
    clients: Arc<RwLock<HashMap<SocketAddr, LynnUser>>>,
) {
    tokio::spawn(async move {
        // If the send flag of the HandlerResult instance is set to true, send the instance through the channel.
        if handler_result.get_is_send() {
            let mut socket_vec = vec![];
            let response = handler_result.get_response_data();
            {
                let mutex = clients.read().await;
                let guard = mutex.deref();
                if let Some(addrs) = handler_result.addrs {
                    for i in addrs {
                        if guard.contains_key(&i) {
                            socket_vec.push(guard.get(&i).unwrap().sender.clone());
                        }
                    }
                }
            }
            if !socket_vec.is_empty() && response.is_some() {
                for i in socket_vec {
                    let response = response.clone().unwrap();
                    let _ = i.send(response).await;
                }
            }
        }
    });
}
