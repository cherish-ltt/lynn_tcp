//! Standardized network benchmark for lynn_tcp (v2.0).
//!
//! Measures end-to-end throughput and latency of a lynn_tcp echo service on
//! the loopback interface. Two traffic models are supported, matching the
//! classic long-connection workloads the framework targets:
//!
//! - **Model 1 — request/response (ping-pong)**: every client keeps exactly
//!   one request in flight; the next request is sent only after the previous
//!   response arrived. Measures throughput *and* round-trip latency
//!   (avg/p50/p95/p99).
//! - **Model 2 — concurrent send/receive (pipelined)**: every client runs two
//!   tasks, one sending requests as fast as the bounded write channel allows,
//!   one receiving responses. Measures sustained throughput only.
//!
//! The server and all clients run in one process over `127.0.0.1`; every
//! cell first warms up (not counted), then measures for a fixed window.
//!
//! # Run
//!
//! ```bash
//! # Full standard matrix: models 1+2 × clients 64/256/1024/4096, 3s warmup + 10s measured per cell
//! cargo bench --bench benchmark
//!
//! # Custom run
//! cargo bench --bench benchmark -- --model 1 --clients 64,512 --duration 15 --payload 256 --json results.json
//! ```
//!
//! Results are printed as a Markdown table and (optionally) written as JSON
//! for archival.

use std::{
    fmt::Write as _,
    net::SocketAddr,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use lynn_tcp::{
    bytes::Bytes,
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_tcp_dependents::HandlerResult,
    tokio::time::timeout,
};

const METHOD_ID: u16 = 1;
/// Per-connection write channel capacity (bounds in-flight pipelined requests).
const CLIENT_CHANNEL_SIZE: usize = 8;
/// Grace period after the measurement window during which late responses are
/// still accepted (they were requested inside the window).
const DRAIN: Duration = Duration::from_millis(500);
/// Connection attempts per client (benchmarks must fail fast, not reconnect).
const CONNECT_ATTEMPTS: usize = 1;

// ── CLI ─────────────────────────────────────────────────────────────────

struct Args {
    models: Vec<u8>,
    client_levels: Vec<usize>,
    duration: Duration,
    warmup: Duration,
    payload: usize,
    json: Option<String>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            models: vec![1, 2],
            client_levels: vec![64, 256, 1024, 4096],
            duration: Duration::from_secs(10),
            warmup: Duration::from_secs(3),
            payload: 128,
            json: None,
        }
    }
}

const USAGE: &str = "lynn_tcp benchmark
Usage: cargo bench --bench benchmark -- [OPTIONS]

Options:
  --model <all|1|2>          Traffic model(s) to run            [default: all]
  --clients <LIST>           Comma separated concurrency levels  [default: 64,256,1024,4096]
  --duration <SECS>          Measured window per cell            [default: 10]
  --warmup <SECS>            Warmup window per cell (uncounted)  [default: 3]
  --payload <BYTES>          Request/response payload size       [default: 128]
  --json <PATH>              Write results as JSON to PATH
  -h, --help                 Print this help";

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            // `cargo bench` appends this flag to harness-less benchmarks.
            "--bench" => {},
            "--model" => {
                let v = it.next().ok_or("missing value for --model")?;
                args.models = match v.as_str() {
                    "all" => vec![1, 2],
                    "1" => vec![1],
                    "2" => vec![2],
                    other => return Err(format!("unknown model '{other}' (use all|1|2)")),
                };
            },
            "--clients" => {
                let v = it.next().ok_or("missing value for --clients")?;
                let mut levels = Vec::new();
                for part in v.split(',') {
                    let n: usize = part
                        .trim()
                        .parse()
                        .map_err(|_| format!("invalid client count '{part}'"))?;
                    if n == 0 {
                        return Err("client counts must be > 0".into());
                    }
                    levels.push(n);
                }
                if levels.is_empty() {
                    return Err("--clients needs at least one value".into());
                }
                args.client_levels = levels;
            },
            "--duration" => args.duration = parse_secs(&it.next(), "--duration")?,
            "--warmup" => args.warmup = parse_secs(&it.next(), "--warmup")?,
            "--payload" => {
                let v = it.next().ok_or("missing value for --payload")?;
                args.payload = v.parse().map_err(|_| format!("invalid payload '{v}'"))?;
                if args.payload == 0 {
                    return Err("--payload must be > 0".into());
                }
            },
            "--json" => args.json = Some(it.next().ok_or("missing value for --json")?),
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            },
            other => return Err(format!("unknown argument '{other}'\n\n{USAGE}")),
        }
    }
    Ok(args)
}

fn parse_secs(value: &Option<String>, flag: &str) -> Result<Duration, String> {
    let v = value.clone().ok_or(format!("missing value for {flag}"))?;
    let secs: u64 = v.parse().map_err(|_| format!("invalid seconds '{v}'"))?;
    if secs == 0 {
        return Err(format!("{flag} must be > 0"));
    }
    Ok(Duration::from_secs(secs))
}

// ── harness plumbing ────────────────────────────────────────────────────

/// One completed benchmark cell.
struct CellResult {
    model: u8,
    clients: usize,
    payload: usize,
    requests: u64,
    responses: u64,
    /// Round-trip latencies in nanoseconds (model 1 only).
    rtts_ns: Vec<u64>,
}

impl CellResult {
    fn throughput_rps(&self, duration: Duration) -> f64 {
        self.responses as f64 / duration.as_secs_f64()
    }

    fn latency_ms_report(&self) -> (Option<f64>, Option<u64>, Option<u64>, Option<u64>) {
        if self.rtts_ns.is_empty() {
            return (None, None, None, None);
        }
        let mut sorted = self.rtts_ns.clone();
        sorted.sort_unstable();
        let p = |frac: f64| sorted[((sorted.len() - 1) as f64 * frac) as usize];
        let avg = sorted.iter().sum::<u64>() as f64 / sorted.len() as f64;
        (
            Some(avg / 1e6),
            Some(p(0.50) / 1_000),
            Some(p(0.95) / 1_000),
            Some(p(0.99) / 1_000),
        )
    }
}

/// Raises the soft file-descriptor limit when needed so high-concurrency
/// cells do not fail on conservative defaults.
fn raise_fd_limit() {
    const TARGET: libc::rlim_t = 32_768;
    unsafe {
        let mut rl: libc::rlimit = std::mem::zeroed();
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rl) != 0 {
            return;
        }
        if rl.rlim_cur >= TARGET {
            return;
        }
        rl.rlim_cur = rl.rlim_max.min(TARGET);
        if libc::setrlimit(libc::RLIMIT_NOFILE, &rl) == 0 {
            println!(" raised fd soft limit to {}", rl.rlim_cur);
        }
    }
}

/// A benchmark server child process: one fresh server per cell, so cells
/// are fully isolated (no leftover connections, queued events, or busy
/// scheduler threads from previous cells can pollute a measurement).
struct BenchServer {
    addr: SocketAddr,
    child: std::process::Child,
}

impl Drop for BenchServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Polls until the server accepts TCP connections.
async fn wait_server(addr: SocketAddr) {
    for _ in 0..400 {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("echo server never became reachable at {addr}");
}

/// Spawns the echo server (`bench_echo_server` binary) as an independent
/// process and waits for its `READY <port>` announcement.
fn spawn_echo_server(max_connections: usize) -> BenchServer {
    let _ = max_connections; // the server binary owns its capacity settings
    let exe = env!("CARGO_BIN_EXE_bench_echo_server");
    let mut child = std::process::Command::new(exe)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn bench_echo_server");
    let stdout = child.stdout.take().expect("server stdout");
    let mut ready_line = String::new();
    {
        use std::io::BufRead as _;
        let mut reader = std::io::BufReader::new(stdout);
        reader
            .read_line(&mut ready_line)
            .expect("read READY line from server");
    }
    let port: u16 = ready_line
        .trim()
        .strip_prefix("READY ")
        .and_then(|rest| rest.parse().ok())
        .unwrap_or_else(|| panic!("unexpected server handshake line: '{ready_line}'"));
    BenchServer {
        addr: format!("127.0.0.1:{port}").parse().expect("addr"),
        child,
    }
}

async fn connect_client(addr: SocketAddr) -> LynnClient<'static> {
    // Connection storms can overflow the listen backlog or exhaust ephemeral
    // ports; retry a few times before giving up (setup is not measured).
    for attempt in 0..5 {
        let config = LynnClientConfigBuilder::new()
            .with_server_addr(addr)
            .expect("valid addr")
            .with_server_single_channel_size(&CLIENT_CHANNEL_SIZE)
            .with_reconnect_max_attempts(&CONNECT_ATTEMPTS)
            .build();
        let client = LynnClient::new_with_config(config).await.start().await;
        if client.is_connected() {
            return client;
        }
        tokio::time::sleep(Duration::from_millis(200 * (attempt as u64 + 1))).await;
    }
    panic!("benchmark client failed to connect to {addr} after 5 attempts");
}

/// Connects `clients` benchmark clients in bounded batches, so the connect
/// storm does not overflow the server's accept backlog.
async fn connect_clients(addr: SocketAddr, clients: usize) -> Vec<LynnClient<'static>> {
    const BATCH: usize = 64;
    let mut connected = Vec::with_capacity(clients);
    let mut remaining = clients;
    while remaining > 0 {
        let batch = remaining.min(BATCH);
        remaining -= batch;
        let handles: Vec<_> = (0..batch)
            .map(|_| tokio::spawn(connect_client(addr)))
            .collect();
        for handle in handles {
            connected.push(handle.await.expect("client task panicked"));
        }
    }
    connected
}

fn build_request(payload: usize) -> HandlerResult {
    HandlerResult::new_with_send_to_server(METHOD_ID, Bytes::from(vec![b'a'; payload]))
}

// ── model 1: ping-pong ──────────────────────────────────────────────────

/// Runs one model-1 client: one request in flight, measure every RTT.
async fn model1_client(
    mut client: LynnClient<'static>,
    request: HandlerResult,
    measure_start: Instant,
    deadline: Instant,
) -> (u64, Vec<u64>) {
    let mut responses = 0u64;
    let mut rtts = Vec::new();
    loop {
        let sent_at = Instant::now();
        if sent_at >= deadline {
            break;
        }
        if client.send_data(request.clone()).await.is_err() {
            break;
        }
        let wait = (deadline + DRAIN).saturating_duration_since(sent_at);
        match timeout(wait, client.get_receive_data()).await {
            Ok(Some(_)) => {
                if sent_at >= measure_start {
                    responses += 1;
                    rtts.push(sent_at.elapsed().as_nanos() as u64);
                }
            },
            // Timed out past the drain window or the connection died.
            _ => break,
        }
    }
    (responses, rtts)
}

async fn run_model1_cell(addr: SocketAddr, clients: usize, args: &Args) -> CellResult {
    let mut connected = connect_clients(addr, clients).await;
    let request = build_request(args.payload);
    let t0 = Instant::now();
    let measure_start = t0 + args.warmup;
    let deadline = t0 + args.warmup + args.duration;

    let handles: Vec<_> = connected
        .drain(..)
        .map(|client| {
            tokio::spawn(model1_client(
                client,
                request.clone(),
                measure_start,
                deadline,
            ))
        })
        .collect();

    let mut result = CellResult {
        model: 1,
        clients,
        payload: args.payload,
        requests: 0,
        responses: 0,
        rtts_ns: Vec::new(),
    };
    for handle in handles {
        let (responses, rtts) = handle.await.expect("model1 client panicked");
        result.responses += responses;
        result.requests += responses;
        result.rtts_ns.extend(rtts);
    }
    result
}

// ── model 2: concurrent send/receive ────────────────────────────────────

/// Sends requests as fast as the bounded write channel allows.
async fn model2_sender(
    tx: tokio::sync::mpsc::Sender<HandlerResult>,
    request: HandlerResult,
    measure_start: Instant,
    deadline: Instant,
) -> u64 {
    let mut requests = 0u64;
    loop {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        if tx.send(request.clone()).await.is_err() {
            break;
        }
        if now >= measure_start {
            requests += 1;
        }
    }
    requests
}

/// Receives responses until the drain window closes, counting those received
/// inside the measurement window.
async fn model2_receiver(
    mut client: LynnClient<'static>,
    measure_start: Instant,
    deadline: Instant,
) -> u64 {
    let mut responses = 0u64;
    loop {
        let now = Instant::now();
        if now >= deadline + DRAIN {
            break;
        }
        let wait = (deadline + DRAIN).saturating_duration_since(now);
        match timeout(wait, client.get_receive_data()).await {
            Ok(Some(_)) => {
                if now >= measure_start && now < deadline + DRAIN {
                    responses += 1;
                }
            },
            _ => break,
        }
    }
    responses
}

async fn run_model2_cell(addr: SocketAddr, clients: usize, args: &Args) -> CellResult {
    let mut connected = connect_clients(addr, clients).await;
    let request = build_request(args.payload);
    let t0 = Instant::now();
    let measure_start = t0 + args.warmup;
    let deadline = t0 + args.warmup + args.duration;

    let mut sender_handles = Vec::with_capacity(clients);
    let mut receiver_handles = Vec::with_capacity(clients);
    for mut client in connected.drain(..) {
        let tx = client
            .get_sender()
            .await
            .expect("connected client has a sender");
        sender_handles.push(tokio::spawn(model2_sender(
            tx,
            request.clone(),
            measure_start,
            deadline,
        )));
        receiver_handles.push(tokio::spawn(model2_receiver(
            client,
            measure_start,
            deadline,
        )));
    }

    let mut result = CellResult {
        model: 2,
        clients,
        payload: args.payload,
        requests: 0,
        responses: 0,
        rtts_ns: Vec::new(),
    };
    for handle in sender_handles {
        result.requests += handle.await.expect("model2 sender panicked");
    }
    for handle in receiver_handles {
        result.responses += handle.await.expect("model2 receiver panicked");
    }
    result
}

// ── reporting ───────────────────────────────────────────────────────────

fn print_markdown(results: &[CellResult], args: &Args) {
    println!();
    println!(
        "| Model | Clients | Payload (B) | Requests | Responses | Throughput (resp/s) | Avg RTT (ms) | p50 (ms) | p95 (ms) | p99 (ms) |"
    );
    println!(
        "|:------|--------:|------------:|---------:|----------:|--------------------:|-------------:|---------:|---------:|---------:|"
    );
    for r in results {
        let (avg, p50, p95, p99) = r.latency_ms_report();
        let ms = |v: Option<u64>| {
            v.map_or_else(|| "—".to_string(), |v| format!("{:.3}", v as f64 / 1e3))
        };
        let avg = avg.map_or_else(|| "—".to_string(), |v| format!("{v:.3}"));
        let name = if r.model == 1 {
            "1 (ping-pong)"
        } else {
            "2 (pipelined)"
        };
        println!(
            "| {name} | {} | {} | {} | {} | {:.0} | {avg} | {} | {} | {} |",
            r.clients,
            r.payload,
            r.requests,
            r.responses,
            r.throughput_rps(args.duration),
            ms(p50),
            ms(p95),
            ms(p99),
        );
    }
    println!();
}

fn write_json(path: &str, results: &[CellResult], args: &Args) -> std::io::Result<()> {
    let cores = std::thread::available_parallelism().map_or(0, |n| n.get());
    let mut out = String::with_capacity(1024 + results.len() * 256);
    let _ = writeln!(
        out,
        "{{\n  \"meta\": {{\n    \"crate_version\": \"{}\",\n    \"os\": \"{}\",\n    \"arch\": \"{}\",\n    \"logical_cores\": {cores},\n    \"topology\": \"loopback, echo server in an independent process (bench_echo_server binary)\",\n    \"payload_bytes\": {},\n    \"warmup_secs\": {},\n    \"duration_secs\": {},\n    \"client_channel_size\": {CLIENT_CHANNEL_SIZE},\n    \"unix_timestamp\": {}\n  }},\n  \"results\": [",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        args.payload,
        args.warmup.as_secs(),
        args.duration.as_secs(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    );
    for (index, r) in results.iter().enumerate() {
        let (avg, p50, p95, p99) = r.latency_ms_report();
        let latency_json = match (avg, p50, p95, p99) {
            (Some(avg), Some(p50), Some(p95), Some(p99)) => {
                format!(
                    "{{\"avg_ms\": {avg:.3}, \"p50_ms\": {}, \"p95_ms\": {}, \"p99_ms\": {}}}",
                    p50 as f64 / 1e3,
                    p95 as f64 / 1e3,
                    p99 as f64 / 1e3
                )
            },
            _ => "null".to_string(),
        };
        let _ = writeln!(
            out,
            "    {{\"model\": {}, \"clients\": {}, \"requests\": {}, \"responses\": {}, \"throughput_rps\": {:.1}, \"latency\": {}}}{}",
            r.model,
            r.clients,
            r.requests,
            r.responses,
            r.throughput_rps(args.duration),
            latency_json,
            if index + 1 == results.len() { "" } else { "," },
        );
    }
    let _ = writeln!(out, "  ]\n}}");
    std::fs::write(path, out)
}

// ── main ────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let args = match parse_args() {
        Ok(args) => args,
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(2);
        },
    };

    // Logging is off by default: tracing log storms (e.g. broken-pipe errors
    // from torn-down connections) serialize on the stdout lock and would
    // poison the measurement. Set LYNN_BENCH_LOGS=1 to debug.
    if std::env::var("LYNN_BENCH_LOGS").is_ok() {
        let _ = lynn_tcp::tracing_subscriber::fmt::try_init();
    }
    println!("lynn_tcp benchmark v{}", env!("CARGO_PKG_VERSION"));
    println!(
        "models {:?} | clients {:?} | payload {} B | warmup {}s | duration {}s",
        args.models,
        args.client_levels,
        args.payload,
        args.warmup.as_secs(),
        args.duration.as_secs()
    );
    raise_fd_limit();

    let max_clients = args.client_levels.iter().copied().max().unwrap_or(1);
    let server_capacity = max_clients * 2;

    let mut results = Vec::with_capacity(args.models.len() * args.client_levels.len());
    for &model in &args.models {
        for &clients in &args.client_levels {
            print!(
                "running model {model} with {clients} clients (warmup {}s + measured {}s)... ",
                args.warmup.as_secs(),
                args.duration.as_secs()
            );
            let started = Instant::now();
            let server = spawn_echo_server(server_capacity);
            let addr = server.addr;
            wait_server(addr).await;
            let result = if model == 1 {
                run_model1_cell(addr, clients, &args).await
            } else {
                run_model2_cell(addr, clients, &args).await
            };
            // Dropping the handle kills the server child process, giving
            // every cell a pristine server.
            drop(server);
            println!(
                "{:.0} resp/s ({} responses in {}s, cell took {:.1}s)",
                result.throughput_rps(args.duration),
                result.responses,
                args.duration.as_secs(),
                started.elapsed().as_secs_f64()
            );
            results.push(result);
        }
    }

    print_markdown(&results, &args);
    if let Some(path) = &args.json {
        if let Err(err) = write_json(path, &results, &args) {
            eprintln!("failed to write JSON to {path}: {err}");
            std::process::exit(1);
        }
        println!("JSON results written to {path}");
    }
}
