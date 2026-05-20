## Lynn_tcp

[![Crates.io](https://img.shields.io/crates/v/lynn_tcp)](https://crates.io/crates/lynn_tcp) [![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/cherish-ltt/lynn_tcp/blob/main/LICENSE) [![doc](https://docs.rs/lynn_tcp/badge.svg)](https://docs.rs/lynn_tcp/latest/lynn_tcp/) [![Downloads](https://img.shields.io/crates/d/lynn_tcp.svg)](https://crates.io/crates/lynn_tcp) [![Rust](https://img.shields.io/badge/rust-1.95%2B-blue)](https://www.rust-lang.org)

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
| `metrics_example` | `cargo run --example metrics_example --features metrics` | Prometheus metrics integration |

The **`echo_server_client`** and **`multi_route_service`** examples are the best starting points for understanding Client ↔ Server communication.

---

### Features

| Feature | Description | Default |
|---------|-------------|---------|
| `server` | TCP server with multi-route async handlers, heartbeat, connection management | ✅ |
| `client` | TCP client for sending/receiving messages | ✅ |
| `metrics` | Prometheus integration (17 production metrics, HTTP /metrics endpoint) | ✅ (via `server`) |

> **Note**: `metrics` is automatically enabled when `server` is selected. To use it independently: `features = ["metrics"]`.

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

#### 🔜 Planned

| Feature | Target Version | Status |
|---------|---------------|--------|
| TLS 1.3 support (rustls/tokio-rustls) | v2.1.0 | 🚧 Planning |
| Client auto-reconnection | v2.2.0 | 📝 Design |
| Middleware support | v2.3.0 | 💡 Idea |
| Scheduled tasks | TBD | 💡 Idea |
| Global database handle | TBD | 💡 Idea |

---

### Flow Chart

[![FlowChart](https://github.com/cherish-ltt/lynn_tcp/blob/main/FlowChart.png?raw=true)](https://github.com/cherish-ltt/lynn_tcp/blob/main/FlowChart.png?raw=true)

---

### Release Notes

See [version.md](https://github.com/cherish-ltt/lynn_tcp/blob/main/version.md) for full version history.

---

### Test Results

Platform: Debian 12.12 (4H4G) — 2025.10.21

- **model-1**: one request by one response
- **model-2**: concurrent `send request` and `recv response`
- total time: 15s

**lynn_tcp v1.1.x**

| Client Concurrency | model-1 (Responses/s) | model-2 (Responses/s) |
| :----------------- | :-------------------- | --------------------- |
| 256                | 182,879               | 80,499                |
| 512                | 249,135               | 61,370                |
| 1024               | 232,861               | 23,143                |
| 2048               | 185,735               | 16,468                |
| 4096               | 160,318               | 13,557                |

**lynn_tcp v1.2.x**

| Client Concurrency | model-1 (Responses/s) | model-2 (Responses/s) |
| :----------------- | :-------------------- | --------------------- |
| 256                | 64,630                | 492,889               |
| 512                | 182,296               | 300,550               |
| 1024               | 163,307               | 158,056               |
| 2048               | 131,346               | 71,263                |
| 4096               | 124,645               | 52,163                |

> 📊 Benchmarks for v2.0.0 are forthcoming. The architectural refactoring focuses on maintainability with no expected performance regression.

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

---

### License

This project is licensed under the MIT license.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Lynn_tcp by you shall be licensed as MIT, without any additional terms or conditions.
