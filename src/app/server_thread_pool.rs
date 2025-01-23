use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use tokio::{
    sync::{
        mpsc::{self, Receiver},
        Mutex,
    },
    task::JoinHandle,
};
use tracing::{error, info};

use crate::{
    dto_factory::input_dto::{check_handler_result, HandlerResult},
    vo_factory::input_vo::InputBufVO,
};

use super::{lynn_server_user::LynnUser, AsyncFunc};

/// A thread pool for handling tasks concurrently.
pub(crate) struct LynnServerThreadPool {
    /// A vector of tuples containing the task sender and the join handle for each thread.
    threads: Vec<(
        mpsc::Sender<(
            Arc<AsyncFunc>,
            InputBufVO,
            Arc<Mutex<HashMap<SocketAddr, LynnUser>>>,
        )>,
        JoinHandle<()>,
    )>,
    /// An index used for load balancing when submitting tasks.
    index: Arc<Mutex<usize>>,
}

impl LynnServerThreadPool {
    /// Creates a new instance of `LynnServerThreadPool`.
    ///
    /// # Parameters
    ///
    /// * `num_threads` - The number of threads in the pool.
    /// * `server_single_processs_permit` - The maximum number of tasks that can be processed concurrently by each thread.
    ///
    /// # Returns
    ///
    /// A new instance of `LynnServerThreadPool`.
    pub(crate) async fn new(num_threads: &usize, server_single_processs_permit: &usize) -> Self {
        let mut threads = Vec::with_capacity(*num_threads);
        let (tx_result, rx_result) = mpsc::channel::<(
            HandlerResult,
            Arc<Mutex<HashMap<SocketAddr, LynnUser>>>,
        )>(*num_threads);
        for i in 1..=*num_threads {
            let tx_result = tx_result.clone();
            let (tx, mut rx) = mpsc::channel::<(
                Arc<AsyncFunc>,
                InputBufVO,
                Arc<Mutex<HashMap<SocketAddr, LynnUser>>>,
            )>(*server_single_processs_permit);
            let handle = tokio::spawn(async move {
                info!("Server - [thread-{}] is listening success!!!", i);
                loop {
                    if let Some((task, input_buf_vo, clients)) = rx.recv().await {
                        let result = task(input_buf_vo).await;
                        let _ = tx_result.send((result, clients)).await;
                    }
                }
            });
            threads.push((tx, handle));
        }
        let lynn_thread_pool = LynnServerThreadPool {
            threads,
            index: Arc::new(Mutex::new(0)),
        }
        .spawn_handler_result(rx_result)
        .await;
        lynn_thread_pool
    }

    /// Submits a task to the thread pool for execution.
    ///
    /// # Parameters
    ///
    /// * `task_body` - A tuple containing the service, input buffer, and client map.
    pub(crate) async fn submit(
        &mut self,
        task_body: (
            Arc<AsyncFunc>,
            InputBufVO,
            Arc<Mutex<HashMap<SocketAddr, LynnUser>>>,
        ),
    ) {
        let mut idx = self.index.lock().await;
        let thread_index = *idx % self.threads.len();
        *idx += 1;
        if let Some((tx, _)) = self.threads.get_mut(thread_index) {
            tx.send(task_body).await.unwrap_or_else(|e| {
                error!("send task to thread-{} err: {}", thread_index, e);
            });
        }
        if *idx >= self.threads.len() {
            *idx = 0;
        }
    }

    /// Spawns a new task to handle the results of completed tasks.
    ///
    /// # Parameters
    ///
    /// * `self` - The current instance of `LynnServerThreadPool`.
    /// * `rx` - A receiver for the results of completed tasks.
    ///
    /// # Returns
    ///
    /// The modified `LynnServerThreadPool` instance.
    pub(crate) async fn spawn_handler_result(
        self,
        mut rx: Receiver<(HandlerResult, Arc<Mutex<HashMap<SocketAddr, LynnUser>>>)>,
    ) -> Self {
        tokio::spawn(async move {
            info!("Server - [thread-result-listening] is listening success!!!");
            loop {
                if let Some((result, clients)) = rx.recv().await {
                    check_handler_result(result, clients).await;
                }
            }
        });
        self
    }
}
