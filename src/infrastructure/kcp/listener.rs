use std::{collections::HashMap, io, net::SocketAddr, sync::Arc, time::Duration};

use kcp_ovo::{KcpCmd, helper::KcpPacketHeader};
use tokio::{net::UdpSocket, sync::mpsc, time::interval};

use crate::{
    application::server::lynn_server::ReactorEventSender,
    infrastructure::{
        connection::connection_limiter::{ConnectionLimiter, ConnectionPermits},
        tcp::{reactor::ReactorEvent, stream::LynnStream},
    },
};

#[cfg(test)]
use super::HELLO;
use super::{KcpConfig, Mode, PACKETS, stream::KcpStream};

#[cfg(test)]
#[path = "listener_tests.rs"]
mod tests;

pub(crate) struct KcpListener {
    socket: Arc<UdpSocket>,
    config: KcpConfig,
    mode: Mode,
}

impl KcpListener {
    pub(crate) fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub(crate) async fn bind(addr: &str, config: KcpConfig, mode: Mode) -> io::Result<Self> {
        Ok(Self {
            socket: Arc::new(UdpSocket::bind(addr).await?),
            config,
            mode,
        })
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "listener owns transport admission and bounded TLS setup"
    )]
    pub(crate) async fn run(
        self,
        max: Option<usize>,
        limiter: Option<Arc<ConnectionLimiter>>,
        queue: ReactorEventSender,
        #[cfg(feature = "kcp-tls")] acceptor: Arc<
            crate::infrastructure::tcp::stream::StreamAcceptor,
        >,
        #[cfg(feature = "kcp-tls")] tasks: crate::application::server::server_tasks::ServerTasks,
        #[cfg(feature = "kcp-tls")] max_handshakes: usize,
    ) {
        use crate::infrastructure::metrics::transport::ConnectionObservation;
        use std::sync::atomic::{AtomicUsize, Ordering};
        #[cfg(feature = "kcp-tls")]
        let handshakes = Arc::new(tokio::sync::Semaphore::new(max_handshakes));
        let count = Arc::new(AtomicUsize::new(0));
        let mut sessions: HashMap<SocketAddr, (u32, mpsc::Sender<Vec<u8>>)> = HashMap::new();
        let mut buffer = vec![0; 65536];
        let mut cleanup = interval(Duration::from_millis(100));
        loop {
            let received = tokio::select! {
                _ = cleanup.tick() => { sessions.retain(|_, (_, tx)| !tx.is_closed()); continue; },
                received = self.socket.recv_from(&mut buffer) => received,
            };
            let (n, addr) = match received {
                Ok(value) => value,
                Err(error) => {
                    tracing::warn!("KCP listener receive: {error}");
                    continue;
                },
            };
            let packet = &buffer[..n];
            let mut cursor = packet;
            let Ok((header, payload)) = KcpPacketHeader::from_bytes(&mut cursor) else {
                continue;
            };
            if header.conv == 0 || n > 1400 || header.len as usize != payload.len() {
                continue;
            }
            if sessions.get(&addr).is_some_and(|(_, tx)| tx.is_closed()) {
                sessions.remove(&addr);
            }
            if let Some((conv, tx)) = sessions.get(&addr) {
                if *conv == header.conv {
                    // Queue overflow drops UDP input; KCP retransmission supplies backpressure recovery.
                    let _ = tx.try_send(packet.to_vec());
                }
                continue;
            }
            if header.cmd != KcpCmd::Push
                || header.sn != 0
                || header.frg != 0
                || payload != self.mode.hello()
            {
                continue;
            }
            #[cfg(feature = "kcp-tls")]
            let admission = tokio::time::Instant::now();
            #[cfg(feature = "kcp-tls")]
            let handshake_permit = if matches!(self.mode, Mode::Tls) {
                match handshakes.clone().try_acquire_owned() {
                    Ok(permit) => Some(permit),
                    Err(_) => continue,
                }
            } else {
                None
            };
            let mut permit = ConnectionPermits {
                global: None,
                per_ip: None,
                observation: Some(ConnectionObservation::accepted()),
            };
            if let Some(max) = max {
                if count
                    .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                        (n < max).then_some(n + 1)
                    })
                    .is_err()
                {
                    continue;
                }
                permit.global = Some(count.clone());
            }
            if let Some(limiter) = &limiter {
                permit.per_ip = limiter.acquire(addr.ip()).await;
                if permit.per_ip.is_none() {
                    continue;
                }
            }
            let (tx, rx) = mpsc::channel(PACKETS);
            let stream = match KcpStream::start(
                self.socket.clone(),
                addr,
                header.conv,
                self.config.clone(),
                self.mode,
                Some(rx),
            ) {
                Ok((stream, _)) => stream,
                Err(error) => {
                    tracing::warn!("KCP setup: {error}");
                    continue;
                },
            };
            if tx.try_send(packet.to_vec()).is_err() {
                continue;
            }
            sessions.insert(addr, (header.conv, tx));
            #[cfg(feature = "kcp-tls")]
            if let Some(handshake_permit) = handshake_permit {
                let acceptor = acceptor.clone();
                let queue = queue.clone();
                tasks.spawn(async move {
                    let _handshake_permit = handshake_permit;
                    let deadline = admission
                        + Duration::from_secs(crate::const_config::TLS_HANDSHAKE_TIMEOUT_SECS);
                    match tokio::time::timeout_at(
                        deadline,
                        acceptor.accept(LynnStream::Kcp(stream), addr),
                    )
                    .await
                    {
                        Ok(Some(stream)) => {
                            queue.push(ReactorEvent::crate_new_socket_event(stream, addr, permit))
                        },
                        Ok(None) => {},
                        Err(_) => tracing::warn!("KCP TLS handshake with {addr} timed out"),
                    }
                });
                continue;
            }
            queue.push(ReactorEvent::crate_new_socket_event(
                LynnStream::Kcp(stream),
                addr,
                permit,
            ));
        }
    }
}
