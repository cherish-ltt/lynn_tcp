use std::{net::SocketAddr, sync::Arc, time::SystemTime};

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
    ClientsStructType, LynnRouter, ReactorEventSender,
    common_api::push_read_half,
    tcp_reactor::{NewSocketEventSender, event::ReactorEvent, TcpSocketConfig},
    connection_limiter::ConnectionLimiter,
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
            Arc<LynnRouter>,
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
        connection_limiter: Option<(
            &u64,                                           // rate_limit
            &usize,                                          // max_connections_per_ip
            Arc<ConnectionLimiter>,
        )>,
        tcp_config: TcpSocketConfig,
    ) {
        loop {
            // Waiting for a new link
            let clinet_result = tcp_listener.accept().await;
            if let Ok((socket, addr)) = clinet_result {
                let mut socket_permit = true;
                let mut socket = Some(socket);

                // Apply TCP socket configuration
                if let Some(ref s) = socket {
                    if let Err(e) = s.set_nodelay(tcp_config.nodelay) {
                        warn!(
                            "Failed to set TCP_NODELAY for {}: {}, using default",
                            addr, e
                        );
                    }
                }

                // Apply TCP keep-alive and buffer sizes using socket2
                if tcp_config.keepalive_enabled || tcp_config.recv_buffer_size > 0 || tcp_config.send_buffer_size > 0 {
                    if let Some(s) = socket.take() {
                        // Convert TcpStream to std::net::TcpStream to access socket2
                        match s.into_std() {
                            Ok(std_socket) => {
                                use socket2::Socket;
                                let socket2 = Socket::from(std_socket);

                                // Apply TCP keep-alive if enabled
                                if tcp_config.keepalive_enabled {
                                    use socket2::TcpKeepalive;
                                    let ka = TcpKeepalive::new()
                                        .with_time(std::time::Duration::from_secs(tcp_config.keepalive_time_secs));
                                    if let Err(e) = socket2.set_tcp_keepalive(&ka) {
                                        warn!(
                                            "Failed to set keep-alive for {}: {}, using default",
                                            addr, e
                                        );
                                    }
                                }

                                // Set buffer sizes if specified (non-zero)
                                if tcp_config.recv_buffer_size > 0 {
                                    if let Err(e) = socket2.set_recv_buffer_size(tcp_config.recv_buffer_size) {
                                        warn!(
                                            "Failed to set recv buffer size for {}: {}, using default",
                                            addr, e
                                        );
                                    }
                                }

                                if tcp_config.send_buffer_size > 0 {
                                    if let Err(e) = socket2.set_send_buffer_size(tcp_config.send_buffer_size) {
                                        warn!(
                                            "Failed to set send buffer size for {}: {}, using default",
                                            addr, e
                                        );
                                    }
                                }

                                // Convert back to TcpStream
                                let std_socket: std::net::TcpStream = socket2.into();
                                match TcpStream::from_std(std_socket) {
                                    Ok(new_s) => { socket = Some(new_s); }
                                    Err(e) => {
                                        warn!("Failed to convert back to TcpStream for {}: {}, closing connection", addr, e);
                                        socket_permit = false;
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Failed to convert TcpStream for {}: {}, using default settings", addr, e);
                                // Note: We lost the socket here, but this is a very rare error case
                                // In practice, this conversion should never fail
                                socket_permit = false;
                            }
                        }
                    }
                }

                // Check global max connections
                if let Some(max_connections) = alow_max_connections {
                    if clients.len() >= *max_connections {
                        socket_permit = false;
                        warn!(
                            "Server socket's count is more than MAX_CONNECTIONS, can not accept new client:{}",
                            addr
                        );
                    }
                }

                // Check connection limiter (rate limit and per-IP limit)
                if socket_permit {
                    if let Some((_, _, limiter)) = &connection_limiter {
                        let ip = addr.ip();
                        if !limiter.check_connection(ip).await {
                            socket_permit = false;
                            warn!(
                                "Connection from {} rejected by connection limiter (rate limit or per-IP limit exceeded)",
                                addr
                            );
                        }
                    }
                }

                if socket_permit {
                    if let Some(s) = socket {
                        info!("Accepted connection from: {}", addr);
                        global_queue.push(ReactorEvent::crate_new_socket_event(s, addr));
                    }
                } else {
                    if let Some(mut s) = socket {
                        let _ = s.shutdown().await;
                    }
                }
            } else {
                if let Err(e) = clinet_result {
                    warn!(
                        "Failed to accept connection, server run next, e :{}",
                        e.to_string()
                    );
                }
            }
        }
    }
}
