use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime},
};

use crossbeam_deque::{Injector, Steal, Stealer, Worker};
use tokio::{
    net::TcpStream,
    sync::{RwLock, Semaphore},
};

use crate::app::{
    AsyncFunc, ClientsStructType, ReactorEventSender, TaskBodyOutChannel,
    common_api::{add_client, check_handler_result},
    tcp_reactor::NewSocketEventSender,
};

enum EventType {
    NewSocket((TcpStream, core::net::SocketAddr)),
    ExcuteTask(TaskBodyOutChannel),
}

pub(crate) struct ReactorEvent {
    event_type: EventType,
}

impl ReactorEvent {
    fn new_with_event_type(event_type: EventType) -> Self {
        Self { event_type }
    }
    pub(crate) fn crate_new_socket_event(socket: TcpStream, addr: core::net::SocketAddr) -> Self {
        ReactorEvent::new_with_event_type(EventType::NewSocket((socket, addr)))
    }

    pub(crate) fn crate_excute_task_event(task_body: TaskBodyOutChannel) -> Self {
        ReactorEvent::new_with_event_type(EventType::ExcuteTask(task_body))
    }
}

pub(crate) struct EventManager {
    global_queue: ReactorEventSender,
}

impl EventManager {
    pub(crate) fn new() -> Self {
        let global_queue = Arc::new(Injector::<ReactorEvent>::new());
        EventManager { global_queue }
    }

    pub(crate) fn run(
        &self,
        clients: ClientsStructType,
        server_single_processs_permit: &usize,
        message_header_mark: u16,
        message_tail_mark: u16,
        router_map_async: Arc<Option<HashMap<u16, Arc<AsyncFunc>>>>,
        reactor_event_sender: ReactorEventSender,
        tx: NewSocketEventSender,
        server_max_reactor_taskpool_size: &usize,
    ) {
        let mut local_queues: Vec<Worker<ReactorEvent>> =
            Vec::with_capacity(*server_max_reactor_taskpool_size);
        let mut stealers: Vec<Stealer<ReactorEvent>> =
            Vec::with_capacity(*server_max_reactor_taskpool_size);
        for _ in 0..*server_max_reactor_taskpool_size {
            let worker = Worker::new_lifo();
            stealers.push(worker.stealer());
            local_queues.push(worker);
        }
        let global_queue = self.global_queue.clone();
        let stealers_arc = Arc::new(stealers);
        for local_queue in local_queues {
            let global_queue_clone = global_queue.clone();
            let stealers_arc_clone = stealers_arc.clone();
            let clients_clone = clients.clone();
            let server_single_processs_permit = server_single_processs_permit.clone();
            let router_map_async = router_map_async.clone();
            let reactor_event_sender = reactor_event_sender.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let local_queue = local_queue;
                let global_queue = global_queue_clone;
                let stealers_arc = stealers_arc_clone;
                let clients = clients_clone;
                loop {
                    if let Some(event) = get_event(&local_queue, &global_queue, &stealers_arc) {
                        match event.event_type {
                            EventType::NewSocket((socket, addr)) => {
                                let last_communicate_time =
                                    Arc::new(RwLock::new(SystemTime::now()));
                                let read_half = add_client(
                                    clients.clone(),
                                    socket,
                                    addr,
                                    last_communicate_time.clone(),
                                )
                                .await;
                                let process_permit =
                                    Arc::new(Semaphore::new(server_single_processs_permit));
                                let _ = tx
                                    .send((
                                        read_half,
                                        process_permit,
                                        addr,
                                        clients.clone(),
                                        message_header_mark,
                                        message_tail_mark,
                                        router_map_async.clone(),
                                        reactor_event_sender.clone(),
                                        last_communicate_time,
                                    ))
                                    .await;
                            }
                            EventType::ExcuteTask(task_body) => {
                                let (task, context, clients) = task_body;
                                let result = task.handler(context).await;
                                check_handler_result(result, clients.clone()).await;
                            }
                        }
                    } else {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                }
            });
        }
    }

    pub(crate) fn get_global_queue(&self) -> ReactorEventSender {
        self.global_queue.clone()
    }
}

#[inline(always)]
fn get_event(
    local_queue: &Worker<ReactorEvent>,
    global_queue: &ReactorEventSender,
    stealers_arc: &Arc<Vec<Stealer<ReactorEvent>>>,
) -> Option<ReactorEvent> {
    // 1. local
    if let Some(event) = local_queue.pop() {
        return Some(event);
    }

    // 2. global
    if let Steal::Success(event) = global_queue.steal_batch_and_pop(local_queue) {
        return Some(event);
    }

    // 3. stealers
    for i in 0..stealers_arc.len() {
        if let Steal::Success(event) = stealers_arc[i].steal() {
            return Some(event);
        }
    }

    None
}
