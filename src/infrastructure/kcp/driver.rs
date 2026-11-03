use std::{io, net::SocketAddr, sync::Arc, time::Duration};

use kcp_ovo::{KcpError, core::kcp::Kcp, helper::KcpPacketHeader};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream},
    net::UdpSocket,
    sync::{mpsc, oneshot, watch},
    time::{Instant, interval},
};

use super::{CHUNK, HELLO, KcpConfig};

pub(crate) struct Driver {
    kcp: Kcp,
    socket: Arc<UdpSocket>,
    incoming: Option<mpsc::Receiver<Vec<u8>>>,
    idle: Duration,
}

impl Driver {
    pub(crate) fn new(
        socket: Arc<UdpSocket>,
        peer: SocketAddr,
        conv: u32,
        config: KcpConfig,
        incoming: Option<mpsc::Receiver<Vec<u8>>>,
    ) -> io::Result<Self> {
        config.validate().map_err(io::Error::other)?;
        let mut kcp = Kcp::new(conv, config.protocol).map_err(io::Error::other)?;
        let output = socket.clone();
        let connected = incoming.is_none();
        kcp.set_output(move |packet| {
            let socket = output.clone();
            async move {
                Ok(if connected {
                    socket.send(&packet).await?
                } else {
                    socket.send_to(&packet, peer).await?
                })
            }
        });
        Ok(Self {
            kcp,
            socket,
            incoming,
            idle: Duration::from_secs(config.idle_timeout_secs),
        })
    }

    pub(crate) async fn run(
        mut self,
        io: &mut DuplexStream,
        acknowledged: watch::Sender<(u64, bool)>,
        ready: oneshot::Sender<io::Result<()>>,
    ) -> io::Result<()> {
        let client = self.incoming.is_none();
        let (mut read, mut write) = tokio::io::split(io);
        let mut ready = Some(ready);
        let mut established = false;
        let mut consumed = 0u64;
        let mut pending = Vec::new();
        let mut offset = 0;
        let mut local_eof = false;
        let mut remote_eof = false;
        let mut output_ready = client;
        let start = Instant::now();
        let mut last_packet = start;
        let mut tick = interval(Duration::from_millis(self.kcp.interval));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut input = vec![0; 65536];
        let mut data = vec![0; CHUNK];
        if client {
            self.kcp.send(HELLO).map_err(io::Error::other)?;
        }
        loop {
            if offset == pending.len() {
                pending.clear();
                offset = 0;
                match self.kcp.recv() {
                    Ok(message) if !established => {
                        if message.as_ref() != HELLO {
                            return Err(io::Error::other("invalid KCP handshake"));
                        }
                        if !client {
                            self.kcp.send(HELLO).map_err(io::Error::other)?;
                            output_ready = true;
                        }
                        established = true;
                        if let Some(ready) = ready.take() {
                            let _ = ready.send(Ok(()));
                        }
                    },
                    Ok(message) => match message.split_first() {
                        Some((1, bytes)) if !remote_eof => pending.extend_from_slice(bytes),
                        Some((2, [])) if !remote_eof => {
                            remote_eof = true;
                            write.shutdown().await?;
                        },
                        _ => return Err(io::Error::other("invalid KCP session message")),
                    },
                    Err(
                        KcpError::QueueEmpty
                        | KcpError::IncompleteData(
                            kcp_ovo::error::IncompleteDataType::PayloadWaitForFrg,
                        ),
                    ) => {},
                    Err(error) => return Err(io::Error::other(error)),
                }
            }
            if output_ready && self.kcp.nodelay {
                let now = start.elapsed().as_millis() as u64;
                // Upstream update gates flush on ts_flush even after send/input.
                self.kcp.ts_flush = now;
                self.kcp.update(now).await.map_err(io::Error::other)?;
            }
            output_ready = false;
            if self.kcp.waitsnd() == 0 {
                acknowledged.send_replace((consumed, local_eof));
            }
            tokio::select! {
                _ = tick.tick() => {
                    if last_packet.elapsed() >= self.idle { return Err(io::Error::new(io::ErrorKind::TimedOut,"KCP peer idle timeout")); }
                    self.kcp.update(start.elapsed().as_millis() as u64).await.map_err(io::Error::other)?;
                    if self.kcp.state != 0 { return Err(io::Error::other("KCP retransmission limit reached")); }
                },
                packet = async {
                    match &mut self.incoming {
                        Some(incoming) => incoming.recv().await.ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe,"KCP listener stopped")),
                        None => self.socket.recv(&mut input).await.map(|n| input[..n].to_vec()),
                    }
                } => {
                    let packet = packet?;
                    let (header, payload) = KcpPacketHeader::from_bytes(&mut bytes::Bytes::copy_from_slice(&packet)).map_err(io::Error::other)?;
                    if header.conv != self.kcp.conv { continue; }
                    if header.len as usize != payload.len() || packet.len() > 1400 || header.frg as usize >= (CHUNK + 1).div_ceil(1372) { return Err(io::Error::other("invalid KCP datagram length or fragmentation")); }
                    self.kcp.current = start.elapsed().as_millis() as u64;
                    self.kcp.input(&packet).map_err(io::Error::other)?;
                    output_ready = true;
                    last_packet = Instant::now();
                },
                n = read.read(&mut data), if established && !local_eof && self.kcp.waitsnd() < usize::from(self.kcp.snd_wnd) => {
                    let n = n?;
                    if n == 0 { self.kcp.send(&[2]).map_err(io::Error::other)?; local_eof = true; }
                    else {
                        let mut message = Vec::with_capacity(n+1); message.push(1); message.extend_from_slice(&data[..n]);
                        self.kcp.send(&message).map_err(io::Error::other)?;
                        consumed += n as u64;
                    }
                    output_ready = true;
                },
                n = write.write(&pending[offset..]), if offset < pending.len() => {
                    let n = n?;
                    if n == 0 { return Err(io::Error::new(io::ErrorKind::WriteZero, "KCP receive consumer stopped")); }
                    offset += n;
                },
            }
        }
    }
}
