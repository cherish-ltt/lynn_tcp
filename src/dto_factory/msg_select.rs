use std::{net::SocketAddr, sync::Arc};

use crate::{app::TaskBody, handler::HandlerContext};

use super::{
    input_dto::IHandlerCombinedTrait, router_handler::IHandlerMethod, AsyncFunc, ClientsStructType,
};

/// A struct representing a message selection.
///
/// This struct contains the address from which the message was sent and the input buffer containing the message data.
pub(crate) struct MsgSelect {
    /// The address from which the message was sent.
    pub(crate) addr: SocketAddr,
    /// The input buffer containing the message data.
    pub(crate) handler_context: HandlerContext,
}

impl MsgSelect {
    /// Creates a new `MsgSelect` instance.
    ///
    /// This function takes the address and input buffer as parameters and returns a new `MsgSelect` instance.
    ///
    /// # Parameters
    ///
    /// * `addr` - The address from which the message was sent.
    /// * `input_buf_vo` - The input buffer containing the message data.
    ///
    /// # Returns
    ///
    /// A new `MsgSelect` instance.
    pub(crate) fn new(addr: SocketAddr, handler_context: HandlerContext) -> Self {
        Self {
            addr,
            handler_context,
        }
    }
}

impl IHandlerCombinedTrait for MsgSelect {
    /// Executes the handler for the message selection.
    ///
    /// This function takes the clients, handler method, and thread pool as parameters and executes the handler for the message selection.
    ///
    /// # Parameters
    ///
    /// * `self` - The mutable reference to the `MsgSelect` instance.
    /// * `clients` - The clients map.
    /// * `handler_method` - The handler method.
    /// * `thread_pool` - The thread pool.
    ///
    /// # Returns
    ///
    /// A `Future` that resolves when the handler execution is complete.
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

impl IHandlerMethod for MsgSelect {
    /// Handles the message selection.
    ///
    /// This function takes the handler method, thread pool, and clients as parameters and handles the message selection.
    ///
    /// # Parameters
    ///
    /// * `self` - The mutable reference to the `MsgSelect` instance.
    /// * `handler_method` - The handler method.
    /// * `thread_pool` - The thread pool.
    /// * `clients` - The clients map.
    ///
    /// # Returns
    ///
    /// A `Future` that resolves when the message handling is complete.
    async fn handler(
        &mut self,
        handler_method: Arc<AsyncFunc>,
        thread_pool: TaskBody,
        clients: ClientsStructType,
    ) {
        let task_body = (
            handler_method.clone(),
            self.handler_context.clone(),
            clients,
        );
        let _ = thread_pool.send(task_body).await;
    }
}
