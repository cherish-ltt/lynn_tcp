#![cfg(feature = "server")]

use lynn_tcp::{
    lynn_server::{
        ClientsContext, LynnServer, LynnServerConfigBuilder, SessionSendOutcome, ShutdownOutcome,
    },
    lynn_state::AppState,
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{Semaphore, mpsc, oneshot},
    task::JoinHandle,
    time::{sleep, timeout},
};

struct Work {
    entered: Semaphore,
    release: Semaphore,
    calls: AtomicUsize,
}

impl Work {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
            calls: AtomicUsize::new(0),
        })
    }

    async fn wait(&self) {
        timeout(Duration::from_secs(3), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }

    async fn block(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
    }
}

fn frame(method: u16) -> Vec<u8> {
    let mut frame = 9177u16.to_le_bytes().to_vec();
    frame.extend_from_slice(&5u64.to_le_bytes());
    frame.push(1);
    frame.extend_from_slice(&method.to_le_bytes());
    frame.extend_from_slice(&7719u16.to_le_bytes());
    frame
}

async fn response(peer: &mut (impl AsyncRead + Unpin)) -> (u16, Vec<u8>) {
    timeout(Duration::from_secs(3), async {
        let mut head = [0; 10];
        peer.read_exact(&mut head).await.unwrap();
        assert_eq!(&head[..2], &9177u16.to_le_bytes());
        let mut body = vec![0; u64::from_le_bytes(head[2..].try_into().unwrap()) as usize];
        peer.read_exact(&mut body).await.unwrap();
        assert_eq!(&body[body.len() - 2..], &7719u16.to_le_bytes());
        (
            u16::from_le_bytes(body[1..3].try_into().unwrap()),
            body[3..body.len() - 2].to_vec(),
        )
    })
    .await
    .unwrap()
}

async fn echo(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(1, "done".into(), vec![input.get_input_addr().unwrap()]).unwrap()
}

async fn delayed(input: InputBufVO, work: AppState<Arc<Work>>) -> HandlerResult {
    work.block().await;
    echo(input).await
}

async fn delayed_broadcast(clients: ClientsContext, work: AppState<Arc<Work>>) -> HandlerResult {
    let targets = clients.get_all_clients_addrs().await;
    work.block().await;
    HandlerResult::new_with_send(1, "all".into(), targets).unwrap()
}

async fn server(work: Arc<Work>) -> LynnServer<'static> {
    LynnServer::new_with_config(
        LynnServerConfigBuilder::new()
            .with_addr("127.0.0.1:0")
            .unwrap()
            .with_server_max_taskpool_size(&2)
            .with_server_max_connections(Some(&8))
            .with_max_connections_per_ip(&8)
            .with_server_check_heart_timeout_time(&300)
            .build(),
    )
    .await
    .with_state(work)
}

async fn launch(
    server: LynnServer<'static>,
    grace: Duration,
) -> (
    SocketAddr,
    oneshot::Sender<()>,
    JoinHandle<lynn_tcp::Result<ShutdownOutcome>>,
) {
    let ready = server.startup_handle();
    let (stop, signal) = oneshot::channel();
    let task = tokio::spawn(server.start_with_shutdown(
        async {
            let _ = signal.await;
        },
        grace,
    ));
    let addr = timeout(Duration::from_secs(3), ready.wait())
        .await
        .unwrap()
        .unwrap();
    (addr, stop, task)
}

async fn stopped_accepting(addr: SocketAddr) {
    timeout(Duration::from_secs(3), async {
        while TcpStream::connect(addr).await.is_ok() {
            sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
}

async fn outcome(task: JoinHandle<lynn_tcp::Result<ShutdownOutcome>>) -> ShutdownOutcome {
    timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap()
}

#[tokio::test]
async fn an_empty_server_can_be_drained() {
    let server = LynnServer::new_with_addr("127.0.0.1:0").await;
    let outcome = server
        .start_with_shutdown(async {}, Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(outcome, ShutdownOutcome::Drained);
}

#[tokio::test]
async fn accepted_handler_and_queued_push_finish_before_close() {
    for peer_eof in [false, true] {
        let work = Work::new();
        let (registered, mut sessions) = mpsc::unbounded_channel();
        let server = server(work.clone())
            .await
            .add_router(1, delayed)
            .with_middleware(move |request, next| {
                registered.send(request.session().clone()).unwrap();
                next.run(request)
            });
        let broadcast = server.broadcast_handle();
        let (addr, stop, task) = launch(server, Duration::from_secs(3)).await;
        let mut peer = TcpStream::connect(addr).await.unwrap();
        peer.write_all(&frame(1)).await.unwrap();
        work.wait().await;
        let session = timeout(Duration::from_secs(3), sessions.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(broadcast.broadcast(7, "queued".into()).unwrap().enqueued, 1);
        assert_eq!(
            session.try_send(9, "private queued".into()).unwrap(),
            SessionSendOutcome::Enqueued
        );
        if peer_eof {
            peer.shutdown().await.unwrap();
        }
        stop.send(()).unwrap();
        stopped_accepting(addr).await;
        assert!(broadcast.broadcast(8, "rejected".into()).is_err());
        assert_eq!(
            session.try_send(8, "rejected".into()).unwrap(),
            SessionSendOutcome::Closed
        );
        assert!(!session.observer().is_closed());
        assert!(!task.is_finished());
        work.release.add_permits(1);
        assert_eq!(response(&mut peer).await, (7, b"queued".to_vec()));
        assert_eq!(response(&mut peer).await, (9, b"private queued".to_vec()));
        assert_eq!(response(&mut peer).await, (1, b"done".to_vec()));
        assert_eq!(outcome(task).await, ShutdownOutcome::Drained);
        assert!(session.observer().is_closed());
        assert_eq!(peer.read(&mut [0]).await.unwrap(), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cross_connection_response_waits_for_the_global_request_barrier() {
    let work = Work::new();
    let server = server(work.clone())
        .await
        .add_router(1, delayed_broadcast)
        .add_router(2, echo);
    let (addr, stop, task) = launch(server, Duration::from_secs(3)).await;
    let mut target = TcpStream::connect(addr).await.unwrap();
    target.write_all(&frame(2)).await.unwrap();
    assert_eq!(response(&mut target).await.1, b"done");
    let mut source = TcpStream::connect(addr).await.unwrap();
    source.write_all(&frame(1)).await.unwrap();
    work.wait().await;
    stop.send(()).unwrap();
    stopped_accepting(addr).await;
    assert!(
        timeout(Duration::from_millis(20), target.read(&mut [0]))
            .await
            .is_err()
    );
    work.release.add_permits(1);
    assert_eq!(response(&mut target).await.1, b"all");
    assert_eq!(response(&mut source).await.1, b"all");
    assert_eq!(outcome(task).await, ShutdownOutcome::Drained);
}

#[tokio::test]
async fn new_requests_after_the_boundary_are_not_accepted() {
    let work = Work::new();
    let server = server(work.clone()).await.add_router(1, delayed);
    let (addr, stop, task) = launch(server, Duration::from_secs(3)).await;
    let mut peer = TcpStream::connect(addr).await.unwrap();
    peer.write_all(&frame(1)).await.unwrap();
    work.wait().await;
    stop.send(()).unwrap();
    stopped_accepting(addr).await;
    peer.write_all(&frame(1).repeat(100)).await.unwrap();
    work.release.add_permits(101);
    assert_eq!(response(&mut peer).await.1, b"done");
    assert_eq!(outcome(task).await, ShutdownOutcome::Drained);
    assert_eq!(work.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn handler_timeout_and_abort_release_app_state_and_connections() {
    for abort in [false, true] {
        let work = Work::new();
        let weak = Arc::downgrade(&work);
        let (registered, mut sessions) = mpsc::unbounded_channel();
        let server = server(work.clone())
            .await
            .add_router(1, delayed)
            .with_middleware(move |request, next| {
                registered.send(request.session().clone()).unwrap();
                next.run(request)
            });
        let broadcast = server.broadcast_handle();
        let grace = if abort {
            Duration::from_secs(30)
        } else {
            Duration::from_millis(40)
        };
        let (addr, stop, task) = launch(server, grace).await;
        let mut peer = TcpStream::connect(addr).await.unwrap();
        peer.write_all(&frame(1)).await.unwrap();
        work.wait().await;
        let session = timeout(Duration::from_secs(3), sessions.recv())
            .await
            .unwrap()
            .unwrap();
        stop.send(()).unwrap();
        stopped_accepting(addr).await;
        drop(work);
        if abort {
            assert!(!task.is_finished(), "abort must cancel an active drain");
            task.abort();
            assert!(task.await.unwrap_err().is_cancelled());
            timeout(Duration::from_secs(3), async {
                while weak.upgrade().is_some() {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            timeout(Duration::from_secs(3), session.observer().closed())
                .await
                .unwrap();
        } else {
            assert_eq!(outcome(task).await, ShutdownOutcome::TimedOut);
            assert!(session.observer().is_closed());
            assert!(
                weak.upgrade().is_none(),
                "return must follow actual task release"
            );
        }
        assert!(broadcast.broadcast(1, "late".into()).is_err());
        assert_eq!(
            timeout(Duration::from_secs(3), peer.read(&mut [0]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        let rebound = TcpListener::bind(addr).await.unwrap();
        drop(rebound);
    }
}

#[tokio::test]
async fn middleware_return_phase_remains_inside_the_drain_barrier() {
    let work = Work::new();
    let gate = work.clone();
    let server = server(work.clone())
        .await
        .add_router(1, echo)
        .with_middleware(move |request, next| {
            let gate = gate.clone();
            async move {
                let response = next.run(request).await;
                gate.block().await;
                response
            }
        });
    let (addr, stop, task) = launch(server, Duration::from_secs(3)).await;
    let mut peer = TcpStream::connect(addr).await.unwrap();
    peer.write_all(&frame(1)).await.unwrap();
    work.wait().await;
    stop.send(()).unwrap();
    stopped_accepting(addr).await;
    assert!(
        timeout(Duration::from_millis(20), peer.read(&mut [0]))
            .await
            .is_err()
    );
    work.release.add_permits(1);
    assert_eq!(response(&mut peer).await.1, b"done");
    assert_eq!(outcome(task).await, ShutdownOutcome::Drained);
}

#[tokio::test]
async fn handler_panic_or_failed_response_reports_incomplete() {
    for (panic, peer_eof) in [(false, false), (true, false), (false, true), (true, true)] {
        let work = Work::new();
        let server =
            server(work.clone())
                .await
                .add_router(1, move |work: AppState<Arc<Work>>| async move {
                    work.block().await;
                    assert!(!panic, "intentional draining handler panic");
                    HandlerResult::new_with_send(
                        1,
                        "absent".into(),
                        vec!["127.0.0.1:1".parse().unwrap()],
                    )
                    .unwrap()
                });
        let (addr, stop, task) = launch(server, Duration::from_secs(3)).await;
        let mut peer = TcpStream::connect(addr).await.unwrap();
        peer.write_all(&frame(1)).await.unwrap();
        work.wait().await;
        if peer_eof {
            peer.shutdown().await.unwrap();
        }
        stop.send(()).unwrap();
        stopped_accepting(addr).await;
        work.release.add_permits(1);
        assert_eq!(outcome(task).await, ShutdownOutcome::Incomplete);
    }
}

#[tokio::test]
async fn zero_grace_cancels_and_startup_error_retains_its_type() {
    let server = LynnServer::new_with_addr("127.0.0.1:0").await;
    assert_eq!(
        server
            .start_with_shutdown(async {}, Duration::ZERO)
            .await
            .unwrap(),
        ShutdownOutcome::TimedOut
    );
    let occupied = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server = LynnServer::new_with_addr(occupied.local_addr().unwrap()).await;
    assert!(
        matches!(server.start_with_shutdown(async {}, Duration::ZERO).await,
        Err(lynn_tcp::LynnError::Io(error)) if error.kind() == std::io::ErrorKind::AddrInUse)
    );
}
