//! Standalone echo server used by the `benchmark` harness.
//!
//! Runs a lynn_tcp echo service as an independent process (like a real
//! deployment), so the load-generating benchmark client cannot interfere
//! with the server's scheduler. Prints `READY <port>` on stdout once the
//! listener is up; the benchmark harness kills the process after each cell.
//!
//! # Run (standalone)
//!
//! ```bash
//! cargo run --release --bin bench_echo_server -- --port 9177
//! ```

use std::net::TcpListener;

use lynn_tcp::{
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
};

const METHOD_ID: u16 = 1;

/// Static constants used to satisfy the `&'a` config builder lifetimes.
static MAX_CONNECTIONS: usize = 32768;
static TASKPOOL: usize = 128;
/// The default per-IP connection limit (10) and rate limit (100/s) make
/// loopback load tests impossible; both are disabled here.
static UNLIMITED_PER_IP: usize = 0;
static UNLIMITED_RATE: u64 = 0;

async fn echo_handler(input_buf_vo: InputBufVO) -> HandlerResult {
    let addr = match input_buf_vo.get_input_addr() {
        Some(addr) => addr,
        None => return HandlerResult::new_without_send(),
    };
    HandlerResult::new_with_send(METHOD_ID, input_buf_vo.get_all_bytes().into(), vec![addr])
        .expect("payload fits the protocol limit")
}

fn main() {
    let kcp = std::env::var("LYNN_BENCH_TRANSPORT").as_deref() == Ok("kcp");
    #[cfg(not(feature = "kcp"))]
    assert!(!kcp, "rebuild with --features kcp");
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(0);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("server runtime");
    runtime.block_on(async move {
        // Probe a free port first so the harness can learn it (the server
        // API owns the actual bind, so port 0 cannot be reported back).
        let port = if port == 0 {
            if kcp {
                std::net::UdpSocket::bind("127.0.0.1:0")
                    .expect("probe bind")
                    .local_addr()
                    .unwrap()
                    .port()
            } else {
                let probe = TcpListener::bind("127.0.0.1:0").expect("probe bind");
                probe.local_addr().expect("probe addr").port()
            }
        } else {
            port
        };
        let addr = format!("127.0.0.1:{port}");
        let config = LynnServerConfigBuilder::new()
            .with_addr(&addr)
            .expect("valid addr")
            .with_server_max_connections(Some(&MAX_CONNECTIONS))
            .with_server_max_taskpool_size(&TASKPOOL)
            .with_max_connections_per_ip(&UNLIMITED_PER_IP)
            .with_connection_rate_limit(&UNLIMITED_RATE);
        #[cfg(feature = "kcp")]
        let config = if kcp {
            config.with_kcp(match std::env::var("LYNN_BENCH_KCP_MODE").as_deref() {
                Ok("normal") => lynn_tcp::lynn_kcp::KcpConfig::normal_mode(),
                Ok("fast") | Err(_) => lynn_tcp::lynn_kcp::KcpConfig::default(),
                _ => panic!("LYNN_BENCH_KCP_MODE must be fast or normal"),
            })
        } else {
            config
        };
        let config = config.build();
        println!("READY {port}");
        use std::io::Write as _;
        std::io::stdout().flush().expect("flush READY line");

        LynnServer::new_with_config(config)
            .await
            .add_router(METHOD_ID, echo_handler)
            .start()
            .await
            .expect("server failed");
    });
}
