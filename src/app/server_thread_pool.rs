use std::{panic, sync::Arc, time::Duration};

use crossbeam_deque::{Injector, Steal, Stealer, Worker};
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

type Threads = Vec<(JoinHandle<()>)>;

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
        // Work theft queue
        let global_queue: Arc<Injector<TaskBodyOutChannel>> = Arc::new(Injector::new());
        let mut local_queues: Vec<Worker<TaskBodyOutChannel>> = Vec::new();
        let mut stealers = Vec::new();
        // Create workers and associate them with stealers
        for _ in 0..*num_threads {
            let worker = Worker::new_fifo();
            stealers.push(worker.stealer());
            local_queues.push(worker);
        }

        let mut threads: Vec<JoinHandle<()>> = Vec::with_capacity(*num_threads);
        let (tx_result, rx_result) = mpsc::channel::<(HandlerResult, ClientsStructType)>(
            *num_threads * DEFAULT_SYSTEM_CHANNEL_SIZE,
        );

        let stealers_arc = Arc::new(stealers);
        for i in 1..=*num_threads {
            let tx_result = tx_result.clone();
            let global_queue_clone = global_queue.clone();
            let local_queues_clone = local_queues.pop().unwrap();
            let stealers_clone = stealers_arc.clone();

            let handle = tokio::spawn(async move {
                info!("Server - [thread-{}] is listening success!!!", i);
                loop {
                    if let Some(result) =
                        get_task(&local_queues_clone, &global_queue_clone, &stealers_clone)
                    {
                        let (task, context, clients) = result;
                        let result = task.handler(context).await;
                        if let Err(e) = tx_result.send((result, clients)).await {
                            error!("Failed to send result to result channel: {}", e);
                        }
                    } else {
                        // 退避
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            });
            threads.push(handle);
        }

        let lynn_thread_pool = LynnServerThreadPool {
            threads: ThreadsStruct(threads),
            index: 0,
            task_body_sender: TaskBodyStruct(global_queue.clone()),
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
                if !rx.is_closed() {
                    if let Some((result, clients)) = rx.recv().await {
                        check_handler_result(result, clients).await;
                    }
                } else {
                    break;
                }
            }
        });
        self
    }
}

#[inline(always)]
fn get_task(
    local: &Worker<TaskBodyOutChannel>,
    global: &Injector<TaskBodyOutChannel>,
    stealers: &[Stealer<TaskBodyOutChannel>],
) -> Option<TaskBodyOutChannel> {
    // 1. local
    if let Some(task) = local.pop() {
        return Some(task);
    }
    // 2. global
    if let Steal::Success(task) = global.steal_batch_and_pop(local) {
        return Some(task);
    }
    // 3. stealers
    for i in 0..stealers.len() {
        if let Steal::Success(task) = stealers[i].steal() {
            return Some(task);
        }
    }
    None
}
