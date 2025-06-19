use std::{collections::HashMap, net::SocketAddr, ops::Deref, sync::Arc, time::SystemTime};

use tokio::{
    io::{AsyncWriteExt, ReadHalf},
    net::{TcpListener, TcpStream},
    sync::{
        RwLock, Semaphore,
        mpsc::{self},
    },
};
use tracing::{info, warn};

use crate::app::{
    AsyncFunc, ClientsStructType, ReactorEventSender,
    common_api::push_read_half,
    tcp_reactor::{NewSocketEventSender, event::ReactorEvent},
};

pub(super) struct CoreReactor {
    pub(super) tx: NewSocketEventSender,
}

impl CoreReactor {
    pub(crate) fn new() -> Self {
        let (tx, mut rx) = mpsc::channel::<(
            ReadHalf<TcpStream>,
            Arc<Semaphore>,
            SocketAddr,
            ClientsStructType,
            u16,
            u16,
            Arc<Option<HashMap<u16, Arc<AsyncFunc>>>>,
            ReactorEventSender,
            Arc<RwLock<SystemTime>>,
        )>(64);
        tokio::spawn(async move {
            while let Some(a) = rx.recv().await {
                push_read_half(a.0, a.1, a.2, a.3, a.4, a.5, a.6, a.7, a.8).await;
            }
        });

        Self { tx }
    }

    pub(crate) async fn run(
        &self,
        tcp_listener: TcpListener,
        clients: ClientsStructType,
        alow_max_connections: Option<&usize>,
        global_queue: ReactorEventSender,
    ) {
        loop {
            // Waiting for a new link
            let clinet_result = tcp_listener.accept().await;
            if let Ok((mut socket, addr)) = clinet_result {
                let mut socket_permit = true;
                {
                    if let Some(max_connections) = alow_max_connections {
                        let clients = clients.write().await;
                        let guard = clients.deref();
                        if guard.len() < *max_connections {
                            socket_permit = true;
                        } else {
                            socket_permit = false;
                        }
                    }
                }
                if socket_permit {
                    info!("Accepted connection from: {}", addr);
                    global_queue.push(ReactorEvent::crate_new_socket_event(socket, addr));
                } else {
                    let _ = socket.shutdown().await;
                    warn!(
                        "Server socket's count is more than MAX_CONNECTIONS ,can not accept new client:{}",
                        addr
                    );
                }
            } else {
                if let Err(e) = clinet_result {
                    warn!(
                        "Failed to accept connection , server run next, e :{}",
                        e.to_string()
                    );
                }
            }
        }
    }
}
