# DDD + 洋葱架构重构 — 实施计划

## 概述

将当前扁平的单体结构重构为 **DDD（Domain-Driven Design）+ 洋葱架构（Onion Architecture）**，使核心域逻辑与基础设施解耦，方便未来替换 SQL/TCP/UDP 等具体实现。重构严格保持所有公共 API 签名不变、不改变任何行为、不添加新功能、不修改业务逻辑。

**架构目标**：
```
┌─────────────────────────────────────┐
│          Interface Layer            │  ← lib.rs 的 pub mod / pub use（对外 API 不变）
│   (LynnServer / LynnClient / 公共类型)│
├─────────────────────────────────────┤
│          Application Layer          │  ← 编排层：LynnServer / LynnClient 的启动流程
│   (ServerApp / ClientApp)           │
├─────────────────────────────────────┤
│           Domain Layer              │  ← 纯业务：Router、HandlerResult、InputBufVO、Handler 系统的 trait
│   (实体 / 值对象 / 接口抽象)          │
├─────────────────────────────────────┤
│         Infrastructure Layer        │  ← 基础设施：TCP 网络 / 事件循环 / 心跳 / 限流 / Prometheus
│   (TcpReactor / 连接管理 / 指标)     │
└─────────────────────────────────────┘
```

## 当前问题（Before）

1. **全部在 `src/` 平铺**：`app/`、`handler/`、`dto_factory/`、`vo_factory/`、`client/` 等是功能命名而非分层命名，导致跨层依赖混乱。
2. **基础设施与域逻辑耦合**：`app/tcp_reactor/`（TCP 事件循环）和 `app/common_api/`（心跳、消息构建）直接依赖 `handler/`、`dto_factory/`、`vo_factory/` 等域类型。
3. **重复的模块名**：`src/app/common_api/` 和 `src/client/common_api/` 以不同方式实现类似逻辑。
4. **`app/` 模块过大**：`LynnServer` 同时负责配置、路由注册、心跳启动、TCP 监听启动，违反单一职责。
5. **`connection_limiter.rs` 重复**：`src/app/connection_limiter.rs` 和 `src/validation.rs` 都有 `ConnectionLimiter`。
6. **`metrics.rs` 与 `metrics_server.rs` 散落在根级**，与基础设施层分离。
7. **`validation.rs` 根级存在**，包含 `ConnectionLimiter`、`SafeBuffer`、`RateLimiter`，理应属于基础设施。

## 改动后目标结构（After）

```
src/
├── lib.rs                          # 接口层：pub mod / pub use（完全不变）
│
├── domain/                         # 领域层（纯业务逻辑，无 Tokio/socket 等基础设施依赖）
│   ├── mod.rs
│   ├── model/                      # 领域模型：实体 + 值对象
│   │   ├── mod.rs
│   │   ├── handler_result.rs       # HandlerResult（从 dto_factory/router_handler.rs 移入）
│   │   ├── input_buf_vo.rs         # InputBufVO（从 vo_factory/input_buf_vo.rs 移入）
│   │   └── lynn_user.rs            # LynnUser（从 app/lynn_server_user.rs 移入）
│   ├── routing/                    # 路由抽象
│   │   ├── mod.rs
│   │   └── router.rs              # LynnRouter（从 app/router.rs 移入）
│   └── handler/                    # Handler 系统抽象（从 handler/ 移入）
│       ├── mod.rs
│       ├── handler_system.rs       # IHandler / IntoSystem / HandlerContext / ClientsContext
│       └── impl_for_context.rs     # SystemParam 实现
│
├── application/                    # 应用层（编排域对象与基础设施）
│   ├── mod.rs
│   ├── server/                     # 服务器应用服务
│   │   ├── mod.rs
│   │   ├── lynn_server.rs          # LynnServer（从 app/mod.rs 移入）
│   │   ├── server_config.rs        # LynnServerConfig / Builder（从 app/lynn_server_config.rs 移入）
│   │   └── server_common.rs        # spawn_check_heart / add_client / push_read_half 等（从 app/common_api/ 移入）
│   └── client/                     # 客户端应用服务
│       ├── mod.rs
│       ├── lynn_client.rs          # LynnClient（从 client/mod.rs 移入）
│       ├── client_config.rs        # LynnClientConfig / Builder（从 client/lynn_client_config.rs 移入）
│       └── client_common.rs        # spawn_handle / spawn_check_heart（从 client/common_api/ 移入）
│
├── infrastructure/                 # 基础设施层（TCP / 事件循环 / 指标 / 限流 / 校验等具体实现）
│   ├── mod.rs
│   ├── tcp/                        # TCP 网络实现
│   │   ├── mod.rs
│   │   ├── reactor.rs              # TcpReactor / CoreReactor / EventManager（从 app/tcp_reactor/ 移入）
│   │   └── tcp_socket_config.rs    # TcpSocketConfig
│   ├── connection/                 # 连接管理
│   │   ├── mod.rs
│   │   └── connection_limiter.rs   # ConnectionLimiter / RateLimiter / IpConnectionLimiter（从 app/ 移入）
│   ├── protocol/                   # 协议实现
│   │   ├── mod.rs
│   │   ├── big_buf_reader.rs       # BigBufReader（从 vo_factory/big_buf_reader.rs 移入）
│   │   └── message_codec.rs        # 消息编码/解码逻辑（从 HandlerResult::get_response_data 提取）
│   ├── validation/                 # 校验
│   │   ├── mod.rs
│   │   └── validation.rs           # validate_message_length 等（从 src/validation.rs 移入）
│   ├── metrics/                    # 指标与监控
│   │   ├── mod.rs
│   │   ├── metrics.rs              # 指标集合（从 src/metrics.rs 移入）
│   │   └── metrics_server.rs       # 指标 HTTP 端点（从 src/metrics_server.rs 移入）
│   └── macros/                     # 宏
│       ├── mod.rs
│       └── handler_macro.rs        # impl_system_param_function! 等（从 macros/handler_macro.rs 移入）
│
├── const_config/                   # 常量配置（保持不动）
│   └── mod.rs
│
├── error.rs                        # 错误类型（保持不动）
│
└── validation.rs                   # ⚠️ 删除，迁移到 infrastructure/validation/
```

## 文件清单

### 新增文件（目录结构）
| 文件路径 | 说明 |
|---------|------|
| `src/domain/mod.rs` | 领域层模块入口 |
| `src/domain/model/mod.rs` | 领域模型模块 |
| `src/domain/model/handler_result.rs` | HandlerResult（从 dto_factory/router_handler.rs 迁入） |
| `src/domain/model/input_buf_vo.rs` | InputBufVO + InputBufVOTrait（从 vo_factory/input_buf_vo.rs 迁入） |
| `src/domain/model/lynn_user.rs` | LynnUser（从 app/lynn_server_user.rs 迁入，去掉 tokio 依赖，变成纯接口） |
| `src/domain/routing/mod.rs` | 路由模块 |
| `src/domain/routing/router.rs` | LynnRouter（从 app/router.rs 迁入） |
| `src/domain/handler/mod.rs` | Handler 系统（从 handler/mod.rs 迁入） |
| `src/domain/handler/handler_system.rs` | IHandler / IntoSystem / HandlerContext / ClientsContext |
| `src/domain/handler/impl_for_context.rs` | SystemParam 实现（从 handler/impl_for_context.rs 迁入） |
| `src/application/mod.rs` | 应用层模块入口 |
| `src/application/server/mod.rs` | 服务器应用模块 |
| `src/application/server/lynn_server.rs` | LynnServer（从 app/mod.rs 迁入） |
| `src/application/server/server_config.rs` | LynnServerConfig / Builder（从 app/lynn_server_config.rs 迁入） |
| `src/application/server/server_common.rs` | 服务器公共函数（从 app/common_api/ 迁入） |
| `src/application/client/mod.rs` | 客户端应用模块 |
| `src/application/client/lynn_client.rs` | LynnClient（从 client/mod.rs 迁入） |
| `src/application/client/client_config.rs` | LynnClientConfig / Builder（从 client/lynn_client_config.rs 迁入） |
| `src/application/client/client_common.rs` | 客户端公共函数（从 client/common_api/ 迁入） |
| `src/infrastructure/mod.rs` | 基础设施层模块入口 |
| `src/infrastructure/tcp/mod.rs` | TCP 模块 |
| `src/infrastructure/tcp/reactor.rs` | TcpReactor / CoreReactor / EventManager（从 app/tcp_reactor/ 迁入） |
| `src/infrastructure/tcp/tcp_socket_config.rs` | TcpSocketConfig |
| `src/infrastructure/connection/mod.rs` | 连接管理模块 |
| `src/infrastructure/connection/connection_limiter.rs` | 限流器（从 app/connection_limiter.rs 迁入） |
| `src/infrastructure/protocol/mod.rs` | 协议模块 |
| `src/infrastructure/protocol/big_buf_reader.rs` | BigBufReader（从 vo_factory/big_buf_reader.rs 迁入） |
| `src/infrastructure/protocol/message_codec.rs` | 消息编码/解码 |
| `src/infrastructure/validation/mod.rs` | 校验模块 |
| `src/infrastructure/validation/validation.rs` | 校验函数（从 src/validation.rs 迁入） |
| `src/infrastructure/metrics/mod.rs` | 指标模块 |
| `src/infrastructure/metrics/metrics.rs` | 指标集合（从 src/metrics.rs 迁入） |
| `src/infrastructure/metrics/metrics_server.rs` | 指标服务器（从 src/metrics_server.rs 迁入） |
| `src/infrastructure/macros/mod.rs` | 宏模块 |
| `src/infrastructure/macros/handler_macro.rs` | 宏（从 macros/handler_macro.rs 迁入） |

### 修改文件
| 文件路径 | 改动描述 | 风险等级 |
|---------|---------|---------|
| `src/lib.rs` | 重写所有 mod/pub use 指向新分层路径；保留所有外部 pub mod / pub use 签名不变 | 高 |
| `Cargo.toml` | 如果存在 `mod.rs` 中 `pub mod` 路径依赖调整，确认 feature flag 不变 | 低 |

### 删除文件（内容已迁移）
| 文件路径 | 原因 |
|---------|------|
| `src/app/mod.rs` | 拆分为 `application/server/lynn_server.rs` + 基础设施层 |
| `src/app/common_api/mod.rs` | 迁移到 `application/server/server_common.rs` |
| `src/app/connection_limiter.rs` | 迁移到 `infrastructure/connection/connection_limiter.rs` |
| `src/app/lynn_server_config.rs` | 迁移到 `application/server/server_config.rs` |
| `src/app/lynn_server_user.rs` | 迁移到 `domain/model/lynn_user.rs` |
| `src/app/router.rs` | 迁移到 `domain/routing/router.rs` |
| `src/app/tcp_reactor/mod.rs` | 迁移到 `infrastructure/tcp/mod.rs` |
| `src/app/tcp_reactor/reactor.rs` | 迁移到 `infrastructure/tcp/reactor.rs` |
| `src/app/tcp_reactor/event.rs` | 迁移到 `infrastructure/tcp/reactor.rs` |
| `src/client/mod.rs` | 迁移到 `application/client/lynn_client.rs` |
| `src/client/common_api/mod.rs` | 迁移到 `application/client/client_common.rs` |
| `src/client/lynn_client_config.rs` | 迁移到 `application/client/client_config.rs` |
| `src/dto_factory/mod.rs` | 删除，HandlerResult 进入 `domain/model/handler_result.rs` |
| `src/dto_factory/router_handler.rs` | 迁移到 `domain/model/handler_result.rs` |
| `src/handler/mod.rs` | 迁移到 `domain/handler/` |
| `src/handler/impl_for_context.rs` | 迁移到 `domain/handler/impl_for_context.rs` |
| `src/macros/mod.rs` | 迁移到 `infrastructure/macros/mod.rs` |
| `src/macros/handler_macro.rs` | 迁移到 `infrastructure/macros/handler_macro.rs` |
| `src/metrics.rs` | 迁移到 `infrastructure/metrics/metrics.rs` |
| `src/metrics_server.rs` | 迁移到 `infrastructure/metrics/metrics_server.rs` |
| `src/validation.rs` | 迁移到 `infrastructure/validation/validation.rs` |
| `src/vo_factory/mod.rs` | 删除，InputBufVO 进入 domain，BigBufReader 进入 infrastructure |
| `src/vo_factory/input_buf_vo.rs` | 迁移到 `domain/model/input_buf_vo.rs` |
| `src/vo_factory/big_buf_reader.rs` | 迁移到 `infrastructure/protocol/big_buf_reader.rs` |

## 实施步骤

### 步骤 1：创建目录骨架与模块文件
- **前置条件**：无
- **改动文件**：创建所有新目录和 `mod.rs` 文件（见上方新增文件清单）
- **改动内容**：
  1. 创建 `src/domain/`、`src/domain/model/`、`src/domain/routing/`、`src/domain/handler/`
  2. 创建 `src/application/`、`src/application/server/`、`src/application/client/`
  3. 创建 `src/infrastructure/`、`src/infrastructure/tcp/`、`src/infrastructure/connection/`、`src/infrastructure/protocol/`、`src/infrastructure/validation/`、`src/infrastructure/metrics/`、`src/infrastructure/macros/`
  4. 每个目录的 `mod.rs` 内部使用 `pub(crate) mod xxx;` 引用子模块
- **验证方式**：`cargo check` 通过

### 步骤 2：迁移领域层（domain）
- **前置条件**：步骤 1 完成
- **改动内容**：

  **2a. `src/domain/model/handler_result.rs`**
  - 从 `src/dto_factory/router_handler.rs` 复制 `HandlerResult` 结构体及所有方法
  - 添加 `pub mod` 别名 `use crate::domain::model::handler_result::HandlerResult;`

  **2b. `src/domain/model/input_buf_vo.rs`**
  - 从 `src/vo_factory/input_buf_vo.rs` 复制 `InputBufVO` 和 `InputBufVOTrait`
  - 保留 `validate_message_length` 调用（通过 `crate::infrastructure::validation::validation::validate_message_length`）

  **2c. `src/domain/model/lynn_user.rs`**
  - 从 `src/app/lynn_server_user.rs` 复制 `LynnUser` 和 `LynnUserSignal`
  - 依赖 `tokio::io::WriteHalf`、`tokio::net::TcpStream` 等基础设施类型 → 在领域层将其抽象为 trait
  - **关键改动**：将 `LynnUser::new` 的 `write_half: WriteHalf<TcpStream>` 参数改为 `WriteHalfTrait` 抽象，或将 `WriteHalf` 作为泛型参数。但为保持最小改动，这里保留直接依赖 `tokio::io::WriteHalf`（领域层在实际中可依赖 tokio 的抽象如 `AsyncWrite`），后续可继续解耦。**此时不做功能改动，仅移动文件 + 调整 import 路径。**

  **2d. `src/domain/routing/router.rs`**
  - 从 `src/app/router.rs` 复制 `LynnRouter`
  - 保留对 `crate::handler::IntoSystem` 的引用（通过 domain handler）

  **2e. `src/domain/handler/handler_system.rs` 和 `impl_for_context.rs`**
  - 从 `src/handler/mod.rs` 和 `handler/impl_for_context.rs` 复制所有类型
  - 重要：`ClientsContext` 引用 `ClientsStruct`（来自 `app/mod.rs`）→ 需要改为引用 domain 中的定义，并将 `ClientsStruct` 也作为 domain 类型
  - `HandlerContext` 引用 `InputBufVO` → 改为 domain 路径

- **验证方式**：`cargo check` 通过，不产生新错误

### 步骤 3：迁移应用层（application）
- **前置条件**：步骤 2 完成
- **改动内容**：

  **3a. `src/application/server/server_config.rs`**
  - 从 `src/app/lynn_server_config.rs` 迁移 `LynnServerConfig` / `LynnServerConfigBuilder`
  - 调整 import：`const_config` 路径不变；去掉 `use crate::app::...` 等

  **3b. `src/application/server/lynn_server.rs`**
  - 从 `src/app/mod.rs` 迁移 `LynnServer` 结构体 + impl
  - 重要：保留所有 pub 方法签名不变
  - `pub async fn new() / new_with_config / add_router / start / log_server` 签名完全不变
  - 内部依赖指向新路径：`domain::routing::router::LynnRouter`、`domain::model::lynn_user::LynnUser` 等
  - `ClientsStruct` / `ClientsStructType` / `AsyncFunc` / `ReactorEventSender` 等类型定义移至本层或基础设施层
  - `TcpSocketConfig`、`TcpReactor` 引用改为 `infrastructure::tcp::` 路径

  **3c. `src/application/server/server_common.rs`**
  - 从 `src/app/common_api/mod.rs` 迁移：`spawn_check_heart`、`check_handler_result`、`send_response`、`input_dto_build`、`add_client`、`push_read_half`
  - 调整所有 import 路径

  **3d. `src/application/client/client_config.rs`**
  - 从 `src/client/lynn_client_config.rs` 迁移

  **3e. `src/application/client/lynn_client.rs`**
  - 从 `src/client/mod.rs` 迁移 `LynnClient`
  - 保留所有 pub 方法签名：`new_with_config`、`new_with_addr`、`start`、`send_data`、`get_receive_data`、`get_sender`、`log_server`

  **3f. `src/application/client/client_common.rs`**
  - 从 `src/client/common_api/mod.rs` 迁移

- **验证方式**：`cargo check` 通过，无新警告

### 步骤 4：迁移基础设施层（infrastructure）
- **前置条件**：步骤 2、3 完成
- **改动内容**：

  **4a. `src/infrastructure/tcp/tcp_socket_config.rs`**
  - 新的 `TcpSocketConfig` 结构体（从 `app/tcp_reactor/mod.rs` 提取）

  **4b. `src/infrastructure/tcp/reactor.rs`**
  - 从 `src/app/tcp_reactor/mod.rs` + `reactor.rs` + `event.rs` 合并迁入
  - `TcpReactor`、`CoreReactor`、`EventManager`、`ReactorEvent` 全部移入
  - 调整所有 import 路径

  **4c. `src/infrastructure/connection/connection_limiter.rs`**
  - 从 `src/app/connection_limiter.rs` 迁入
  - 保留全部代码和测试，仅调整 import

  **4d. `src/infrastructure/protocol/big_buf_reader.rs`**
  - 从 `src/vo_factory/big_buf_reader.rs` 迁入

  **4e. `src/infrastructure/protocol/message_codec.rs`**
  - 从 `HandlerResult::get_response_data()` 中提取消息编码逻辑作为独立函数
  - 注意：`get_response_data()` 本身就是 HandlerResult 的方法，不应在基础设施层重复 → **改为在 HandlerResult 中直接调用基础设施的编解码函数**
  - 实际上是 HandlerResult 的方法保留，内部调用 `MessageCodec::encode(method_id, bytes, header_mark, tail_mark)`
  - 类似地，客户端/服务端的读取解析逻辑调用 `MessageCodec::decode()`

  **4f. `src/infrastructure/validation/validation.rs`**
  - 从 `src/validation.rs` 迁入：去除 `ConnectionLimiter`（已存在 infrastructure/connection 中）、`SafeBuffer`、`RateLimiter`
  - 仅保留 `validate_message_length`、`validate_message_format` 等纯校验函数
  - 注意：`ConnectionLimiter` 在 `src/validation.rs` 中重复定义了 → 迁移时**只保留 `app/connection_limiter.rs` 那份**，删除 validation.rs 中的重复定义

  **4g. `src/infrastructure/metrics/metrics.rs`**
  - 从 `src/metrics.rs` 完整迁入，调整 import

  **4h. `src/infrastructure/metrics/metrics_server.rs`**
  - 从 `src/metrics_server.rs` 完整迁入，调整 import

  **4i. `src/infrastructure/macros/handler_macro.rs`**
  - 从 `src/macros/handler_macro.rs` 完整迁入，调整 import

- **验证方式**：`cargo check` 通过

### 步骤 5：重写 `src/lib.rs` 的模块注册和 pub use
- **前置条件**：步骤 2-4 完成
- **改动文件**：`src/lib.rs`
- **改动内容**：
  1. 删除旧的 `mod app;` `mod client;` `mod dto_factory;` `mod handler;` `mod macros;` `mod metrics;` `mod metrics_server;` `mod vo_factory;` 等
  2. 替换为 `mod domain;` `mod application;` `mod infrastructure;`
  3. 保留 `mod const_config;` `mod error;`（不变）
  4. 保留 `mod validation;` 但改为 `pub(crate) mod infrastructure;` 然后在 infrastructure 内部引用 validation
  5. 所有 `pub mod lynn_server` / `pub mod lynn_client` / `pub mod lynn_tcp_dependents` / `pub mod lynn_metrics` 的 **pub use 路径指向新的分层位置**
  6. 例如：
     - `pub use super::app::LynnServer;` → `pub use super::application::server::lynn_server::LynnServer;`
     - `pub use super::app::lynn_config_api::LynnServerConfig;` → `pub use super::application::server::server_config::LynnServerConfig;`
     - `pub use super::handler::ClientsContext;` → `pub use super::domain::handler::handler_system::ClientsContext;`
     - `pub use super::dto_factory::input_dto::HandlerResult;` → `pub use super::domain::model::handler_result::HandlerResult;`
     - `pub use super::vo_factory::InputBufVOTrait;` → `pub use super::domain::model::input_buf_vo::InputBufVOTrait;`
     - `pub use super::vo_factory::input_vo::InputBufVO;` → `pub use super::domain::model::input_buf_vo::InputBufVO;`
     - `pub use super::client::LynnClient;` → `pub use super::application::client::lynn_client::LynnClient;`
     - `pub use super::client::client_config::LynnClientConfig;` → `pub use super::application::client::client_config::LynnClientConfig;`
     - 指标相关：`pub use super::infrastructure::metrics::metrics::{Metrics, Timer, export_metrics};`
     - `pub use super::infrastructure::metrics::metrics_server::{MetricsServerConfig, serve_metrics, spawn_metrics_server};`
     - `pub use prometheus;` 保留不变
  7. **关键**：确保 `pub extern crate bytes;` `pub extern crate tokio;` `pub extern crate tracing;` `pub extern crate tracing_subscriber;` 保留

- **验证方式**：
  - 确认所有 pub use 路径正确
  - `cargo check` 通过
  - 对比 git diff（lib.rs）确认只有路径重定向，无业务变化

### 步骤 6：删除旧文件并验证编译
- **前置条件**：步骤 1-5 全部完成且编译通过
- **改动内容**：
  1. 逐个删除旧文件（见上方删除文件清单）
  2. 每删除一个，运行 `cargo check` 确认无引用丢失
- **验证方式**：
  - 最终 `cargo check` 干净通过
  - `cargo clippy --all-targets -- -D warnings` 干净通过
  - `cargo test --all-features` 全部通过

### 步骤 7：最终全量验证
- **前置条件**：步骤 6 完成
- **改动内容**：无代码改动
- **验证方式**：
  ```bash
  cargo check --all-features 2>&1
  cargo clippy --all-targets --all-features -- -D warnings 2>&1
  cargo test --all-features 2>&1
  ```
  三个命令全部干净通过

## 依赖关系

```
步骤 1 (目录骨架)
  └─→ 步骤 2 (领域层迁移)
        ├─→ 步骤 3 (应用层迁移)
        │     └─→ 步骤 5 (lib.rs)
        └─→ 步骤 4 (基础设施迁移)
              └─→ 步骤 5 (lib.rs)
                    └─→ 步骤 6 (删除旧文件)
                          └─→ 步骤 7 (最终验证)
```

- 步骤 2/3/4 可并行执行（因为各自独立的目录和文件）
- 步骤 5 依赖 2/3/4 全部完成
- 步骤 6 依赖步骤 5 完成

## 测试策略

### 单元测试
- `domain/handler/impl_for_context.rs` 中的 SystemParam 实现 → 无测试，保持不动
- `infrastructure/connection/connection_limiter.rs` 已有 `mod tests` → 保持，路径修改后应仍能运行
- `infrastructure/validation/validation.rs` 已有 `mod tests` → 迁移后保持
- `infrastructure/metrics/metrics.rs` 已有 `mod tests` → 迁移后保持
- `infrastructure/metrics/metrics_server.rs` 已有 `mod tests` → 迁移后保持

### 集成测试
- `cargo test --all-features` 运行所有测试套件

### 回归测试
- 对比重构前后的公共 API 输出确保一致：
  ```bash
  # 重构前（在 git stash 前）
  cargo doc --no-deps
  # 重构后
  cargo doc --no-deps
  # 对比文档中的公共 API 签名
  ```

## 注意事项

### 关键依赖路径
重构中最复杂的是**循环依赖**的避免：

```
domain/handler/handler_system.rs
  → HandlerContext 包含 InputBufVO ✓（domain/model/input_buf_vo.rs）
  → HandlerContext 包含 ClientsContext ✓（同文件）
  → IHandler 返回 HandlerResult ✓（domain/model/handler_result.rs）

application/server/lynn_server.rs
  → 引用 domain 的 Router / HandlerResult / InputBufVO ✓
  → 引用 infrastructure 的 TcpReactor / TcpSocketConfig ✓

infrastructure/tcp/reactor.rs
  → 引用 domain 的 LynnRouter / InputBufVO ✓
  → 引用 application 的 server_common 中的 check_handler_result / add_client / push_read_half
    ⚠️ 这是基础设施引用应用层，在洋葱架构中是允许的（基础设施实现端口/适配器）
```

### 需要特别注意的类型别名
以下类型别名在 `app/mod.rs` 中定义，迁移后需要选择合适的分层位置：

| 类型别名 | 原定义位置 | 新位置 |
|---------|-----------|--------|
| `ClientsStructType = Arc<DashMap<SocketAddr, LynnUser>>` | `app/mod.rs` | `domain/model/lynn_user.rs` 或 `application/server/lynn_server.rs`（因 DashMap 是基础设施依赖，应放在应用层） |
| `ClientsStruct(pub ClientsStructType)` | `app/mod.rs` | 同上 |
| `AsyncFunc = Box<dyn IHandler>` | `app/mod.rs` | `domain/handler/handler_system.rs`（纯类型抽象） |
| `TaskBodyOutChannel` | `app/mod.rs` | `application/server/lynn_server.rs` |
| `ReactorEventSender = Arc<Injector<ReactorEvent>>` | `app/mod.rs` | `infrastructure/tcp/reactor.rs`（依赖 crossbeam） |

### 不改变行为的关键点
1. **`LynnServer::new()` 内部调用了 `TcpReactor::new()`** → 保持
2. **`LynnServer::add_router()` 的参数 `IntoSystem<Param>`** → 保持完全相同的 trait bound
3. **`HandlerResult` 的所有构造方法** → 方法签名不变，`new_without_send()` / `new_with_send()` / `new_with_send_to_server()` 的 cfg feature 条件不变
4. **`InputBufVO` 的 `InputBufVOTrait`** → 保持 trait 中的所有方法签名
5. **`pub use` 的 feature gate** → `#[cfg(feature = "server")]` / `#[cfg(feature = "client")]` / `#[cfg(any(feature = "server", feature = "client"))]` 必须完全一致

### 迁移中的坑
- **`src/validation.rs` 中有重复的 `ConnectionLimiter`** → 迁移时**必须只保留 `app/connection_limiter.rs` 的那一份**，否则编译冲突
- **`src/validation.rs` 中的 `RateLimiter`** 与 `app/connection_limiter.rs` 中的 `RateLimiter` 功能不同（一个在 connection 层面的 token bucket，一个是 message 层面的 rate limit）→ 迁移时应保留两个不同的实现（可改名区分）
- **`server_common.rs` 中的 `push_read_half` 被 `CoreReactor` 调用** → 需要确保 `application` 层对 `infrastructure` 层可见。在基础设施中引用应用层函数是允许的（依赖倒置），但需要确认 `mod` 可见性正确

### 迁移后的模块可见性
由于 Rust 的模块系统，模块之间的可见性需要调整：
- `domain/` 中的类型需要是 `pub(crate)` 以便 `application/` 和 `infrastructure/` 引用
- `application/` 中的 pub 类型需要是 `pub` 以便 `lib.rs` 引用
- `infrastructure/` 中的类型根据情况用 `pub(crate)` 或 `pub`
