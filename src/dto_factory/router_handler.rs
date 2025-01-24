use std::{collections::HashMap, net::SocketAddr, ops::Deref, sync::Arc};

use crate::{
    app::lynn_user_api::LynnUser,
    const_config::{DEFAULT_MESSAGE_HEADER_MARK, DEFAULT_MESSAGE_TAIL_MARK},
};

use super::{AsyncFunc, ClientsStructType, TaskBody};

/// A struct representing the result of a handler.
///
/// This struct contains a boolean indicating whether data should be sent, optional result data, and optional addresses.
#[cfg(any(feature = "server", feature = "client"))]
#[derive(Clone)]
pub struct HandlerResult {
    // A boolean indicating whether data should be sent.
    is_send: bool,
    // Optional result data, containing a u64 number and a byte vector.
    result_data: Option<(u16, Vec<u8>)>,
    // Optional vector of socket addresses.
    addrs: Option<Vec<SocketAddr>>,
    message_header_mark: Option<u16>,
    message_tail_mark: Option<u16>,
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
    #[cfg(feature = "server")]
    pub fn new_with_send(
        method_id: u16,
        response_data: Vec<u8>,
        target_addrs: Vec<SocketAddr>,
    ) -> Self {
        Self {
            is_send: true,
            result_data: Some((method_id, response_data)),
            addrs: Some(target_addrs),
            message_header_mark: None,
            message_tail_mark: None,
        }
    }

    #[cfg(feature = "client")]
    pub fn new_with_send_to_server(method_id: u16, response_data: Vec<u8>) -> Self {
        Self {
            is_send: true,
            result_data: Some((method_id, response_data)),
            addrs: None,
            message_header_mark: None,
            message_tail_mark: None,
        }
    }

    /// Creates a new HandlerResult instance with send flag set to false, without result data and addresses.
    ///
    /// # Returns
    ///
    /// A new HandlerResult instance.
    #[cfg(feature = "server")]
    pub fn new_without_send() -> Self {
        Self {
            is_send: false,
            result_data: None,
            addrs: None,
            message_header_mark: None,
            message_tail_mark: None,
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

    pub(crate) fn get_addrs(&self) -> Option<Vec<SocketAddr>> {
        self.addrs.clone()
    }

    pub(crate) fn is_with_mark(&self) -> bool {
        if self.message_header_mark.is_some() && self.message_tail_mark.is_some() {
            true
        } else {
            false
        }
    }

    pub(crate) fn set_marks(&mut self, message_header_mark: u16, message_tail_mark: u16) {
        self.message_header_mark = Some(message_header_mark);
        self.message_tail_mark = Some(message_tail_mark);
    }

    /// Gets the response data, converting the u64 number to a big-endian byte slice and inserting it at the beginning of the byte vector.
    ///
    /// # Returns
    ///
    /// The response data as an optional byte vector.
    pub(crate) fn get_response_data(&self) -> Option<Vec<u8>> {
        match self.result_data.clone() {
            Some((method_id, bytes)) => {
                let mut vec = Vec::new();

                if let Some(mark) = self.message_header_mark {
                    vec.extend_from_slice(&mark.to_be_bytes());
                } else {
                    vec.extend_from_slice(&DEFAULT_MESSAGE_HEADER_MARK.to_be_bytes());
                }

                let mut msg_len = 0_u64;
                let constructor_id = 1_u8.to_be_bytes();
                let method_id_bytes = method_id.to_be_bytes();
                let bytes_body_len = bytes.len();
                let msg_tail_len = 2_u64;
                msg_len = constructor_id.len() as u64
                    + method_id_bytes.len() as u64
                    + bytes_body_len as u64
                    + msg_tail_len;

                vec.extend_from_slice(&msg_len.to_be_bytes());

                vec.extend_from_slice(&constructor_id);

                vec.extend_from_slice(&method_id_bytes);

                vec.extend_from_slice(&bytes);
                if let Some(mark) = self.message_tail_mark {
                    vec.extend_from_slice(&mark.to_be_bytes());
                } else {
                    vec.extend_from_slice(&DEFAULT_MESSAGE_TAIL_MARK.to_be_bytes());
                }
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
        clients: ClientsStructType,
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
        clients: ClientsStructType,
    );
}

/// A function for checking and sending a HandlerResult instance.
///
/// This function checks the HandlerResult instance and sends it through a channel if the send flag is set to true.
pub(crate) async fn check_handler_result(
    handler_result: HandlerResult,
    clients: ClientsStructType,
) {
    tokio::spawn(async move {
        // If the send flag of the HandlerResult instance is set to true, send the instance through the channel.
        if handler_result.get_is_send() {
            let mut socket_vec = vec![];
            let response = handler_result.get_response_data();
            {
                let mutex = clients.read().await;
                let guard = mutex.deref();
                if let Some(addrs) = handler_result.get_addrs() {
                    for i in addrs {
                        if guard.contains_key(&i) {
                            socket_vec.push(guard.get(&i).unwrap().sender.clone());
                        }
                    }
                }
            }
            if !socket_vec.is_empty() && response.is_some() {
                for i in socket_vec {
                    let response = handler_result.clone();
                    let _ = i.send(response).await;
                }
            }
        }
    });
}
