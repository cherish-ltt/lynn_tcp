use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::SystemTime};

use tokio::{
    io::ReadHalf,
    net::{TcpListener, TcpStream},
    sync::{RwLock, Semaphore, mpsc::Sender},
};

use crate::app::{
    AsyncFunc, ClientsStructType, ReactorEventSender,
    tcp_reactor::{event::EventManager, reactor::CoreReactor},
};

mod event;
mod reactor;

pub(crate) mod event_api {
    pub(crate) use super::event::*;
}

pub(crate) type NewSocketEventSender = Sender<(
    ReadHalf<TcpStream>,
    Arc<Semaphore>,
    SocketAddr,
    ClientsStructType,
    u16,
    u16,
    Arc<Option<HashMap<u16, Arc<AsyncFunc>>>>,
    ReactorEventSender,
    Arc<RwLock<SystemTime>>,
)>;

pub(super) struct TcpReactor {
    core_reactor: CoreReactor,
    event_manager: EventManager,
}

impl TcpReactor {
    pub(super) fn new() -> Self {
        Self {
            core_reactor: CoreReactor::new(),
            event_manager: EventManager::new(),
        }
    }

    pub(super) async fn start(
        &self,
        clients: ClientsStructType,
        server_single_processs_permit: &usize,
        message_header_mark: u16,
        message_tail_mark: u16,
        router_map_async: Arc<Option<HashMap<u16, Arc<AsyncFunc>>>>,
        tcp_listener: TcpListener,
        alow_max_connections: Option<&usize>,
        server_max_reactor_taskpool_size: &usize,
    ) {
        self.event_manager.run(
            clients.clone(),
            server_single_processs_permit,
            message_header_mark,
            message_tail_mark,
            router_map_async,
            self.event_manager.get_global_queue(),
            self.core_reactor.tx.clone(),
            server_max_reactor_taskpool_size,
        );
        self.core_reactor
            .run(
                tcp_listener,
                clients,
                alow_max_connections,
                self.event_manager.get_global_queue(),
            )
            .await;
    }
}
