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

use super::{HELLO, KcpConfig, PACKETS, stream::KcpStream};

#[cfg(test)]
#[path = "listener_tests.rs"]
mod tests;

pub(crate) struct KcpListener {
    socket: Arc<UdpSocket>,
    config: KcpConfig,
}

impl KcpListener {
    pub(crate) async fn bind(addr: &str, config: KcpConfig) -> io::Result<Self> {
        Ok(Self {
            socket: Arc::new(UdpSocket::bind(addr).await?),
            config,
        })
    }

    pub(crate) async fn run(
        self,
        max: Option<usize>,
        limiter: Option<Arc<ConnectionLimiter>>,
        queue: ReactorEventSender,
    ) {
        use crate::infrastructure::metrics::transport::ConnectionObservation;
        use std::sync::atomic::{AtomicUsize, Ordering};
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
            let Ok((header, payload)) =
                KcpPacketHeader::from_bytes(&mut bytes::Bytes::copy_from_slice(packet))
            else {
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
                || payload.as_ref() != HELLO
            {
                continue;
            }
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
            queue.push(ReactorEvent::crate_new_socket_event(
                LynnStream::Kcp(stream),
                addr,
                permit,
            ));
        }
    }
}
