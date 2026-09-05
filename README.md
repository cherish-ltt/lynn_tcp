<div align="center">
<img src="docs/logo.png" alt="lynn_tcp logo" width="280"/>
<h1>Lynn_tcp</h1>
<p>
  <a href="https://github.com/cherish-ltt/lynn_tcp/actions/workflows/rust-ci.yml">
    <img src="https://img.shields.io/github/actions/workflow/status/cherish-ltt/lynn_tcp/rust-ci.yml?branch=main" alt="Build Status"/>
  </a>
  <a href="https://crates.io/crates/lynn_tcp">
    <img src="https://img.shields.io/crates/v/lynn_tcp.svg" alt="crates.io version"/>
  </a>
  <a href="https://docs.rs/lynn_tcp">
    <img src="https://docs.rs/lynn_tcp/badge.svg" alt="documentation"/>
  </a>
  <a href="https://github.com/cherish-ltt/lynn_tcp/blob/main/LICENSE">
    <img src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg" alt="license: MIT OR Apache-2.0"/>
  </a>
  <a href="https://www.rust-lang.org">
    <img src="https://img.shields.io/badge/rust-1.98.1+-orange.svg" alt="rust version"/>
  </a>
</p>
</div>

English | [简体中文](https://github.com/cherish-ltt/lynn_tcp/blob/main/README_ZH.md)

**Lynn_tcp** is a lightweight, high-performance asynchronous TCP framework built on Tokio.

It adopts a **DDD (Domain-Driven Design) + Onion Architecture**, separating core domain logic from infrastructure concerns for better maintainability and extensibility.

---

### Keywords

- **Lightweight**: Concise code that is easy to learn and use
- **Concurrent & Performance**: Built on Tokio's excellent async runtime for high-concurrency multi-user connections
- **Low Latency**: Read-write separation design for minimal latency
- **Security**: Strong typing and memory safety guarantees of Rust
- **Production Ready**: Prometheus metrics, connection limiting, rate limiting, and heartbeat management built-in

> **Tips**: Lynn_tcp is designed for <u>message forwarding</u> and <u>long-lived TCP game servers</u>.
>
> Quickly develop business scenarios with the framework — customize message parsing, encryption, routing, and more.

---

### Architecture

Lynn_tcp v2.0 is organized into three clean layers following the Onion Architecture pattern:

```
┌─────────────────────────────────────┐
│         Interface Layer             │  ← Public API (no breaking changes)
│   (lynn_server / lynn_client / ...) │
├─────────────────────────────────────┤
│         Application Layer           │  ← Orchestration: server/client startup
│   (LynnServer / LynnClient)         │
├─────────────────────────────────────┤
│          Domain Layer               │  ← Pure business logic
│   (Router / Handler / Model)        │
├─────────────────────────────────────┤
│       Infrastructure Layer          │  ← Concrete implementations
│   (TCP / Metrics / Validation)      │
└─────────────────────────────────────┘
```

---

### Quick Start

#### Dependencies

Add to your `Cargo.toml`:

**Full features** (recommended):

```toml
[dependencies]
lynn_tcp = "2"
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread"] }
tracing-subscriber = "0.3"
```

**Server only**:

```toml
[dependencies]
lynn_tcp = { version = "2", features = ["server"] }
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread"] }
tracing-subscriber = "0.3"
```

**Client only**:

```toml
[dependencies]
lynn_tcp = { version = "2", features = ["client"] }
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread"] }
tracing-subscriber = "0.3"
```

#### Minimal Server

```rust
use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let _ = LynnServer::new()
        .await
        .add_router(1, ping_handler)
        .add_router(2, echo_handler)
        .add_router(3, broadcast_handler)
        .start()
        .await;

    Ok(())
}

pub async fn ping_handler() -> HandlerResult {
    println!("📡 Ping received");
    HandlerResult::new_without_send()
}

pub async fn echo_handler(input_buf_vo: InputBufVO) -> HandlerResult {
    let addr = input_buf_vo.get_input_addr().unwrap();
    println!("📨 Echo from: {}", addr);
    HandlerResult::new_without_send()
}

pub async fn broadcast_handler(clients_context: ClientsContext) -> HandlerResult {
    let addrs = clients_context.get_all_clients_addrs().await;
    HandlerResult::new_with_send(3, "hello everyone!".into(), addrs)
}
```

#### Server with Custom Config

```rust
use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let config = LynnServerConfigBuilder::new()
        .with_addr("0.0.0.0:9876")?
        .with_server_max_connections(Some(&500))
        .with_server_max_taskpool_size(&256)
        .with_tcp_nodelay(&true)
        .with_tcp_keepalive_enabled(&true)
        .with_tcp_keepalive_time_secs(&120)
        .build();

    let _ = LynnServer::new_with_config(config)
        .await
        .add_router(1, my_handler)
        .start()
        .await;

    Ok(())
}

pub async fn my_handler() -> HandlerResult {
    HandlerResult::new_without_send()
}
```

#### Minimal Client

```rust
use lynn_tcp::{
    lynn_client::LynnClient,
    lynn_tcp_dependents::*,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let mut client = LynnClient::new_with_addr("127.0.0.1:9177")
            .await
            .start()
            .await;

    // Send a message
    client.send_data(
        HandlerResult::new_with_send_to_server(1, "hello".into())
    ).await?;

    // Receive response
    if let Some(response) = client.get_receive_data().await {
        println!("Received: {:?}", response.get_all_bytes());
    }

    Ok(())
}
```

---

### Examples

All examples are located in the [`examples/`](examples/) directory. Run them with:

| Example | Command | Description |
|---------|---------|-------------|
| `basic_server` | `cargo run --example basic_server` | Default server with 3 handler signatures |
| `custom_config_server` | `cargo run --example custom_config_server` | Server with custom Builder configuration |
| `custom_protocol` | `cargo run --example custom_protocol` | Custom message header/tail marks |
| `echo_server_client` | `cargo run --example echo_server_client` | **Full request-response cycle** (Client ↔ Server) |
| `multi_route_service` | `cargo run --example multi_route_service` | **Multi-route dispatch and verification** |
| `custom_protocol_full` | `cargo run --example custom_protocol_full` | Custom protocol with client support |
| `state_example` | `cargo run --example state_example` | **Global state injection** (`AppState<T>`) |
| `tls_example` | `cargo run --example tls_example --features tls` | **TLS 1.3 encrypted** server ↔ client |
| `metrics_example` | `cargo run --example metrics_example --features metrics` | Prometheus metrics integration |

The **`echo_server_client`** and **`multi_route_service`** examples are the best starting points for understanding Client ↔ Server communication.

---

### Features

| Feature | Description | Default |
|---------|-------------|---------|
| `server` | TCP server with multi-route async handlers, heartbeat, connection management | ✅ |
| `client` | TCP client for sending/receiving messages, automatic reconnection | ✅ |
| `metrics` | Prometheus integration (17 production metrics, HTTP /metrics endpoint) | ✅ (via `server`) |
| `tls` | TLS 1.3 transport encryption for server & client (rustls/ring) | ❌ opt-in |
| `seaorm` | Built-in SeaORM support: `with_db(...)` + `DbConn` state handle | ❌ opt-in |

> **Note**: `metrics` is automatically enabled when `server` is selected. To use it independently: `features = ["metrics"]`. `tls` and `seaorm` are disabled by default and must be enabled explicitly.

---

### Available Configuration (LynnServerConfigBuilder)

| Method | Description | Default |
|--------|-------------|---------|
| `with_addr()` | Server listen address | `0.0.0.0:9177` |
| `with_server_max_connections()` | Max concurrent connections | `100_000` |
| `with_server_max_taskpool_size()` | Async task pool size (throughput) | `512` |
| `with_server_single_processs_permit()` | Max concurrent processing tasks | `1024` |
| `with_server_check_heart_interval()` | Heartbeat check interval (s) | `60` |
| `with_server_check_heart_timeout_time()` | Heartbeat timeout (s) | `180` |
| `with_tcp_nodelay()` | TCP_NODELAY (disable Nagle) | `true` |
| `with_tcp_keepalive_enabled()` | TCP keep-alive | `true` |
| `with_tcp_keepalive_time_secs()` | Keep-alive interval (s) | `120` |
| `with_message_header_mark()` | Custom message header mark | `0x23D9` |
| `with_message_tail_mark()` | Custom message tail mark | `0x1E27` |
| `with_max_connections_per_ip()` | Max connections per IP | `100` |
| `with_connection_rate_limit()` | Connection rate (per second) | `0` (disabled) |
| `with_read_timeout_secs()` | Read timeout (s, 0 = disabled) | `0` |
| `with_write_timeout_secs()` | Write timeout (s, 0 = disabled) | `0` |
| `with_recv_buffer_size()` | Receive buffer (bytes) | `65535` |
| `with_send_buffer_size()` | Send buffer (bytes) | `65535` |
| `with_tls()` *(feature `tls`)* | Enable TLS 1.3 with a `TlsServerConfig` | off |
| `with_tls_cert_paths()` *(feature `tls`)* | Enable TLS 1.3 from PEM cert/key paths | off |

### Available Configuration (LynnClientConfigBuilder)

| Method | Description | Default |
|--------|-------------|---------|
| `with_server_addr()` | Server address to connect to | — |
| `with_server_single_channel_size()` | Per-connection channel capacity | `64` |
| `with_server_check_heart_interval()` | Heartbeat send interval (s) | `5` |
| `with_message_header_mark()` / `with_message_tail_mark()` | Custom message marks | `9177` / `7719` |
| `with_reconnect_max_attempts()` | Connect attempts per session (initial connect & each reconnect) | `3` |
| `with_reconnect_interval_secs()` | Delay between attempts (s) | `1` |
| `with_connect_timeout_secs()` | Per-attempt connect/TLS-handshake timeout (s) | `3` |
| `with_tls()` *(feature `tls`)* | Enable TLS 1.3 with a `TlsClientConfig` (CA, SNI, mTLS) | off |

---

### TLS 1.3 Encryption (feature `tls`)

TLS is **disabled by default**. Enable the `tls` feature and attach certificates — TLS 1.3 only (rustls + ring):

```toml
[dependencies]
lynn_tcp = { version = "2", features = ["tls"] }
```

```rust
use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*, lynn_tls::*};

// Server: opt in with a certificate chain + private key (PEM files).
let config = LynnServerConfigBuilder::new()
    .with_addr("0.0.0.0:9177")?
    .with_tls_cert_paths("cert.pem", "key.pem") // or: .with_tls(TlsServerConfig::new(...))
    .build();
// Mutual TLS: TlsServerConfig::new("cert.pem", "key.pem").with_client_ca("client_ca.pem")
```

```rust
use lynn_tcp::{lynn_client::*, lynn_tls::*};

// Client: verify the server against a CA trust anchor.
let config = LynnClientConfigBuilder::new()
    .with_server_addr("127.0.0.1:9177")?
    .with_tls(
        TlsClientConfigBuilder::new()
            .with_ca_cert_path("ca.pem")
            .with_server_name("localhost") // optional SNI override
            .build(),
    )?
    .build();
```

Miss-configured TLS fails fast (missing files, invalid pairs) and handshake failures simply drop the offending connection. Run the runnable demo: `cargo run --example tls_example --features tls`.

---

### Global State (Dependency Injection)

Register any `Send + Sync + 'static` value once and extract it in handler parameters through `AppState<T>` (axum-style). One value per type; several types can coexist. SeaORM users can enable the `seaorm` feature and use `with_db(...)` + `DbConn`:

```rust
use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*, lynn_state::AppState};

#[tokio::main]
async fn main() {
    let _ = LynnServer::new()
        .await
        .with_state(UserRepo::new()) // or .with_db(db) with feature "seaorm"
        .add_router(1, find_user)
        .start()
        .await;
}

// `repo` derefs to &UserRepo — no global statics, no manual Arc plumbing.
async fn find_user(repo: AppState<UserRepo>, input: InputBufVO) -> HandlerResult {
    let name = repo.find_user(42);
    HandlerResult::new_with_send(1, name.into(), vec![input.get_input_addr().unwrap()])
}
```

State is resolved per request, so `with_state` may be called before or after `add_router` — just register before `start()`. Run the runnable demo: `cargo run --example state_example`.

---

### Client Automatic Reconnection

The client supervises its own connection: when it drops, it reconnects automatically — **3 attempts by default**, 1s apart (configurable). User-facing channels survive reconnections, stale queued frames are discarded, and the live state is one call away:

```rust
let mut client = LynnClient::new_with_addr("127.0.0.1:9177").await.start().await;
if !client.is_connected() {
    eprintln!("server unreachable");
}
```

---

### Road Map

#### ✅ Core Features (v1.0.0+)

- ✅ TCP Server with multi-route async handler dispatch
- ✅ TCP Client with message send/receive
- ✅ Custom message header/tail marks
- ✅ Automatic client heartbeat & cleanup
- ✅ Asynchronous task routing service
- ✅ Prometheus + Grafana monitoring (v1.2.5)
- ✅ DDD + Onion Architecture refactoring (v2.0.0)
- ✅ 7 runnable examples covering all scenarios
- ✅ TLS 1.3 transport encryption for server & client (v2.0.0-rc.3)
- ✅ Client automatic reconnection with configurable attempts (v2.0.0-rc.3)
- ✅ Global state injection (`AppState<T>`) + built-in SeaORM support (v2.0.0-rc.3)

#### 🔜 Planned

| Feature | Target Version | Status |
|---------|---------------|--------|
| Middleware support | v2.2.0 | 📝 Design |
| Scheduled tasks | TBD | 💡 Idea |

---

### Flow Chart

**v2.x — DDD + Onion Architecture**

[![FlowChart v2](https://github.com/cherish-ltt/lynn_tcp/blob/main/docs/FlowChart-v2.png?raw=true)](https://github.com/cherish-ltt/lynn_tcp/blob/main/docs/FlowChart-v2.png?raw=true)

<details>
<summary>v1.x architecture (historical)</summary>

[![FlowChart](https://github.com/cherish-ltt/lynn_tcp/blob/main/docs/FlowChart.png?raw=true)](https://github.com/cherish-ltt/lynn_tcp/blob/main/docs/FlowChart.png?raw=true)

</details>

---

### Release Notes

See [docs/version.md](https://github.com/cherish-ltt/lynn_tcp/blob/main/docs/version.md) for full version history, and per-version changelogs under [docs/update_logs/](https://github.com/cherish-ltt/lynn_tcp/tree/main/docs/update_logs).

---

### Benchmarks

Starting with v2.0.0-rc.3, Lynn_tcp ships a **standardized, reusable benchmark harness** (`benches/benchmark.rs` + the standalone `bench_echo_server` binary). The server runs as an independent process (like a real deployment), every concurrency level starts from a fresh server process, and each cell measures a fixed window after a discarded warmup. The pre-2.0 tables were produced by an unrecoverable one-off script and have been retired.

**Traffic models**

- **Model 1 — request/response (ping-pong)**: each client keeps exactly one request in flight and measures every round-trip; reports throughput **and** RTT percentiles.
- **Model 2 — concurrent send/receive (pipelined)**: each client runs one sending and one receiving task with a bounded in-flight window (8); reports sustained throughput.

**Standard matrix**: models 1+2 × client concurrency 64 / 256 / 1024 / 4096 (logarithmic 4× steps, covering the latency-bound and saturation regimes), echo payload 128 B, 3 s warmup + 10 s measured per cell.

**Reproduce**

```bash
cargo bench --bench benchmark -- --json results.json
# customize: --model all|1|2  --clients 64,256  --duration 15  --warmup 3  --payload 128
```

**Standard run for a new version / machine (maintainers)**

```bash
git pull && cargo bench --bench benchmark -- --json docs/benchmark/v<version>-<machine>.json
```

Before running, make sure the file-descriptor limit allows the highest concurrency level (`ulimit -n 65535`; the harness also auto-raises it up to the hard limit). Archive the JSON under `docs/benchmark/` named `v<version>-<machine>.json`, then update the results table in **both** READMEs in the same commit (see [AGENTS.md](AGENTS.md) §10.3).

**lynn_tcp v2.0.0-rc.3** — Apple M1 Pro (8 logical cores: 6 performance + 2 efficiency, 16 GB), macOS, loopback — 2026.09.05 ([raw JSON](docs/benchmark/v2.0.0-rc.3-apple-m1-pro.json))

| Model | Clients | Payload (B) | Throughput (resp/s) | Avg RTT (ms) | p50 (ms) | p95 (ms) | p99 (ms) |
|:------|--------:|------------:|--------------------:|-------------:|---------:|---------:|---------:|
| 1 (ping-pong) | 64   | 128 | **129,479** | 0.494 | 0.446 | 0.855 | 1.303 |
| 1 (ping-pong) | 256  | 128 | **136,350** | 1.877 | 1.682 | 3.503 | 5.064 |
| 1 (ping-pong) | 1024 | 128 | **144,873** | 7.067 | 5.679 | 16.497 | 22.707 |
| 1 (ping-pong) | 4096 | 128 | **127,680** | 32.065 | 29.457 | 80.264 | 112.598 |
| 2 (pipelined) | 64   | 128 | **103,459** | — | — | — | — |
| 2 (pipelined) | 256  | 128 | **131,482** | — | — | — | — |
| 2 (pipelined) | 1024 | 128 | **113,253** | — | — | — | — |
| 2 (pipelined) | 4096 | 128 | **69,145** | — | — | — | — |

> 📊 Numbers are loopback, single-machine measurements where client load generation shares the CPU with the server — treat them as relative comparisons between versions, not absolute capacity. Model-1 RTT grows with concurrency as queues form (classic queueing behavior); Model-2 throughput saturates near 1024 clients on this machine.

<details>
<summary>Fuzzy comparison with v1.1.x / v1.2.x (different harness &amp; hardware — trend-level only)</summary>

> The v1.x numbers were produced by an unrecoverable one-off script on Debian 12 (4-core / 4 GB server); the v2.0 numbers come from the standardized harness above on Apple M1 Pro (8 logical cores / 16 GB). Hardware, OS and methodology all differ — read the tables as trends, not absolutes.

**Model 1 — request/response (resp/s)**

| Clients | v1.1.x | v1.2.x | v2.0.0-rc.3 |
|--------:|-------:|-------:|------------:|
| 256     | 182,879 | 64,630 | **136,350** |
| 1024    | 232,861 | 163,307 | **144,873** |
| 4096    | 160,318 | 124,645 | **127,680** |

**Model 2 — concurrent send/receive (resp/s)**

| Clients | v1.1.x | v1.2.x | v2.0.0-rc.3 |
|--------:|-------:|-------:|------------:|
| 256     | 80,499 | 492,889 | **131,482** |
| 1024    | 23,143 | 158,056 | **113,253** |
| 4096    | 13,557 | 52,163 | **69,145** |

**Takeaways**

- Throughput stays in the same order of magnitude — no recognizable regression. At 4096 clients both models beat both v1.x generations (Model-2: 5.1× v1.1.x, 1.3× v1.2.x).
- The clearest v2.0 win is **stability**: v1.x collapses at high concurrency (Model-2 drops 83–89% from its peak), while v2.0 degrades only ~12% (Model-1) and ~47% (Model-2 — with a bounded 8-deep in-flight window, versus v1.x's unbounded burst).
- Model-2 semantics differ (v1.x appears to burst without in-flight limits; v2.0 uses a fixed 8-deep pipeline), so treat that table as directional.
- v2.0 additionally reports RTT percentiles, which v1.x never measured.

</details>

---

### FAQ

**Q: Is v2.0.0 backward compatible with v1.x?**

A: Yes. All public API signatures remain unchanged. You only need to update the version in `Cargo.toml`: `lynn_tcp = "2"`.

**Q: Do I need to change my code after upgrading?**

A: No. The refactoring was purely structural — no behavior was modified. Your existing code will compile and run as before.

**Q: Why jump from v1.2.x to v2.0.0?**

A: The DDD + Onion Architecture restructuring represents a significant architectural improvement. The major version bump reflects this depth of change, even though the public API is fully compatible.

**Q: How do I run the examples?**

A: See the [Examples](#examples) section above. Each example can be run with `cargo run --example <name>`.

**Q: How do I enable metrics?**

A: Metrics are automatically enabled with the `server` feature. To run the metrics example: `cargo run --example metrics_example --features metrics`. See [METRICS.md](METRICS.md) for detailed documentation.

**Q: How do I enable TLS?**

A: Enable the `tls` feature and call `.with_tls_cert_paths("cert.pem", "key.pem")` on the server builder and `.with_tls(...)` on the client builder — TLS 1.3 only, off by default. See the [TLS 1.3 Encryption](#tls-13-encryption-feature-tls) section.

**Q: How do I share a database handle with handlers?**

A: Call `.with_state(db)` (or `.with_db(db)` with the `seaorm` feature) on `LynnServer` and take an `AppState<T>` parameter in the handler. See [Global State](#global-state-dependency-injection).

**Q: Does the client reconnect automatically?**

A: Yes. After a disconnect it retries 3 times (1s apart) by default, configurable via `with_reconnect_max_attempts` / `with_reconnect_interval_secs`. Check the live state with `client.is_connected()`.

---

### License

This project is dual-licensed under either of:

- MIT license ([LICENSE](LICENSE))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

### Contribution

- Please read [AGENTS.md](AGENTS.md) first — it is the project's development constitution, defining commit conventions, CI standards, formatting/lint rules, testing requirements (coverage ≥ 80% via `cargo-llvm-cov`), and the release workflow. All pull requests and code reviews are checked against it.
- Before submitting, run locally: `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- Keep `README.md` and `README_ZH.md` in sync when changing documentation.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Lynn_tcp by you shall be licensed as MIT OR Apache-2.0, without any additional terms or conditions.
