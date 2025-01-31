use std::sync::Arc;

use tokio::{
    sync::mpsc::{self, Receiver},
    task::JoinHandle,
};
use tracing::{error, info};

use crate::{
    app::common_api::check_handler_result, const_config::DEFAULT_SYSTEM_CHANNEL_SIZE,
    dto_factory::input_dto::HandlerResult, handler::HandlerContext,
};

use super::{AsyncFunc, ClientsStructType, TaskBody};

type Threads = Vec<(
    mpsc::Sender<(Arc<AsyncFunc>, HandlerContext, ClientsStructType)>,
    JoinHandle<()>,
)>;

struct ThreadsStruct(Threads);
pub(super) struct TaskBodyStruct(pub(super) TaskBody);

type TaskBodyOutChannel = (Arc<AsyncFunc>, HandlerContext, ClientsStructType);
/// A thread pool for handling tasks concurrently.
pub(crate) struct LynnServerThreadPool {
    /// A vector of tuples containing the task sender and the join handle for each thread.
    threads: ThreadsStruct,
    pub(crate) task_body_sender: TaskBodyStruct,
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
        let (tx_result, rx_result) = mpsc::channel::<(HandlerResult, ClientsStructType)>(
            *num_threads * DEFAULT_SYSTEM_CHANNEL_SIZE,
        );
        let (task_body_sender, task_body_rx) =
            mpsc::channel::<TaskBodyOutChannel>(*num_threads * DEFAULT_SYSTEM_CHANNEL_SIZE);
        let mut thread_task_body_rx_vec = Vec::new();
        for i in 1..=*num_threads {
            let tx_result = tx_result.clone();
            let (tx, mut rx) = mpsc::channel::<(Arc<AsyncFunc>, HandlerContext, ClientsStructType)>(
                DEFAULT_SYSTEM_CHANNEL_SIZE,
            );
            let handle = tokio::spawn(async move {
                info!("Server - [thread-{}] is listening success!!!", i);
                loop {
                    if let Some((task, context, clients)) = rx.recv().await {
                        //let result = task(input_buf_vo).await;
                        let result = task.handler(context).await;
                        if let Err(e) = tx_result.send((result, clients)).await {
                            error!("Failed to send result to result channel: {}", e);
                        }
                    }
                }
            });
            threads.push((tx.clone(), handle));
            thread_task_body_rx_vec.push(tx);
        }

        tokio::spawn(async move {
            let mut index = 0;
            let thread_len = thread_task_body_rx_vec.len();
            let mut task_body_rx = task_body_rx;
            loop {
                if let Some(task_body) = task_body_rx.recv().await {
                    let thread_index = index % thread_len;
                    index += 1;
                    if let Some(tx) = thread_task_body_rx_vec.get_mut(thread_index) {
                        if let Err(e) = tx.send(task_body).await {
                            error!("Failed to send task to thread-{}: {}", thread_index, e);
                        }
                    }
                    if index >= thread_len {
                        index = 0;
                    }
                }
            }
        });
        let lynn_thread_pool = LynnServerThreadPool {
            threads: ThreadsStruct(threads),
            index: 0,
            task_body_sender: TaskBodyStruct(task_body_sender),
        }
        .spawn_handler_result(rx_result)
        .await;
        lynn_thread_pool
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
        mut rx: Receiver<(HandlerResult, ClientsStructType)>,
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
