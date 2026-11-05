//! Independent TCP server for `benches/middleware_compare.py`.
//! Counts whole-process allocations; no tracing subscriber is installed.

#[path = "support/allocation_counter.rs"]
mod allocation_counter;

use std::{
    io::{BufRead, Write},
    sync::atomic::{AtomicU64, Ordering},
};

use lynn_tcp::{
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
};

static HANDLED: AtomicU64 = AtomicU64::new(0);
static MAX_CONNECTIONS: usize = 32768;
static TASKPOOL: usize = 128;
static UNLIMITED_PER_IP: usize = 0;
static UNLIMITED_RATE: u64 = 0;

async fn echo(input: InputBufVO) -> HandlerResult {
    HANDLED.fetch_add(1, Ordering::Relaxed);
    HandlerResult::new_with_send(
        1,
        input.get_all_bytes().into(),
        vec![input.get_input_addr().expect("network request")],
    )
    .expect("echo fits the protocol limit")
}

// The comparison script replaces this block with plain registration for the old source baseline.
// BEGIN MIDDLEWARE
mod middleware {
    use std::time::{Duration, Instant};

    use lynn_tcp::{
        LynnError,
        lynn_middleware::{MiddlewareRequest, Next},
    };

    use super::*;

    struct Principal;

    async fn pass(request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
        next.run(request).await
    }

    async fn authorize(
        mut request: MiddlewareRequest,
        next: Next,
    ) -> lynn_tcp::Result<HandlerResult> {
        if request.payload().first() != Some(&b'a') {
            return Err(LynnError::Handler("invalid public benchmark marker".into()));
        }
        request.insert(Principal);
        next.run(request).await
    }

    async fn deadline(request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
        tokio::time::timeout(Duration::from_secs(1), next.run(request))
            .await
            .map_err(|_| LynnError::Timeout("benchmark deadline".into()))?
    }

    async fn observe(request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
        let started = Instant::now();
        let result = next.run(request).await;
        tracing::trace!(
            elapsed_ns = started.elapsed().as_nanos(),
            ok = result.is_ok()
        );
        std::hint::black_box(started.elapsed());
        result
    }

    pub(super) fn configure<'a>(server: LynnServer<'a>, mode: &str) -> LynnServer<'a> {
        match mode {
            "empty" => server,
            "pass" => server.with_middleware(pass),
            "composed" => server
                .with_middleware(observe)
                .with_middleware(deadline)
                .with_middleware(authorize),
            _ => panic!("LYNN_MIDDLEWARE_MODE must be empty, pass or composed"),
        }
    }
}
// END MIDDLEWARE

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::env::var("LYNN_BENCH_SERVER_STATS")?;
    let mode = std::env::var("LYNN_MIDDLEWARE_MODE")?;
    let addr = std::net::TcpListener::bind("127.0.0.1:0")?.local_addr()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()?;
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let control = std::thread::spawn(move || {
        let mut line = String::new();
        let result = std::io::stdin().lock().read_line(&mut line);
        let _ = stop.send(());
        result.and_then(|_| {
            if line.trim() == "STOP" {
                Ok(())
            } else {
                Err(std::io::Error::other("missing STOP command"))
            }
        })
    });
    let result = runtime.block_on(async {
        let address = addr.to_string();
        let config = LynnServerConfigBuilder::new()
            .with_addr(&address)?
            .with_server_max_connections(Some(&MAX_CONNECTIONS))
            .with_server_max_taskpool_size(&TASKPOOL)
            .with_max_connections_per_ip(&UNLIMITED_PER_IP)
            .with_connection_rate_limit(&UNLIMITED_RATE)
            .build();
        let server = middleware::configure(
            LynnServer::new_with_config(config)
                .await
                .add_router(1, echo),
            &mode,
        );
        println!("READY {}", addr.port());
        std::io::stdout().flush()?;
        tokio::select! {
            result = server.start() => result,
            _ = stopped => Ok(()),
        }
    });
    drop(runtime);
    result?;
    control.join().expect("control thread panicked")?;
    let stats = allocation_counter::snapshot(HANDLED.load(Ordering::Relaxed))?;
    std::fs::write(output, stats)?;
    Ok(())
}
