use std::{
    future::Future,
    io,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use tokio::{
    io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf},
    net::UdpSocket,
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
};

use super::{BUFFER, KcpConfig, Mode, driver};

type Flush = Pin<Box<dyn Future<Output = io::Result<()>> + Send>>;

pub(crate) struct KcpStream {
    io: DuplexStream,
    worker: JoinHandle<()>,
    acknowledged: watch::Receiver<(u64, bool)>,
    error: watch::Receiver<Option<String>>,
    written: u64,
    flush: Option<Flush>,
    shutdown: Option<Flush>,
}

impl KcpStream {
    #[allow(
        clippy::too_many_arguments,
        reason = "transport construction requires peer, mode and bounded ingress"
    )]
    pub(crate) fn start(
        socket: Arc<UdpSocket>,
        peer: SocketAddr,
        conv: u32,
        config: KcpConfig,
        mode: Mode,
        incoming: Option<mpsc::Receiver<Vec<u8>>>,
    ) -> io::Result<(Self, oneshot::Receiver<io::Result<()>>)> {
        let (io, transport) = tokio::io::duplex(BUFFER);
        let (ack_tx, acknowledged) = watch::channel((0, false));
        let (error_tx, error) = watch::channel(None);
        let (ready_tx, ready) = oneshot::channel();
        let driver = driver::Driver::new(socket, peer, conv, config, mode, incoming)?;
        // Drop driver I/O before its completion sender, including before the first poll.
        let driver_task = (driver, transport, error_tx);
        let worker = tokio::spawn(async move {
            let mut driver_task = driver_task;
            if let Err(error) = driver_task
                .0
                .run(&mut driver_task.1, ack_tx, ready_tx)
                .await
            {
                tracing::warn!("KCP session {peer} ended: {error}");
                driver_task.2.send_replace(Some(error.to_string()));
            }
        });
        Ok((
            Self {
                io,
                worker,
                acknowledged,
                error,
                written: 0,
                flush: None,
                shutdown: None,
            },
            ready,
        ))
    }

    #[cfg(feature = "server")]
    pub(crate) fn completion(&self) -> watch::Receiver<Option<String>> {
        self.error.clone()
    }

    #[cfg(feature = "client")]
    pub(crate) async fn connect(
        peer: SocketAddr,
        config: KcpConfig,
        mode: Mode,
    ) -> io::Result<Self> {
        use std::{collections::hash_map::RandomState, hash::BuildHasher};
        let socket = Arc::new(
            UdpSocket::bind(if peer.is_ipv4() {
                "0.0.0.0:0"
            } else {
                "[::]:0"
            })
            .await?,
        );
        socket.connect(peer).await?;
        let conv = (RandomState::new().hash_one(socket.local_addr()?) as u32).max(1);
        let (stream, ready) = Self::start(socket, peer, conv, config, mode, None)?;
        ready
            .await
            .map_err(|_| io::Error::other("KCP handshake worker stopped"))??;
        Ok(stream)
    }
}

impl Drop for KcpStream {
    fn drop(&mut self) {
        self.worker.abort();
    }
}

impl AsyncRead for KcpStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.io).poll_read(cx, buf);
        // The worker publishes errors before closing the duplex; inspect after polling EOF.
        if let Some(error) = this.error.borrow().as_ref() {
            return Poll::Ready(Err(io::Error::other(error.clone())));
        }
        result
    }
}

impl AsyncWrite for KcpStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if let Some(error) = this.error.borrow().as_ref() {
            return Poll::Ready(Err(io::Error::other(error.clone())));
        }
        let result = Pin::new(&mut this.io).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = result {
            this.written += n as u64;
            this.flush = None;
        }
        result
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.flush.is_none() {
            let expected = this.written;
            let mut ack = this.acknowledged.clone();
            this.flush = Some(Box::pin(async move {
                ack.wait_for(|count| count.0 >= expected)
                    .await
                    .map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "KCP closed before acknowledgement",
                        )
                    })?;
                Ok(())
            }));
        }
        let result = this.flush.as_mut().unwrap().as_mut().poll(cx);
        if result.is_ready() {
            this.flush = None;
        }
        result
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        std::task::ready!(self.as_mut().poll_flush(cx))?;
        let this = self.get_mut();
        std::task::ready!(Pin::new(&mut this.io).poll_shutdown(cx))?;
        if this.shutdown.is_none() {
            let mut ack = this.acknowledged.clone();
            this.shutdown = Some(Box::pin(async move {
                ack.wait_for(|progress| progress.1).await.map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "KCP closed before shutdown acknowledgement",
                    )
                })?;
                Ok(())
            }));
        }
        this.shutdown.as_mut().unwrap().as_mut().poll(cx)
    }
}
