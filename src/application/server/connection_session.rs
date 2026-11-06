use std::{net::SocketAddr, sync::Arc, time::SystemTime};

use tokio::{
    sync::{RwLock, Semaphore, mpsc::Sender, watch},
    task::JoinHandle,
};

use crate::{
    application::server::server_tasks::ServerTasks,
    domain::model::lynn_user::{ClientsStructType, LynnUserSignal},
    infrastructure::connection::connection_limiter::ConnectionPermits,
};

#[cfg(test)]
#[path = "connection_lifetime_tests.rs"]
mod lifetime_tests;

pub(crate) struct ConnectionSession {
    pub(crate) handle: crate::domain::model::session::SessionHandle,
    pub(crate) writer: JoinHandle<std::io::Result<()>>,
    pub(crate) closed: watch::Receiver<bool>,
    pub(crate) clients: ClientsStructType,
    pub(crate) addr: SocketAddr,
    pub(crate) last_communicate_time: Arc<RwLock<SystemTime>>,
    pub(crate) permit: ConnectionPermits,
    pub(crate) tasks: ServerTasks,
}

impl ConnectionSession {
    pub(crate) async fn finish_requests(permits: Arc<Semaphore>, mut remaining: usize) {
        // A peer's EOF drains only its requests; the server boundary drains all requests.
        while remaining > 0 {
            let batch = remaining.min(u32::MAX as usize) as u32;
            let Ok(permit) = permits.acquire_many(batch).await else {
                return;
            };
            permit.forget();
            remaining -= batch as usize;
        }
    }

    pub(crate) async fn close_writer(&mut self) {
        let sender = self
            .clients
            .get(&self.addr)
            .filter(|user| {
                Arc::ptr_eq(
                    &user.get_last_communicate_time(),
                    &self.last_communicate_time,
                )
            })
            .map(|user| user.response_sender());
        let tasks = &self.tasks;
        tokio::select! {
            _ = async {
                if let Some(sender) = sender {
                    queue_shutdown(sender, tasks).await;
                } else {
                    tasks.record_failure();
                }
                if !matches!((&mut self.writer).await, Ok(Ok(()))) {
                    tasks.record_failure();
                }
            } => {},
            _ = self.closed.changed() => tasks.record_failure(),
        }
    }
}

async fn queue_shutdown(sender: Sender<LynnUserSignal>, tasks: &ServerTasks) {
    loop {
        if tasks.is_draining() {
            tasks.requests_finished().await;
            if sender.send(LynnUserSignal::Shutdown).await.is_err() {
                tasks.record_failure();
            }
            return;
        }
        let permit = tokio::select! {
            biased;
            _ = tasks.draining() => continue,
            permit = sender.reserve() => permit,
        };
        let Ok(permit) = permit else {
            tasks.record_failure();
            return;
        };
        if let Some(_admission) = tasks.try_admit() {
            permit.send(LynnUserSignal::Shutdown);
            return;
        }
    }
}

impl Drop for ConnectionSession {
    fn drop(&mut self) {
        self.writer.abort();
        self.clients.remove_if(&self.addr, |_, user| {
            Arc::ptr_eq(
                &user.get_last_communicate_time(),
                &self.last_communicate_time,
            )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        application::server::server_common::read_connection,
        domain::{
            model::lynn_user::LynnUser, routing::router::LynnRouter,
            state::state_registry::StateRegistry,
        },
        infrastructure::connection::connection_limiter::ConnectionLimiter,
        infrastructure::tcp::stream::BoxedWriteHalf,
    };
    use bytes::BytesMut;
    use crossbeam_deque::Injector;
    use dashmap::DashMap;
    use std::{
        io,
        pin::Pin,
        sync::atomic::{AtomicUsize, Ordering},
        task::{Context, Poll, Waker},
        time::Duration,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWrite},
        sync::Semaphore,
        time::{sleep, timeout},
    };

    #[tokio::test]
    async fn eof_waiting_for_queue_capacity_obeys_the_later_global_barrier() {
        let (_owner, tasks) = ServerTasks::new();
        let request = tasks.request();
        let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
        assert!(
            sender
                .try_send(LynnUserSignal::SendResponse(BytesMut::from(&b"queued"[..])))
                .is_ok()
        );
        let mut shutdown = std::pin::pin!(queue_shutdown(sender.clone(), &tasks));
        let mut context = Context::from_waker(Waker::noop());
        assert!(shutdown.as_mut().poll(&mut context).is_pending());
        tasks.begin_draining();
        assert!(matches!(
            receiver.recv().await,
            Some(LynnUserSignal::SendResponse(_))
        ));
        assert!(shutdown.as_mut().poll(&mut context).is_pending());
        assert!(receiver.try_recv().is_err());
        assert!(
            sender
                .try_send(LynnUserSignal::SendResponse(BytesMut::from(
                    &b"accepted"[..]
                )))
                .is_ok()
        );
        request.complete();
        assert!(shutdown.as_mut().poll(&mut context).is_pending());
        assert!(matches!(
            receiver.recv().await,
            Some(LynnUserSignal::SendResponse(response)) if response == b"accepted"[..]
        ));
        timeout(Duration::from_secs(1), shutdown).await.unwrap();
        assert!(matches!(
            receiver.recv().await,
            Some(LynnUserSignal::Shutdown)
        ));
        assert!(!tasks.has_failed());
    }

    #[tokio::test]
    async fn closing_the_queue_while_waiting_for_accepted_requests_records_failure() {
        let (_owner, tasks) = ServerTasks::new();
        let request = tasks.request();
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        tasks.begin_draining();
        let mut shutdown = std::pin::pin!(queue_shutdown(sender, &tasks));
        let mut context = Context::from_waker(Waker::noop());
        assert!(shutdown.as_mut().poll(&mut context).is_pending());
        drop(receiver);
        request.complete();
        timeout(Duration::from_secs(1), shutdown).await.unwrap();
        assert!(tasks.has_failed());
    }

    struct BrokenWriter;

    impl AsyncWrite for BrokenWriter {
        fn poll_write(
            self: Pin<&mut Self>,
            _: &mut Context<'_>,
            _: &[u8],
        ) -> Poll<io::Result<usize>> {
            Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()))
        }

        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn write_error_cleans_up_reader_online_entry_and_quota() {
        assert_writer_cleanup(Box::new(BrokenWriter), 0).await;
    }

    #[tokio::test]
    async fn write_timeout_cleans_up_reader_online_entry_and_quota() {
        let (writer, _blocked_peer) = tokio::io::duplex(1);
        assert_writer_cleanup(Box::new(writer), 1).await;
    }

    async fn assert_writer_cleanup(write_half: BoxedWriteHalf, write_timeout_secs: u64) {
        let (_owner, tasks) = ServerTasks::new();
        let addr: SocketAddr = "127.0.0.1:9177".parse().unwrap();
        let clients = Arc::new(DashMap::new());
        let time = Arc::new(RwLock::new(SystemTime::now()));
        let limiter = Arc::new(ConnectionLimiter::new(0, 1));
        let permit = limiter.acquire(addr.ip()).await;
        let global = Arc::new(AtomicUsize::new(1));
        let (user, writer, closed) =
            LynnUser::new(write_half, time.clone(), write_timeout_secs, tasks.clone());
        let send = user.send_response(&BytesMut::from(&b"response"[..]));
        clients.insert(addr, user);
        let session = ConnectionSession {
            handle: crate::domain::model::session::SessionHandle::closed_for_test(),
            writer,
            closed,
            clients: clients.clone(),
            addr,
            last_communicate_time: time.clone(),
            permit: ConnectionPermits {
                per_ip: permit,
                global: Some(global.clone()),
                observation: None,
            },
            tasks,
        };
        let (read, mut peer) = tokio::io::duplex(64);
        tokio::spawn(read_connection(
            Box::new(read),
            Arc::new(Semaphore::new(1)),
            addr,
            clients.clone(),
            9177,
            7719,
            Arc::new(LynnRouter::new()),
            Arc::new(Injector::new()),
            time,
            Arc::new(StateRegistry::new()),
            session,
            1,
            0,
        ));
        send.await;
        timeout(Duration::from_secs(2), async {
            while !clients.is_empty() {
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        assert!(limiter.acquire(addr.ip()).await.is_some());
        assert_eq!(global.load(Ordering::Acquire), 0);
        assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn stale_cleanup_does_not_remove_replacement_connection() {
        let (_owner, tasks) = ServerTasks::new();
        let addr: SocketAddr = "127.0.0.1:9177".parse().unwrap();
        let clients = Arc::new(DashMap::new());
        let limiter = Arc::new(ConnectionLimiter::new(0, 2));
        let old_time = Arc::new(RwLock::new(SystemTime::now()));
        let (old_user, writer, closed) = LynnUser::new(
            Box::new(tokio::io::sink()),
            old_time.clone(),
            0,
            tasks.clone(),
        );
        clients.insert(addr, old_user);
        let session = ConnectionSession {
            handle: crate::domain::model::session::SessionHandle::closed_for_test(),
            writer,
            closed,
            clients: clients.clone(),
            addr,
            last_communicate_time: old_time,
            permit: ConnectionPermits {
                per_ip: limiter.acquire(addr.ip()).await,
                global: None,
                observation: None,
            },
            tasks: tasks.clone(),
        };
        let new_permit = limiter.acquire(addr.ip()).await;
        let (new_user, new_writer, _) = LynnUser::new(
            Box::new(tokio::io::sink()),
            Arc::new(RwLock::new(SystemTime::now())),
            0,
            tasks,
        );
        clients.insert(addr, new_user);
        drop(session);
        assert!(clients.contains_key(&addr));
        let third = limiter.acquire(addr.ip()).await;
        assert!(third.is_some());
        assert!(limiter.acquire(addr.ip()).await.is_none());
        drop(third);
        drop(new_permit);
        new_writer.abort();
    }

    #[tokio::test]
    async fn backpressured_response_does_not_hold_clients_map_lock() {
        let (_owner, tasks) = ServerTasks::new();
        let addr: SocketAddr = "127.0.0.1:9177".parse().unwrap();
        let clients = Arc::new(DashMap::new());
        let (transport, _peer) = tokio::io::duplex(1);
        let (user, writer, _) = LynnUser::new(
            Box::new(transport),
            Arc::new(RwLock::new(SystemTime::now())),
            0,
            tasks,
        );
        let response = BytesMut::from(&b"pending"[..]);
        user.send_response(&response).await;
        tokio::task::yield_now().await;
        for _ in 0..crate::const_config::DEFAULT_SYSTEM_CHANNEL_SIZE {
            user.send_response(&response).await;
        }
        clients.insert(addr, user);
        let pending = clients.get(&addr).unwrap().send_response(&response);
        assert!(clients.remove(&addr).is_some());
        writer.abort();
        timeout(Duration::from_secs(1), pending).await.unwrap();
    }
}
