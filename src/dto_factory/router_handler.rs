use std::{collections::HashMap, net::SocketAddr, ops::Deref, sync::Arc};

use bytes::{Buf, Bytes, BytesMut};

use crate::const_config::{
    DEFAULT_MESSAGE_HEADER_MARK, DEFAULT_MESSAGE_TAIL_MARK, SERVER_MESSAGE_HEADER_MARK,
    SERVER_MESSAGE_TAIL_MARK,
};

use super::{AsyncFunc, ClientsStructType, TaskBody};

/// A struct representing the result of a handler.
///
/// This struct contains a boolean indicating whether data should be sent, optional result data, and optional addresses.
#[cfg(any(feature = "server", feature = "client"))]
#[derive(Clone)]
pub struct HandlerResult {
    /// A boolean indicating whether data should be sent.
    is_send: bool,
    /// A boolean indicating whether the message is a heartbeat message.
    is_heart: bool,
    // Optional result data, containing a u64 number and a byte vector.
    result_data: Option<(u16, Bytes)>,
    // Optional vector of socket addresses.
    addrs: Option<Vec<SocketAddr>>,
    /// Optional message header mark, used to identify the start of a message.
    /// The default value is `DEFAULT_MESSAGE_HEADER_MARK`(9177).
    message_header_mark: Option<u16>,
    /// Optional message tail mark, used to identify the end of a message.
    /// The default value is `DEFAULT_MESSAGE_TAIL_MARK`(7719).
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
        response_data: Bytes,
        target_addrs: Vec<SocketAddr>,
    ) -> Self {
        Self {
            is_send: true,
            is_heart: false,
            result_data: Some((method_id, response_data)),
            addrs: Some(target_addrs),
            message_header_mark: None,
            message_tail_mark: None,
        }
    }

    #[cfg(feature = "client")]
    pub fn new_with_send_to_server(method_id: u16, response_data: Bytes) -> Self {
        Self {
            is_send: true,
            is_heart: false,
            result_data: Some((method_id, response_data)),
            addrs: None,
            message_header_mark: None,
            message_tail_mark: None,
        }
    }

    #[cfg(feature = "client")]
    pub(crate) fn new_with_send_heart_to_server() -> Self {
        Self {
            is_send: true,
            is_heart: true,
            result_data: Some((0_u16, Bytes::new())),
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
    #[cfg(any(feature = "server", feature = "client"))]
    pub fn new_without_send() -> Self {
        Self {
            is_send: false,
            is_heart: false,
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
    pub(crate) fn get_response_data(&self) -> Option<Bytes> {
        match self.result_data.clone() {
            Some((method_id, bytes)) => {
                let mut bytes_mut = BytesMut::new();

                if let Some(mark) = self.message_header_mark {
                    bytes_mut.extend_from_slice(&mark.to_be_bytes());
                } else {
                    bytes_mut.extend_from_slice(&DEFAULT_MESSAGE_HEADER_MARK.to_be_bytes());
                }

                let mut msg_len = 0_u64;
                let constructor_id = if self.is_heart {
                    2_u8.to_be_bytes()
                } else {
                    1_u8.to_be_bytes()
                };
                let method_id_bytes = method_id.to_be_bytes();
                let bytes_body_len = bytes.len();
                let msg_tail_len = 2_u64;
                msg_len = constructor_id.len() as u64
                    + method_id_bytes.len() as u64
                    + bytes_body_len as u64
                    + msg_tail_len;

                bytes_mut.extend_from_slice(&msg_len.to_be_bytes());

                bytes_mut.extend_from_slice(&constructor_id);

                bytes_mut.extend_from_slice(&method_id_bytes);

                bytes_mut.extend_from_slice(&bytes);
                if let Some(mark) = self.message_tail_mark {
                    bytes_mut.extend_from_slice(&mark.to_be_bytes());
                } else {
                    bytes_mut.extend_from_slice(&DEFAULT_MESSAGE_TAIL_MARK.to_be_bytes());
                }
                Some(bytes_mut.copy_to_bytes(bytes_mut.len()))
            }
            None => None,
        }
    }
}

/// A trait representing a combined handler method and data.
///
/// This trait combines the IHandlerMethod and IHandlerData traits.
pub(crate) trait IHandlerCombinedTrait: IHandlerMethod {
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
#[inline]
pub(crate) async fn check_handler_result(
    mut handler_result: HandlerResult,
    clients: ClientsStructType,
) {
    tokio::spawn(async move {
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
                let mutex = clients.read().await;
                let guard = mutex.deref();
                if let Some(addrs) = handler_result.get_addrs() {
                    for socket_addr in addrs {
                        if guard.contains_key(&socket_addr) {
                            if let Some(socket) = guard.get(&socket_addr) {
                                socket.send_response(response.clone()).await;
                            }
                        }
                    }
                }
            }
        }
    });
}
