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

[English](https://github.com/cherish-ltt/lynn_tcp/blob/main/README.md) | 简体中文

**Lynn_tcp** 是一个轻量级、高性能的异步 TCP 框架，基于 Tokio 构建。

采用 **DDD（领域驱动设计）+ 洋葱架构**，将核心领域逻辑与基础设施实现分离，提升可维护性和可扩展性。

---

### 特点

- **轻量级**: 简洁易懂的代码，快速上手
- **并发与性能**: 基于 Tokio 优秀的异步运行时，轻松实现高并发多用户连接
- **低延迟**: 读写分离设计，最小化延迟
- **安全稳定**: Rust 强类型与内存安全保障
- **生产就绪**: 内置 Prometheus 指标、连接限制、速率限制、心跳管理

> **提示**: Lynn_tcp 主要用于<u>消息转发</u>和<u>长连接 TCP 游戏服务器</u>。
>
> 基于框架快速开发业务场景 — 自定义消息解析、加密、路由等。

---

### 架构

Lynn_tcp v2.0 遵循洋葱架构，分为三个清晰层次：

```
┌─────────────────────────────────────┐
│          接口层 (Interface)          │  ← 公开 API（向后兼容，无破坏性变更）
│   (lynn_server / lynn_client / ...) │
├─────────────────────────────────────┤
│          应用层 (Application)        │  ← 编排层：服务器/客户端启动流程
│   (LynnServer / LynnClient)         │
├─────────────────────────────────────┤
│          领域层 (Domain)             │  ← 纯业务逻辑
│   (Router / Handler / Model)        │
├─────────────────────────────────────┤
│        基础设施层 (Infrastructure)    │  ← 具体实现
│   (TCP / Metrics / Validation)      │
└─────────────────────────────────────┘
```

---

### 快速开始

#### 依赖配置

在 `Cargo.toml` 中添加：

**完整功能**（推荐）：

```toml
[dependencies]
lynn_tcp = "2"
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread"] }
tracing-subscriber = "0.3"
```

**仅服务器端**：

```toml
[dependencies]
lynn_tcp = { version = "2", features = ["server"] }
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread"] }
tracing-subscriber = "0.3"
```

**仅客户端**：

```toml
[dependencies]
lynn_tcp = { version = "2", features = ["client"] }
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread"] }
tracing-subscriber = "0.3"
```

#### 最小服务器示例

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
    println!("📡 收到 Ping");
    HandlerResult::new_without_send()
}

pub async fn echo_handler(input_buf_vo: InputBufVO) -> HandlerResult {
    let addr = input_buf_vo.get_input_addr().unwrap();
    println!("📨 来自: {}", addr);
    HandlerResult::new_without_send()
}

pub async fn broadcast_handler(clients_context: ClientsContext) -> HandlerResult {
    let addrs = clients_context.get_all_clients_addrs().await;
    HandlerResult::new_with_send(3, "大家好！".into(), addrs)
}
```

#### 自定义配置服务器

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

#### 最小客户端示例

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

    // 发送消息
    client.send_data(
        HandlerResult::new_with_send_to_server(1, "你好".into())
    ).await?;

    // 接收响应
    if let Some(response) = client.get_receive_data().await {
        println!("收到响应: {:?}", response.get_all_bytes());
    }

    Ok(())
}
```

---

### 示例

所有示例位于 [`examples/`](examples/) 目录。使用以下命令运行：

| 示例 | 命令 | 说明 |
|------|------|------|
| `basic_server` | `cargo run --example basic_server` | 默认配置服务器，展示 3 种 Handler 签名 |
| `custom_config_server` | `cargo run --example custom_config_server` | 通过 Builder 自定义服务器配置 |
| `custom_protocol` | `cargo run --example custom_protocol` | 自定义消息头尾标记 |
| `echo_server_client` | `cargo run --example echo_server_client` | **完整请求-响应循环**（客户端 ↔ 服务器双向通信） |
| `multi_route_service` | `cargo run --example multi_route_service` | **多路由分发与验证** |
| `custom_protocol_full` | `cargo run --example custom_protocol_full` | 自定义协议客户端配套 |
| `state_example` | `cargo run --example state_example` | **全局状态注入**（`AppState<T>`） |
| `tls_example` | `cargo run --example tls_example --features tls` | **TLS 1.3 加密**服务器 ↔ 客户端 |
| `metrics_example` | `cargo run --example metrics_example --features metrics` | Prometheus 指标集成演示 |

建议从 **`echo_server_client`** 和 **`multi_route_service`** 入手，它们最能展示客户端 ↔ 服务器的通信模式。

---

### 特性

| 特性 | 说明 | 默认启用 |
|------|------|---------|
| `server` | TCP 服务器，支持多路由异步 Handler、心跳、连接管理 | ✅ |
| `client` | TCP 客户端，支持消息收发与断线自动重连 | ✅ |
| `metrics` | Prometheus 集成（17 个生产级指标，HTTP /metrics 端点） | ✅（由 `server` 启用） |
| `tls` | 服务端与客户端 TLS 1.3 传输加密（rustls/ring） | ❌ 按需开启 |
| `seaorm` | 内置 SeaORM 支持：`with_db(...)` + `DbConn` 数据库句柄 | ❌ 按需开启 |

> **注意**: 选择 `server` 特性时 `metrics` 会自动启用。单独使用：`features = ["metrics"]`。`tls` 与 `seaorm` 默认关闭，必须显式启用。

---

### 可配置项（LynnServerConfigBuilder）

| 方法 | 说明 | 默认值 |
|------|------|--------|
| `with_addr()` | 服务器监听地址 | `0.0.0.0:9177` |
| `with_server_max_connections()` | 最大并发连接数 | `100_000` |
| `with_server_max_taskpool_size()` | 异步任务池大小（吞吐量） | `512` |
| `with_server_single_processs_permit()` | 最大并发处理任务数 | `1024` |
| `with_server_check_heart_interval()` | 心跳检测间隔（秒） | `60` |
| `with_server_check_heart_timeout_time()` | 心跳超时时间（秒） | `180` |
| `with_tcp_nodelay()` | TCP_NODELAY（禁用 Nagle 算法） | `true` |
| `with_tcp_keepalive_enabled()` | TCP 保活 | `true` |
| `with_tcp_keepalive_time_secs()` | 保活间隔（秒） | `120` |
| `with_message_header_mark()` | 自定义消息头标记 | `0x23D9` |
| `with_message_tail_mark()` | 自定义消息尾标记 | `0x1E27` |
| `with_max_connections_per_ip()` | 每 IP 最大连接数 | `100` |
| `with_connection_rate_limit()` | 连接速率限制（每秒，0=禁用） | `0` |
| `with_read_timeout_secs()` | 读取超时（秒，0=禁用） | `0` |
| `with_write_timeout_secs()` | 写入超时（秒，0=禁用） | `0` |
| `with_recv_buffer_size()` | 接收缓冲区（字节） | `65535` |
| `with_send_buffer_size()` | 发送缓冲区（字节） | `65535` |
| `with_tls()` *（feature `tls`）* | 使用 `TlsServerConfig` 开启 TLS 1.3 | 关闭 |
| `with_tls_cert_paths()` *（feature `tls`）* | 从 PEM 证书/私钥路径开启 TLS 1.3 | 关闭 |

### 可配置项（LynnClientConfigBuilder）

| 方法 | 说明 | 默认值 |
|------|------|--------|
| `with_server_addr()` | 要连接的服务器地址 | — |
| `with_server_single_channel_size()` | 单连接通道容量 | `64` |
| `with_server_check_heart_interval()` | 心跳发送间隔（秒） | `5` |
| `with_message_header_mark()` / `with_message_tail_mark()` | 自定义消息标记 | `9177` / `7719` |
| `with_reconnect_max_attempts()` | 每轮连接尝试次数（初始连接与每次重连） | `3` |
| `with_reconnect_interval_secs()` | 两次尝试的间隔（秒） | `1` |
| `with_connect_timeout_secs()` | 单次连接/TLS 握手超时（秒） | `3` |
| `with_tls()` *（feature `tls`）* | 使用 `TlsClientConfig` 开启 TLS 1.3（CA、SNI、mTLS） | 关闭 |

---

### TLS 1.3 传输加密（feature `tls`）

TLS **默认关闭**。启用 `tls` feature 并配置证书即手动开启 —— 仅支持 TLS 1.3（rustls + ring）：

```toml
[dependencies]
lynn_tcp = { version = "2", features = ["tls"] }
```

```rust
use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*, lynn_tls::*};

// 服务端：配置证书链 + 私钥（PEM 文件）即开启。
let config = LynnServerConfigBuilder::new()
    .with_addr("0.0.0.0:9177")?
    .with_tls_cert_paths("cert.pem", "key.pem") // 或：.with_tls(TlsServerConfig::new(...))
    .build();
// 双向 mTLS：TlsServerConfig::new("cert.pem", "key.pem").with_client_ca("client_ca.pem")
```

```rust
use lynn_tcp::{lynn_client::*, lynn_tls::*};

// 客户端：通过 CA 信任锚校验服务器证书。
let config = LynnClientConfigBuilder::new()
    .with_server_addr("127.0.0.1:9177")?
    .with_tls(
        TlsClientConfigBuilder::new()
            .with_ca_cert_path("ca.pem")
            .with_server_name("localhost") // 可选的 SNI 覆盖
            .build(),
    )?
    .build();
```

TLS 配置错误（文件缺失、证书私钥不配对）会在启动时立即报错；握手失败的连接会被直接关闭。可运行演示：`cargo run --example tls_example --features tls`。

---

### 全局状态（依赖注入）

通过 `with_state` 注册任意 `Send + Sync + 'static` 的共享值（如数据库句柄），Handler 参数声明 `AppState<T>` 即自动装配（类似 axum 的 State）。每个类型一个值，多个类型可共存。SeaORM 用户启用 `seaorm` feature 后可用 `with_db(...)` + `DbConn`：

```rust
use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*, lynn_state::AppState};

#[tokio::main]
async fn main() {
    let _ = LynnServer::new()
        .await
        .with_state(UserRepo::new()) // seaorm feature 下也可：.with_db(db)
        .add_router(1, find_user)
        .start()
        .await;
}

// `repo` 直接解引用为 &UserRepo —— 无全局静态、无需手动传递 Arc。
async fn find_user(repo: AppState<UserRepo>, input: InputBufVO) -> HandlerResult {
    let name = repo.find_user(42);
    HandlerResult::new_with_send(1, name.into(), vec![input.get_input_addr().unwrap()])
}
```

状态在请求处理时解析，因此 `with_state` 可以在 `add_router` 之前或之后调用 —— 只需在 `start()` 前注册即可。可运行演示：`cargo run --example state_example`。

---

### 客户端断线自动重连

客户端内置连接监督者：连接断开后自动重连 —— **默认尝试 3 次**，间隔 1 秒（均可配置）。用户侧收发通道跨重连复用，断线期间积压的过期帧会被丢弃，连接状态可随时查询：

```rust
let mut client = LynnClient::new_with_addr("127.0.0.1:9177").await.start().await;
if !client.is_connected() {
    eprintln!("服务器不可达");
}
```

---

### 规划

#### ✅ 已完成核心功能（v1.0.0+）

- ✅ TCP 服务器，支持多路由异步 Handler 分发
- ✅ TCP 客户端，支持消息收发
- ✅ 自定义消息头尾标记
- ✅ 客户端心跳检测与自动清理
- ✅ 异步任务路由服务
- ✅ Prometheus + Grafana 监控集成（v1.2.5）
- ✅ DDD + 洋葱架构重构（v2.0.0）
- ✅ 7 个可运行示例覆盖所有场景
- ✅ 服务端与客户端 TLS 1.3 传输加密（v2.0.0-rc.3）
- ✅ 客户端自动断线重连，尝试次数可配置（v2.0.0-rc.3）
- ✅ 全局状态注入（`AppState<T>`）+ 内置 SeaORM 支持（v2.0.0-rc.3）

#### 🔜 计划中

| 功能 | 目标版本 | 状态 |
|------|---------|------|
| 中间件支持 | v2.2.0 | 📝 设计中 |
| 定时任务 | 待定 | 💡 构思中 |

---

### 流程图

**v2.x — DDD + 洋葱架构**

[![FlowChart v2](https://github.com/cherish-ltt/lynn_tcp/blob/main/docs/FlowChart-v2.png?raw=true)](https://github.com/cherish-ltt/lynn_tcp/blob/main/docs/FlowChart-v2.png?raw=true)

<details>
<summary>v1.x 架构（历史版本）</summary>

[![FlowChart](https://github.com/cherish-ltt/lynn_tcp/blob/main/docs/FlowChart.png?raw=true)](https://github.com/cherish-ltt/lynn_tcp/blob/main/docs/FlowChart.png?raw=true)

</details>

---

### 版本介绍

查看完整版本历史：[docs/version.md](https://github.com/cherish-ltt/lynn_tcp/blob/main/docs/version.md)，各版本更新日志见 [docs/update_logs/](https://github.com/cherish-ltt/lynn_tcp/tree/main/docs/update_logs)。

---

### 测试结果

平台: Debian 12.12 (4H4G) — 2025.10.21

- **model-1**: 一次请求一次响应
- **model-2**: 并发发送与接收
- 测试时长: 15s

**lynn_tcp v1.1.x**

| 客户端并发数 | model-1 (请求/秒) | model-2 (请求/秒) |
| :----------- | :---------------- | ----------------- |
| 256          | 182,879           | 80,499            |
| 512          | 249,135           | 61,370            |
| 1024         | 232,861           | 23,143            |
| 2048         | 185,735           | 16,468            |
| 4096         | 160,318           | 13,557            |

**lynn_tcp v1.2.x**

| 客户端并发数 | model-1 (请求/秒) | model-2 (请求/秒) |
| :----------- | :---------------- | ----------------- |
| 256          | 64,630            | 492,889           |
| 512          | 182,296           | 300,550           |
| 1024         | 163,307           | 158,056           |
| 2048         | 131,346           | 71,263            |
| 4096         | 124,645           | 52,163            |

> 📊 v2.0.0 的基准测试数据即将发布。架构重构主要关注可维护性，预期不会带来性能回退。

---

### 常见问题

**问: v2.0.0 向后兼容 v1.x 吗？**

答: 是的。所有公共 API 签名保持不变。只需更新 `Cargo.toml` 中的版本号：`lynn_tcp = "2"`。

**问: 升级后需要修改代码吗？**

答: 不需要。重构完全是结构性的，没有修改任何行为。现有代码可以直接编译运行。

**问: 为什么从 v1.2.x 直接跳到 v2.0.0？**

答: DDD + 洋葱架构重构代表了重大的架构改进。尽管公共 API 完全兼容，但大版本号提升反映了这次变更的深度。

**问: 如何运行示例？**

答: 参见上方的[示例](#示例)表格。每个示例都可以通过 `cargo run --example <名称>` 运行。

**问: 如何启用指标监控？**

答: `server` 特性会自动启用 `metrics`。运行指标示例：`cargo run --example metrics_example --features metrics`。详细文档见 [METRICS.md](METRICS.md)。

**问: 如何启用 TLS？**

答: 启用 `tls` feature，服务端 Builder 调用 `.with_tls_cert_paths("cert.pem", "key.pem")`、客户端 Builder 调用 `.with_tls(...)` —— 仅支持 TLS 1.3，默认关闭。参见 [TLS 1.3 传输加密](#tls-13-传输加密feature-tls)。

**问: 如何让 Handler 访问数据库句柄？**

答: 在 `LynnServer` 上调用 `.with_state(db)`（`seaorm` feature 下也可用 `.with_db(db)`），Handler 声明 `AppState<T>` 参数即可。参见[全局状态](#全局状态依赖注入)。

**问: 客户端会自动重连吗？**

答: 会。断线后默认自动重试 3 次（间隔 1 秒），可通过 `with_reconnect_max_attempts` / `with_reconnect_interval_secs` 配置；用 `client.is_connected()` 实时查询连接状态。

---

### 开源协议

本项目采用双许可，您可任选其一：

- MIT 协议（[LICENSE](LICENSE)）
- Apache License, Version 2.0（[LICENSE-APACHE](LICENSE-APACHE)）

### 关于贡献

- 提交前请先阅读 [AGENTS.md](AGENTS.md) —— 它是本项目的"开发宪法"，定义了提交规范、CI 标准、格式化与 lint 规则、测试要求（`cargo-llvm-cov` 覆盖率 ≥ 80%）以及发布流程。所有 pull request 与代码审查均参照其执行。
- 提交前请在本地运行：`cargo fmt --all`、`cargo clippy --all-targets -- -D warnings`、`cargo test`。
- 修改文档时请保持 `README.md` 与 `README_ZH.md` 同步。

除非您另有明确说明，否则您有意提交以包含在 Lynn_tcp 中的任何贡献都应被许可为 MIT OR Apache-2.0，无需任何额外的条款或条件。
