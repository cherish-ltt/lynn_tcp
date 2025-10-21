## Lynn_tcp

[![Crates.io](https://img.shields.io/crates/v/lynn_tcp)](https://crates.io/crates/lynn_tcp)  [![MIT licensed](https://img.shields.io/badge/license-MIT-blue.svg)](https://github.com/cherish-ltt/lynn_tcp/blob/main/LICENSE) [![doc](https://docs.rs/axum/badge.svg)](https://docs.rs/lynn_tcp/latest/lynn_tcp/) [![Downloads](https://img.shields.io/crates/d/lynn_tcp.svg)](https://crates.io/crates/lynn_tcp)

`Lynn_tcp` 是一个轻量级的tcp服务器框架

------

### 特点

- **轻量级**: 简洁的代码，更容易学习和使用

- **并发和性能**: 基于Tokio出色的异步性能，更便捷的实现多用户链路的高并发处理能力

- **低延迟,高效率**: 采用读写分离设计，实现更低的延迟

- **单机服务器**: 专注于单机服务器下的tcp业务开发

- **安全稳定**: 用Rust编写的具有强类型和内存安全的代码

  > **注意**: Lynn_tcp主要用于<u>消息转发</u>和<u>tcp游戏服务器</u>
  >
  > 可以基于框架快速开发合适的业务场景
  >
  > 通过自定义设置，可以实现不同的消息解析、通信数据加密等操作

### 如何使用

#### Dependencies

在Cargo.toml上激活你所需的lynn_tcp功能：

**full features**

使用 `cargo add lynn_tcp` 或者:

```rust
[dependencies]
lynn_tcp = { git = "https://github.com/cherish-ltt/lynn_tcp.git", branch = "main" }
```

**server feature**

```rust
[dependencies]
lynn_tcp = { git = "https://github.com/cherish-ltt/lynn_tcp.git", branch = "main", features = "server" }
```

**client feature**

```rust
[dependencies]
lynn_tcp = { git = "https://github.com/cherish-ltt/lynn_tcp.git", branch = "main", features = "client" }
```

#### Server

```rust
use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = LynnServer::new()
        .await
        .add_router(1, my_service)
        .add_router(2, my_service_with_buf)
        .add_router(3, my_service_with_clients)
        .start()
        .await;
    Ok(())
}

pub async fn my_service() -> HandlerResult {
    HandlerResult::new_without_send()
}
pub async fn my_service_with_buf(input_buf_vo: InputBufVO) -> HandlerResult {
    println!(
        "service read from :{}",
        input_buf_vo.get_input_addr().unwrap()
    );
    HandlerResult::new_without_send()
}
pub async fn my_service_with_clients(clients_context: ClientsContext) -> HandlerResult {
    HandlerResult::new_with_send(1, "hello lynn".into(), clients_context.get_all_clients_addrs().await)
}
```

#### Server with config

```rust
use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = LynnServer::new_with_config(
        LynnServerConfigBuilder::new()
            .with_server_ipv4("0.0.0.0:9177")
            .with_server_max_connections(Some(&200))
            // Suggestion 256-512
            .with_server_max_taskpool_size(&512)
            // ...more
            .build(),
    )
    .await
    .add_router(1, my_service)
    .add_router(2, my_service_with_buf)
    .add_router(3, my_service_with_clients)
    .start()
    .await;
    Ok(())
}

pub async fn my_service() -> HandlerResult {
    HandlerResult::new_without_send()
}
pub async fn my_service_with_buf(input_buf_vo: InputBufVO) -> HandlerResult {
    println!(
        "service read from :{}",
        input_buf_vo.get_input_addr().unwrap()
    );
    HandlerResult::new_without_send()
}
pub async fn my_service_with_clients(clients_context: ClientsContext) -> HandlerResult {
    HandlerResult::new_with_send(
        1,
        "hello lynn".into(),
        clients_context.get_all_clients_addrs().await,
    )
}
```

#### Client

```rust
use lynn_tcp::{
    lynn_client::LynnClient,
    lynn_tcp_dependents::*,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = LynnClient::new_with_ipv4("127.0.0.1:9177")
            .await
            .start()
            .await;
    let _ = client.send_data(HandlerResult::new_with_send_to_server(1, "hello".into())).await;
    let input_buf_vo = client.get_receive_data().await.unwrap();
    Ok(())
}
```

### 特性

- `server`: 可定制的TCP服务，可以轻松实现多用户长连接和高并发处理能力，并为提供路由服务
- `client`: 可自定义的TCP客户端，可以向TCP服务器发送消息，并从服务器接收消息

### 规划

#### 核心功能

- [x] Tcp server - tcp服务器

- [x] Tcp client - tcp客户端

- [x] Custom message parsing - 自定义报文解析

- [x] Automatically clean sockets - 心跳检测

- [x] Routing service for asynchronous tasks - 异步任务路由服务

> Note:
>
> v1.0.0及以上版本支持所有核心功能

#### 扩展功能

- [ ] Scheduled tasks - 定时任务
- [ ] Middleware - 中间件
- [ ] Global database handle - 数据库句柄
- [ ] Communication data encryption - 加密数据
- [ ] Disconnecting reconnection mechanism - 客户端断线重连机制

### 流程图

[FlowChart.png](![FlowChart](https://github.com/cherish-ltt/lynn_tcp/blob/main/FlowChart.png?raw=true))

![FlowChart](https://github.com/cherish-ltt/lynn_tcp/blob/main/FlowChart.png?raw=true)

### 版本介绍

[version.md](https://github.com/cherish-ltt/lynn_tcp/blob/main/version.md)

### 测试结果

平台: Debian12.12 (4H4G)

model-1: one request by one response

model-2: concurrent `send request` and `recv response`

总时长: 20s

**version: lynn_tcp-v1.1.x**

| client concurrency | model-1(Responses/Seconds) | model-2(Responses/Seconds) |
| :----------------- | :------------------------- | -------------------------- |
| 256                | 182,879                    | 80,499                     |
| 512                | 249,135                    | 61,370                     |
| 1024               | 232,861                    | 23,143                     |
| 2048               | 185,735                    | 16,468                     |
| 4096               | 160,318                    | 13,557                     |

**version: lynn_tcp-v1.2.x**

| client concurrency | model-1(Responses/Seconds) | model-2(Responses/Seconds) |
| :----------------- | -------------------------- | -------------------------- |
| 256                | 64,630                     | 492,889                    |
| 512                | 182,296                    | 300,550                    |
| 1024               | 163,307                    | 158,056                    |
| 2048               | 131,346                    | 71,263                     |
| 4096               | 124,645                    | 52,163                     |

### 开源协议

MIT license

### 关于贡献

除非您另有明确说明，否则您有意提交以包含在Lynn_tcp中的任何贡献都应被许可为MIT，无需任何额外的条款或条件