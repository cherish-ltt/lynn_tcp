#![cfg(feature = "server")]

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use lynn_tcp::{
    LynnError,
    lynn_metrics::METRICS,
    lynn_middleware::{MiddlewareRequest, Next, RequestContext},
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_state::AppState,
    lynn_tcp_dependents::HandlerResult,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{Mutex, Semaphore},
    time::{advance, pause, resume, sleep, timeout},
};

static LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Before,
    Handler,
    After,
}

struct Work {
    stage: Stage,
    entered: Semaphore,
    gate: Semaphore,
    calls: AtomicUsize,
    handler_calls: AtomicUsize,
    completed: AtomicUsize,
    blocked_drops: Arc<AtomicUsize>,
    request_drops: Arc<AtomicUsize>,
}

impl Work {
    fn new(stage: Stage) -> Arc<Self> {
        Arc::new(Self {
            stage,
            entered: Semaphore::new(0),
            gate: Semaphore::new(0),
            calls: AtomicUsize::new(0),
            handler_calls: AtomicUsize::new(0),
            completed: AtomicUsize::new(0),
            blocked_drops: Arc::new(AtomicUsize::new(0)),
            request_drops: Arc::new(AtomicUsize::new(0)),
        })
    }

    async fn block(&self) {
        let _probe = DropProbe(self.blocked_drops.clone());
        self.entered.add_permits(1);
        self.gate.acquire().await.unwrap().forget();
        self.completed.fetch_add(1, Ordering::SeqCst);
    }

    async fn wait_until_blocked(&self) {
        timeout(Duration::from_secs(3), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
}

struct DropProbe(Arc<AtomicUsize>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

async fn chain(mut request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
    if request.method_id() != 1 {
        return next.run(request).await;
    }
    let work = request.app_state::<Arc<Work>>().unwrap();
    work.calls.fetch_add(1, Ordering::SeqCst);
    request.insert(DropProbe(work.request_drops.clone()));
    if work.stage == Stage::Before {
        work.block().await;
    }
    let result = next.run(request).await;
    if work.stage == Stage::After {
        work.block().await;
    }
    result
}

async fn handler(context: RequestContext, work: AppState<Arc<Work>>) -> HandlerResult {
    assert!(context.get::<DropProbe>().is_some());
    work.handler_calls.fetch_add(1, Ordering::SeqCst);
    if work.stage == Stage::Handler {
        work.block().await;
    }
    echo(context).await
}

async fn echo(context: RequestContext) -> HandlerResult {
    HandlerResult::new_with_send(1, "ok".into(), vec![context.peer_addr()]).unwrap()
}

async fn deadline(request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
    timeout(Duration::from_secs(1), next.run(request))
        .await
        .map_err(|_| LynnError::timeout("middleware deadline"))?
}

async fn map_timeout(request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
    let addr = request.peer_addr();
    match next.run(request).await {
        Err(LynnError::Timeout(_)) => HandlerResult::new_with_send(1, "timeout".into(), vec![addr]),
        result => result,
    }
}

async fn configured(permits: &'static usize) -> (SocketAddr, LynnServer<'static>) {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .with_connection_rate_limit(&0)
        .with_max_connections_per_ip(&0)
        .with_server_single_process_permit(permits)
        .with_server_check_heart_timeout_time(&300)
        .build();
    (addr, LynnServer::new_with_config(config).await)
}

async fn connect(addr: SocketAddr) -> TcpStream {
    timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(peer) = TcpStream::connect(addr).await {
                return peer;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

fn frame(method: u16) -> Vec<u8> {
    let mut data = 9177u16.to_le_bytes().to_vec();
    data.extend_from_slice(&5u64.to_le_bytes());
    data.push(1);
    data.extend_from_slice(&method.to_le_bytes());
    data.extend_from_slice(&7719u16.to_le_bytes());
    data
}

async fn receive(peer: &mut TcpStream) -> Vec<u8> {
    timeout(Duration::from_secs(3), async {
        let mut head = [0; 10];
        peer.read_exact(&mut head).await.unwrap();
        let mut body = vec![0; u64::from_le_bytes(head[2..].try_into().unwrap()) as usize];
        peer.read_exact(&mut body).await.unwrap();
        body[3..body.len() - 2].to_vec()
    })
    .await
    .expect("response timeout")
}

async fn wait_for(mut ready: impl FnMut() -> bool) {
    timeout(Duration::from_secs(3), async {
        while !ready() {
            sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn timeout_drops_before_handler_and_after_futures_and_request_data_without_retry() {
    let _lock = LOCK.lock().await;
    for stage in [Stage::Before, Stage::Handler, Stage::After] {
        let work = Work::new(stage);
        let (addr, server) = configured(&1).await;
        let task = tokio::spawn(
            server
                .with_state(work.clone())
                .with_middleware(map_timeout)
                .with_middleware(deadline)
                .with_middleware(chain)
                .add_router(1, handler)
                .add_router(9, echo)
                .start(),
        );
        let mut peer = connect(addr).await;
        peer.write_all(&frame(1)).await.unwrap();
        work.wait_until_blocked().await;
        pause();
        advance(Duration::from_secs(2)).await;
        resume();
        assert_eq!(receive(&mut peer).await, b"timeout");
        assert_eq!(work.blocked_drops.load(Ordering::SeqCst), 1);
        assert_eq!(work.request_drops.load(Ordering::SeqCst), 1);
        work.gate.add_permits(1);
        peer.write_all(&frame(9)).await.unwrap();
        assert_eq!(receive(&mut peer).await, b"ok");
        assert_eq!(work.calls.load(Ordering::SeqCst), 1);
        assert_eq!(work.completed.load(Ordering::SeqCst), 0);
        assert_eq!(
            work.handler_calls.load(Ordering::SeqCst),
            usize::from(stage != Stage::Before)
        );
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn permits_cover_before_and_after_stages_and_eof_drains_the_entire_chain() {
    let _lock = LOCK.lock().await;
    for stage in [Stage::Before, Stage::After] {
        let work = Work::new(stage);
        let (addr, server) = configured(&1).await;
        let task = tokio::spawn(
            server
                .with_state(work.clone())
                .with_middleware(chain)
                .add_router(1, handler)
                .start(),
        );
        let mut peer = connect(addr).await;
        peer.write_all(&frame(1)).await.unwrap();
        work.wait_until_blocked().await;
        let received = METRICS.messages.received_total.get();
        let dropped = METRICS.messages.dropped_total.get();
        peer.write_all(&frame(1).repeat(8)).await.unwrap();
        peer.shutdown().await.unwrap();
        wait_for(|| {
            METRICS.messages.received_total.get() == received + 8.0
                && METRICS.messages.dropped_total.get() == dropped + 8.0
        })
        .await;
        work.gate.add_permits(1);
        assert_eq!(receive(&mut peer).await, b"ok");
        assert_eq!(
            timeout(Duration::from_secs(3), peer.read(&mut [0]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        assert_eq!(work.calls.load(Ordering::SeqCst), 1);
        assert_eq!(work.handler_calls.load(Ordering::SeqCst), 1);
        assert_eq!(work.blocked_drops.load(Ordering::SeqCst), 1);
        assert_eq!(work.request_drops.load(Ordering::SeqCst), 1);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn zero_permits_reject_without_entering_middleware() {
    let _lock = LOCK.lock().await;
    let work = Work::new(Stage::Before);
    let (addr, server) = configured(&0).await;
    let task = tokio::spawn(
        server
            .with_state(work.clone())
            .with_middleware(chain)
            .add_router(1, handler)
            .start(),
    );
    let mut peer = connect(addr).await;
    peer.write_all(&frame(1)).await.unwrap();
    peer.shutdown().await.unwrap();
    assert_eq!(
        timeout(Duration::from_secs(3), peer.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    assert_eq!(work.calls.load(Ordering::SeqCst), 0);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn owner_cancellation_drops_every_blocked_stage_and_registered_state() {
    let _lock = LOCK.lock().await;
    for stage in [Stage::Before, Stage::Handler, Stage::After] {
        let work = Work::new(stage);
        let weak = Arc::downgrade(&work);
        let blocked_drops = work.blocked_drops.clone();
        let request_drops = work.request_drops.clone();
        let (addr, server) = configured(&1).await;
        let task = tokio::spawn(
            server
                .with_state(work.clone())
                .with_middleware(chain)
                .add_router(1, handler)
                .start(),
        );
        let mut peer = connect(addr).await;
        peer.write_all(&frame(1)).await.unwrap();
        work.wait_until_blocked().await;
        drop(work);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        wait_for(|| weak.upgrade().is_none()).await;
        assert_eq!(blocked_drops.load(Ordering::SeqCst), 1);
        assert_eq!(request_drops.load(Ordering::SeqCst), 1);
        assert_eq!(
            timeout(Duration::from_secs(3), peer.read(&mut [0]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    }
}

#[tokio::test]
async fn dropping_unstarted_server_releases_middleware_captures_and_state() {
    let _lock = LOCK.lock().await;
    let captured = Arc::new(());
    let state = Arc::new(());
    let capture_weak = Arc::downgrade(&captured);
    let state_weak = Arc::downgrade(&state);
    let server =
        LynnServer::new()
            .await
            .with_state(state)
            .with_middleware(move |request, next: Next| {
                let captured = captured.clone();
                async move {
                    let result = next.run(request).await;
                    drop(captured);
                    result
                }
            });
    drop(server);
    wait_for(|| capture_weak.upgrade().is_none() && state_weak.upgrade().is_none()).await;
}

#[tokio::test]
async fn middleware_and_handler_panics_return_permits_without_closing_connection() {
    let _lock = LOCK.lock().await;
    for stage in 0..4 {
        let drops = Arc::new(AtomicUsize::new(0));
        let captured = drops.clone();
        let (addr, server) = configured(&1).await;
        let task = tokio::spawn(
            server
                .with_middleware(move |request: MiddlewareRequest, next: Next| {
                    let drops = captured.clone();
                    async move {
                        if request.method_id() == 9 {
                            return next.run(request).await;
                        }
                        let _probe = DropProbe(drops);
                        assert_ne!(stage, 0, "intentional middleware before panic");
                        let result = next.run(request).await;
                        assert_ne!(stage, 1, "intentional middleware after panic");
                        result
                    }
                })
                .add_router(1, move || async move {
                    assert_ne!(stage, 2, "intentional handler panic");
                    HandlerResult::new_without_send()
                })
                .add_router(2, |_: AppState<u64>| async {
                    HandlerResult::new_without_send()
                })
                .add_router(9, echo)
                .start(),
        );
        let mut peer = connect(addr).await;
        peer.write_all(&frame(if stage == 3 { 2 } else { 1 }))
            .await
            .unwrap();
        wait_for(|| drops.load(Ordering::SeqCst) == 1).await;
        peer.write_all(&frame(9)).await.unwrap();
        assert_eq!(receive(&mut peer).await, b"ok");
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
    }
}
