use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use tokio::{
    sync::{
        mpsc::{self, Receiver},
        Mutex, RwLock,
    },
    task::JoinHandle,
};
use tracing::{debug, error, info};

use crate::{
    dto_factory::input_dto::{check_handler_result, HandlerResult},
    vo_factory::input_vo::InputBufVO,
};

use super::{lynn_server_user::LynnUser, AsyncFunc, TaskBody, DEFAULT_SYSTEM_CHANNEL_SIZE};

/// A thread pool for handling tasks concurrently.
pub(crate) struct LynnServerThreadPool {
    /// A vector of tuples containing the task sender and the join handle for each thread.
    threads: Vec<(
        mpsc::Sender<(
            Arc<AsyncFunc>,
            InputBufVO,
            Arc<RwLock<HashMap<SocketAddr, LynnUser>>>,
        )>,
        JoinHandle<()>,
    )>,
    pub(crate) task_body_sender: mpsc::Sender<TaskBody>,
    /// An index used for load balancing when submitting tasks.
    index: usize,
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
    pub(crate) async fn new(num_threads: &usize) -> Self {
        let mut threads = Vec::with_capacity(*num_threads);
        let (tx_result, rx_result) = mpsc::channel::<(
            HandlerResult,
            Arc<RwLock<HashMap<SocketAddr, LynnUser>>>,
        )>(*num_threads * DEFAULT_SYSTEM_CHANNEL_SIZE);
        let (task_body_sender, task_body_rx) =
            mpsc::channel::<TaskBody>(*num_threads * DEFAULT_SYSTEM_CHANNEL_SIZE);
        let mut thread_task_body_rx_vec = Vec::new();
        for i in 1..=*num_threads {
            let tx_result = tx_result.clone();
            let (tx, mut rx) = mpsc::channel::<(
                Arc<AsyncFunc>,
                InputBufVO,
                Arc<RwLock<HashMap<SocketAddr, LynnUser>>>,
            )>(DEFAULT_SYSTEM_CHANNEL_SIZE);
            let handle = tokio::spawn(async move {
                info!("Server - [thread-{}] is listening success!!!", i);
                loop {
                    if let Some((task, input_buf_vo, clients)) = rx.recv().await {
                        let result = task(input_buf_vo).await;
                        let _ = tx_result.send((result, clients)).await;
                    }
                }
            });
            threads.push((tx.clone(), handle));
            thread_task_body_rx_vec.push(tx);
        }

        let join_handle = tokio::spawn(async move {
            let mut index = 0;
            let thread_len = thread_task_body_rx_vec.len();
            let mut task_body_rx = task_body_rx;
            loop {
                if let Some(task_body) = task_body_rx.recv().await {
                    let thread_index = index % thread_len;
                    index += 1;
                    if let Some(tx) = thread_task_body_rx_vec.get_mut(thread_index) {
                        tx.send(task_body).await.unwrap_or_else(|e| {
                            error!("send task to thread-{} err: {}", thread_index, e);
                        });
                    }
                    if index >= thread_len {
                        index = 0;
                    }
                }
            }
        });
        let lynn_thread_pool = LynnServerThreadPool {
            threads,
            index: 0,
            task_body_sender,
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
    #[deprecated(since = "v1.0.0", note = "No need to manually 'submit'")]
    pub(crate) async fn submit(
        &mut self,
        task_body: (
            Arc<AsyncFunc>,
            InputBufVO,
            Arc<RwLock<HashMap<SocketAddr, LynnUser>>>,
        ),
    ) {
        let mut idx = self.index;
        let thread_index = idx % self.threads.len();
        idx += 1;
        if let Some((tx, _)) = self.threads.get_mut(thread_index) {
            tx.send(task_body).await.unwrap_or_else(|e| {
                error!("send task to thread-{} err: {}", thread_index, e);
            });
        }
        if idx >= self.threads.len() {
            self.index = 0;
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
        mut rx: Receiver<(HandlerResult, Arc<RwLock<HashMap<SocketAddr, LynnUser>>>)>,
    ) -> Self {
        tokio::spawn(async move {
            info!("Server - [thread-result-listening] is listening success!!!");
            loop {
                if let Some((result, clients)) = rx.recv().await {
                    check_handler_result(result, clients).await;
                    //debug!("发送开始");
                }
            }
        });
        self
    }
}
