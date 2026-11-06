use super::*;
use crate::{
    application::server::{connection_session::ConnectionSession, server_common::read_connection},
    domain::model::{
        lynn_user::LynnUser,
        session::{SessionHandle, SessionObserver},
    },
    infrastructure::connection::connection_limiter::ConnectionPermits,
    infrastructure::tcp::session_io::SessionIo,
};
use std::{
    io,
    pin::Pin,
    sync::atomic::AtomicUsize,
    task::{Context, Poll},
    time::SystemTime,
};
use tokio::{
    io::AsyncWrite,
    sync::{Notify, RwLock, Semaphore},
    time::timeout,
};

#[derive(Clone, Copy)]
enum Block {
    Write,
    Flush,
    Shutdown,
    Error,
}

struct ControlledWriter {
    block: Block,
    written: Arc<AtomicUsize>,
    blocked: Arc<Notify>,
    _state: Arc<()>,
}

impl AsyncWrite for ControlledWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let count = if matches!(self.block, Block::Write) {
            if self.written.load(Ordering::SeqCst) != 0 {
                self.blocked.notify_one();
                return Poll::Pending;
            }
            bytes.len().min(3)
        } else {
            bytes.len()
        };
        self.written.fetch_add(count, Ordering::SeqCst);
        Poll::Ready(Ok(count))
    }

    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        if matches!(self.block, Block::Flush) {
            self.blocked.notify_one();
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.block {
            Block::Shutdown => Poll::Pending,
            Block::Error => Poll::Ready(Err(io::ErrorKind::BrokenPipe.into())),
            _ => Poll::Ready(Ok(())),
        }
    }
}

#[tokio::test]
async fn one_deadline_cancels_blocked_write_flush_shutdown_and_full_queue() {
    for block in [Block::Write, Block::Flush, Block::Shutdown, Block::Error] {
        let state = Arc::new(());
        let weak = Arc::downgrade(&state);
        let server = LynnServer::new_with_config(
            crate::application::server::server_config::LynnServerConfigBuilder::new()
                .with_addr("127.0.0.1:0")
                .unwrap()
                .with_server_max_taskpool_size(&2)
                .build(),
        )
        .await
        .with_state(state.clone());
        let tasks = server.tasks.clone();
        let written = Arc::new(AtomicUsize::new(0));
        let blocked = Arc::new(Notify::new());
        let address: SocketAddr = "127.0.0.1:60000".parse().unwrap();
        let identity = Arc::new(RwLock::new(SystemTime::now()));
        let (completion, observer) = SessionObserver::new();
        let (user, writer, closed) = LynnUser::new(
            Box::new(SessionIo::new(
                ControlledWriter {
                    block,
                    written: written.clone(),
                    blocked: blocked.clone(),
                    _state: state,
                },
                completion.clone(),
                tasks.connection(),
            )),
            identity.clone(),
            0,
            tasks.clone(),
        );
        let handle = SessionHandle::new(
            &user,
            observer.clone(),
            closed.clone(),
            tasks.clone(),
            (9177, 7719),
        );
        user.send_response(&bytes::BytesMut::from(&b"response"[..]))
            .await;
        if matches!(block, Block::Write | Block::Flush) {
            timeout(Duration::from_secs(2), blocked.notified())
                .await
                .unwrap();
            for _ in 0..crate::const_config::DEFAULT_SYSTEM_CHANNEL_SIZE {
                user.send_response(&bytes::BytesMut::from(&b"never-retried"[..]))
                    .await;
            }
        }
        let global = Arc::new(AtomicUsize::new(1));
        let limiter = Arc::new(ConnectionLimiter::new(0, 1));
        let permit = ConnectionPermits {
            global: Some(global.clone()),
            per_ip: limiter.acquire(address.ip()).await,
            observation: None,
        };
        server.clients.0.insert(address, user);
        let session = ConnectionSession {
            handle,
            writer,
            closed,
            clients: server.clients.0.clone(),
            addr: address,
            last_communicate_time: identity.clone(),
            permit,
            tasks: tasks.clone(),
        };
        let (read, _peer) = tokio::io::duplex(64);
        tasks.spawn(read_connection(
            Box::new(SessionIo::new(read, completion, tasks.connection())),
            Arc::new(Semaphore::new(1)),
            address,
            server.clients.0.clone(),
            9177,
            7719,
            server.lynn_router.clone(),
            Arc::new(Injector::new()),
            identity,
            server.state_registry.clone(),
            session,
            1,
            0,
        ));
        let outcome = timeout(
            Duration::from_secs(2),
            server.start_with_shutdown(async {}, Duration::from_millis(40)),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(
            outcome,
            if matches!(block, Block::Error) {
                ShutdownOutcome::Incomplete
            } else {
                ShutdownOutcome::TimedOut
            }
        );
        assert!(observer.is_closed());
        assert!(weak.upgrade().is_none());
        assert_eq!(global.load(Ordering::Acquire), 0);
        assert!(limiter.acquire(address.ip()).await.is_some());
        assert_eq!(
            written.load(Ordering::SeqCst),
            if matches!(block, Block::Write) { 3 } else { 8 }
        );
        timeout(Duration::from_secs(1), tasks.finished())
            .await
            .unwrap();
    }
}
