use std::{net::SocketAddr, ops::DerefMut, sync::Arc};

use tokio::sync::Mutex;

use crate::{
    app::{lynn_thread_pool_api::LynnThreadPool, lynn_user_api::LynnUser},
    service::IService,
};

use super::{
    input_dto::IHandlerCombinedTrait,
    input_vo::InputBufVO,
    router_handler::{HandlerData, IHandlerData, IHandlerMethod},
};

/// A struct representing a message selection.
///
/// This struct contains the address from which the message was sent and the input buffer containing the message data.
pub(crate) struct MsgSelect {
    /// The address from which the message was sent.
    pub(crate) addr: SocketAddr,
    /// The input buffer containing the message data.
    pub(crate) input_buf_vo: InputBufVO,
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
    pub(crate) fn new(addr: SocketAddr, input_buf_vo: InputBufVO) -> Self {
        Self { addr, input_buf_vo }
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
        clients: std::sync::Arc<
            tokio::sync::Mutex<std::collections::HashMap<SocketAddr, LynnUser>>,
        >,
        handler_method: Arc<Box<dyn IService>>,
        thread_pool: Arc<Mutex<LynnThreadPool>>,
    ) {
        // Business logic
        self.handler(handler_method, thread_pool, clients).await;
        // Post-proxy
        //check_handler_result(handler_result, clients).await;
    }
}

impl IHandlerData for MsgSelect {
    /// Gets the handler data for the message selection.
    ///
    /// This function returns the handler data for the message selection.
    ///
    /// # Returns
    ///
    /// The handler data for the message selection.
    fn get_data(&self) -> super::router_handler::HandlerData {
        HandlerData::new_without_data()
    }

    /// Gets the method ID for the message selection.
    ///
    /// This function returns the method ID for the message selection.
    ///
    /// # Returns
    ///
    /// The method ID for the message selection.
    fn get_method_id(&self) -> u64 {
        self.input_buf_vo.get_method_id()
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
        handler_method: Arc<Box<dyn IService>>,
        thread_pool: Arc<Mutex<LynnThreadPool>>,
        clients: std::sync::Arc<
            tokio::sync::Mutex<std::collections::HashMap<SocketAddr, LynnUser>>,
        >,
    ) {
        let mut thread_pool_mutex = thread_pool.lock().await;
        let thread_pool_guard = thread_pool_mutex.deref_mut();
        let task_body = (handler_method.clone(), self.input_buf_vo.clone(), clients);
        thread_pool_guard.submit(task_body).await;
        //handler_method.deref().service(&mut self.input_buf_vo)
    }
}