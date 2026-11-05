#![cfg(feature = "server")]

use std::{net::SocketAddr, sync::Arc, time::Duration};

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
    task::JoinHandle,
    time::{sleep, timeout},
};

static LOCK: Mutex<()> = Mutex::const_new(());

struct Gate {
    entered: Semaphore,
    release: Semaphore,
}

impl Gate {
    fn new() -> Self {
        Self {
            entered: Semaphore::new(0),
            release: Semaphore::new(0),
        }
    }

    async fn block(&self) {
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
    }

    async fn entered(&self) {
        timeout(Duration::from_secs(3), self.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
    }
}

async fn configured() -> (SocketAddr, LynnServer<'static>) {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .with_connection_rate_limit(&0)
        .with_max_connections_per_ip(&0)
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
    .unwrap()
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

async fn stop(task: JoinHandle<lynn_tcp::Result<()>>) {
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    wait_for(|| METRICS.connections.active.get() == 0.0).await;
}

async fn echo(context: RequestContext) -> HandlerResult {
    HandlerResult::new_with_send(1, "ok".into(), vec![context.peer_addr()]).unwrap()
}

#[tokio::test]
async fn processing_histogram_excludes_before_and_after_and_send_metrics_wait_for_write() {
    let _lock = LOCK.lock().await;
    let gates = Arc::new([Gate::new(), Gate::new(), Gate::new()]);
    let middleware_gates = gates.clone();
    let histogram = &METRICS.messages.processing_duration_seconds;
    let count = histogram.get_sample_count();
    let sum = histogram.get_sample_sum();
    let sent = METRICS.messages.sent_total.get();
    let bytes = METRICS.network.bytes_sent_total.get();
    let (addr, server) = configured().await;
    let task = tokio::spawn(
        server
            .with_state(gates.clone())
            .with_middleware(move |request, next: Next| {
                let gates = middleware_gates.clone();
                async move {
                    gates[0].block().await;
                    let result = next.run(request).await;
                    gates[2].block().await;
                    result
                }
            })
            .add_router(
                1,
                |context: RequestContext, gates: AppState<Arc<[Gate; 3]>>| async move {
                    gates[1].block().await;
                    echo(context).await
                },
            )
            .start(),
    );
    let mut peer = connect(addr).await;
    peer.write_all(&frame(1)).await.unwrap();
    gates[0].entered().await;
    let before = std::time::Instant::now();
    sleep(Duration::from_millis(250)).await;
    assert_eq!(histogram.get_sample_count(), count);
    let before_elapsed = before.elapsed().as_secs_f64();
    gates[0].release.add_permits(1);
    gates[1].entered().await;
    sleep(Duration::from_millis(30)).await;
    gates[1].release.add_permits(1);
    gates[2].entered().await;
    let handler_sum = histogram.get_sample_sum();
    let handler_seconds = handler_sum - sum;
    assert_eq!(histogram.get_sample_count(), count + 1);
    assert!(handler_seconds >= 0.025, "handler wait was not measured");
    assert!(
        handler_seconds < before_elapsed,
        "before wait entered the handler histogram"
    );
    sleep(Duration::from_millis(150)).await;
    assert_eq!(histogram.get_sample_sum(), handler_sum);
    assert_eq!(METRICS.messages.sent_total.get(), sent);
    assert_eq!(METRICS.network.bytes_sent_total.get(), bytes);
    gates[2].release.add_permits(1);
    assert_eq!(receive(&mut peer).await, b"ok");
    wait_for(|| METRICS.messages.sent_total.get() == sent + 1.0).await;
    assert_eq!(METRICS.network.bytes_sent_total.get(), bytes + 17.0);
    stop(task).await;
}

#[tokio::test]
async fn short_circuits_skip_handler_samples_and_only_unhandled_errors_count_as_drops() {
    let _lock = LOCK.lock().await;
    let handled = Arc::new(Semaphore::new(0));
    let captured = handled.clone();
    let histogram = &METRICS.messages.processing_duration_seconds;
    let count = histogram.get_sample_count();
    let dropped = METRICS.messages.dropped_total.get();
    let sent = METRICS.messages.sent_total.get();
    let (addr, server) = configured().await;
    let task = tokio::spawn(
        server
            .with_middleware(move |request: MiddlewareRequest, next: Next| {
                let handled = captured.clone();
                async move {
                    let result = match request.method_id() {
                        1 => HandlerResult::new_with_send(
                            1,
                            "rejected".into(),
                            vec![request.peer_addr()],
                        ),
                        2 => Ok(HandlerResult::new_without_send()),
                        3 => Err(LynnError::handler("unhandled middleware error")),
                        _ => next.run(request).await,
                    };
                    handled.add_permits(1);
                    result
                }
            })
            .add_router(1, echo)
            .add_router(2, echo)
            .add_router(3, echo)
            .add_router(4, echo)
            .start(),
    );
    let mut peer = connect(addr).await;
    peer.write_all(&frame(1)).await.unwrap();
    assert_eq!(receive(&mut peer).await, b"rejected");
    peer.write_all(&[frame(2), frame(3)].concat())
        .await
        .unwrap();
    timeout(Duration::from_secs(3), handled.acquire_many(3))
        .await
        .unwrap()
        .unwrap()
        .forget();
    wait_for(|| METRICS.messages.dropped_total.get() == dropped + 1.0).await;
    assert_eq!(histogram.get_sample_count(), count);
    assert_eq!(METRICS.messages.sent_total.get(), sent + 1.0);
    peer.write_all(&frame(4)).await.unwrap();
    assert_eq!(receive(&mut peer).await, b"ok");
    assert_eq!(histogram.get_sample_count(), count + 1);
    assert_eq!(METRICS.messages.dropped_total.get(), dropped + 1.0);
    stop(task).await;
}

#[tokio::test]
async fn handler_panic_missing_state_timeout_and_owner_cancel_each_record_one_sample() {
    let _lock = LOCK.lock().await;
    for kind in 0..4 {
        let gate = Arc::new(Gate::new());
        let histogram = &METRICS.messages.processing_duration_seconds;
        let count = histogram.get_sample_count();
        let (addr, server) = configured().await;
        let task = tokio::spawn(
            server
                .with_state(gate.clone())
                .with_middleware(move |request, next: Next| async move {
                    if kind == 2 {
                        return timeout(Duration::from_millis(80), next.run(request))
                            .await
                            .map_err(|_| LynnError::timeout("test handler deadline"))?;
                    }
                    next.run(request).await
                })
                .add_router(1, || async { panic!("intentional handler panic") })
                .add_router(2, |_: AppState<u64>| async {
                    HandlerResult::new_without_send()
                })
                .add_router(3, |gate: AppState<Arc<Gate>>| async move {
                    gate.block().await;
                    HandlerResult::new_without_send()
                })
                .start(),
        );
        let mut peer = connect(addr).await;
        peer.write_all(&frame(if kind < 2 { kind + 1 } else { 3 }))
            .await
            .unwrap();
        if kind == 3 {
            gate.entered().await;
            assert_eq!(histogram.get_sample_count(), count);
            stop(task).await;
        } else {
            wait_for(|| histogram.get_sample_count() == count + 1).await;
            stop(task).await;
        }
        wait_for(|| histogram.get_sample_count() == count + 1).await;
    }
}
