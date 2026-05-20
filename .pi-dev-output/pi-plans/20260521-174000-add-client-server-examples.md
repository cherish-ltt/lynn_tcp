# 新增 3 个客户端-服务器交互示例 — 实施计划

## 概述

当前 `examples/` 目录下有 4 个示例，但**全部仅展示 Server 端**，没有 Client 端代码。没有任何示例展示以下关键功能维度：

- Client ↔ Server 完整通信（发送→处理→回复→接收）
- 路由验证（正确 method_id 被对应 Handler 处理、未知 method_id 被拒绝）
- 请求-响应周期
- 多客户端交互

本计划新增 **3 个示例**，按优先级 P0/P1 排列。所有新增示例均使用默认 features（`server`+`client`），无需额外 feature 标记。

## 文件清单

### 新增文件

| 文件路径 | 用途说明 | 风险等级 |
|---------|---------|---------|
| `examples/echo_server_client.rs` | **P0** — 完整请求-响应循环：Client 发送 → Server 处理 → 回复 → Client 接收 | 低 |
| `examples/multi_route_service.rs` | **P0** — 路由分发与验证：多个 route 注册、正确/未知 route 行为演示 | 低 |
| `examples/custom_protocol_full.rs` | **P1** — 自定义协议客户端配套：Client 也使用 0x1234/0x4321 标记，与 custom_protocol.rs 配对运行 | 低 |

### 修改文件

无。所有现有代码保持不变。

## 对现有代码的假设（已验证）

以下假设已通过代码阅读验证：

1. **`LynnClient`** 可通过默认 feature 直接使用（`client` 是默认 features 之一）
2. **`HandlerResult::new_with_send_to_server(method_id, response_data)`** 是 Client 端发送消息的正确 API
3. **`client.send_data(handler_result)`** 发送消息给 Server
4. **`client.get_receive_data()`** 接收来自 Server 的响应，返回 `Option<InputBufVO>`
5. **Server 的 `ClientsContext.get_all_clients_addrs()`** 返回 `Vec<SocketAddr>`，可用于 `HandlerResult::new_with_send()` 回传
6. **未知 method_id** 时 Server 记录 `warn!("router_map_async no method match,{}", method_id)`（在 `server_common.rs:234`）
7. **`InputBufVO`** 可以通过 `get_method_id()`、`get_all_bytes()` 解析响应数据
8. **示例使用 `#[tokio::main]`** 和 `Result<(), Box<dyn std::error::Error>>` 签名
9. **无需在 `Cargo.toml` 中声明 `[[example]]`**，Cargo 自动发现 `examples/*.rs`

## 实施步骤

### 步骤 1：创建 `examples/echo_server_client.rs`（P0 ⭐⭐⭐）

- **前置条件**：无
- **改动文件**：`examples/echo_server_client.rs`（新增）
- **改动内容**：

  一个**自包含的完整示例**，启动 Server + 一个 Client，演示完整的请求-响应循环。

  **架构**：
  - Server 监听 `0.0.0.0:9177`（默认地址）
  - 注册一个 `echo_handler`（route 1），接收 `InputBufVO`，解析客户端地址，将收到的消息加上 "Echo: " 前缀后回传给发送方
  - Client 连接 Server，发送 `HandlerResult::new_with_send_to_server(1, "Hello Lynn!")`，然后等待接收响应并打印

  **Server 端 echo_handler 逻辑**：
  ```rust
  pub async fn echo_handler(input_buf_vo: InputBufVO, clients_context: ClientsContext) -> HandlerResult {
      let client_addr = input_buf_vo.get_input_addr()...;
      let payload = input_buf_vo.get_all_bytes();
      // 构造回传响应
      let response = format!("Echo: {}", String::from_utf8_lossy(&payload));
      // 使用 ClientsContext 获取客户端地址并回传
      let addrs = clients_context.get_all_clients_addrs().await;
      HandlerResult::new_with_send(1, response.into(), addrs)
  }
  ```

  **Client 端逻辑**：
  ```rust
  // 创建并启动 Client
  let mut client = LynnClient::new_with_addr("127.0.0.1:9177").await.start().await;
  // 发送消息给 Server
  client.send_data(HandlerResult::new_with_send_to_server(1, "Hello Lynn!".into())).await;
  // 等待 Server 响应
  if let Some(response) = client.get_receive_data().await {
      // 解析响应
      let method_id = response.get_method_id();
      let payload = response.get_all_bytes();
      println!("Got response: method_id={:?}, payload={:?}", method_id, payload);
  }
  ```

  **教学要点**：
  - `HandlerResult::new_with_send_to_server()` 构造发送给 Server 的消息
  - `client.send_data()` / `client.get_receive_data()` 收发 API
  - Server 端 `ClientsContext.get_all_clients_addrs()` 获取客户端地址用于回传
  - `HandlerResult::new_with_send()` 回传响应给指定客户端
  - 完整请求-响应生命周期

  **关键实现细节**：
  - Server 和 Client 在不同的 `#[tokio::main]` 中无法同时运行。需要使用 `tokio::spawn` 将 Server 放在后台任务中，Client 在 main 中运行
  - 需要给 Server 一小段启动时间（使用小型 sleep，如 `tokio::time::sleep(Duration::from_millis(100))`）
  - Server handler 签名使用 `(InputBufVO, ClientsContext)` 组合模式 —— 同时获取消息内容和客户端上下文
  - 注意：`add_router` 目前不支持多参数 handler，需要确认。查看 `impl_system_param_function!` 宏和 `impl_tuple!`，它支持 `(T1, T2)` 两个参数的模式

  **⚠️ 重要发现**：根据 `impl_for_context.rs`，只有 `InputBufVO` 和 `ClientsContext` 分别实现了 `SystemParam`，宏也对 `(T1, T2)` 模式做了支持。所以 handler 可以写成：
  ```rust
  async fn echo_handler(input_buf_vo: InputBufVO, clients_context: ClientsContext) -> HandlerResult
  ```

- **验证方式**：运行 `cargo run --example echo_server_client`，确认输出包含：
  - Server 启动信息
  - Client 发送消息日志
  - handler 处理日志（显示收到的消息内容）
  - Client 收到响应并打印 `Echo:` 前缀

### 步骤 2：创建 `examples/multi_route_service.rs`（P0 ⭐⭐⭐）

- **前置条件**：无
- **改动文件**：`examples/multi_route_service.rs`（新增）
- **改动内容**：

  一个**多路由分发与验证示例**，演示 Server 注册多个路由，测试正确/未知 method_id 的行为。

  **架构**：
  - Server 监听 `0.0.0.0:9177`（默认地址）
  - 注册 3 个路由处理器：
    - route 1: `login_handler` — 解析 payload，打印登录信息，回传 "login success"
    - route 2: `user_info_handler` — 解析 payload，打印用户信息请求，回传用户信息
    - route 3: `logout_handler` — 解析 payload，打印登出信息，回传 "logout success"
  - Client 依次发送 3 条消息：
    1. `method_id=1, payload="alice"` → 期望被 `login_handler` 处理
    2. `method_id=2, payload="get_user_info"` → 期望被 `user_info_handler` 处理
    3. `method_id=99, payload="unknown"` → 期望 Server 打印 `"router_map_async no method match,99"`

  **Client 端测试步骤**：
  ```rust
  // 1. 发送登录请求
  client.send_data(HandlerResult::new_with_send_to_server(1, "login:alice".into())).await;
  let r1 = client.get_receive_data().await; // 预期 "login success"
  
  // 2. 发送用户信息请求
  client.send_data(HandlerResult::new_with_send_to_server(2, "get_user_info".into())).await;
  let r2 = client.get_receive_data().await; // 预期用户信息
  
  // 3. 发送未知路由请求
  client.send_data(HandlerResult::new_with_send_to_server(99, "unknown_route".into())).await;
  // Server 端日志: "router_map_async no method match,99"
  // Client 端不会收到响应（因为 route 99 不存在，Server 没有回传）
  ```

  **教学要点**：
  - 路由注册与转发验证 —— 正确 method_id 被正确 handler 处理
  - 不同 route 返回不同格式的响应
  - 未知 method_id 的 Server 端行为（日志警告 `"router_map_async no method match,99"`）
  - 多 route 架构设计模式
  - 如何处理未知 route（通过 `tokio::time::timeout` 检测超时，因为无响应会阻塞 `get_receive_data`）

- **验证方式**：运行 `cargo run --example multi_route_service`，确认：
  - 输出显示 `login_handler` 处理了 route 1
  - 输出显示 `user_info_handler` 处理了 route 2
  - Client 收到正确的响应内容
  - Server 日志包含 `"router_map_async no method match,99"`（对于未知 route）

### 步骤 3：创建 `examples/custom_protocol_full.rs`（P1 ⭐⭐）

- **前置条件**：无
- **改动文件**：`examples/custom_protocol_full.rs`（新增）
- **改动内容**：

  一个**自定义协议客户端配套示例**，解决现有 `custom_protocol.rs` 只有 Server 端的问题。新增 Client 也使用自定义 `0x1234`/`0x4321` 消息标记。

  **架构**：
  - Server 监听 `0.0.0.0:9178`，配置 `header_mark=0x1234`，`tail_mark=0x4321`
  - 注册 `custom_protocol_handler`（route 1）
  - Client 使用相同的 `header_mark=0x1234`、`tail_mark=0x4321` 配置连接
  - Client 发送消息，接收 Server 回传的响应并解析

  **Client 端配置**：
  ```rust
  let config = LynnClientConfigBuilder::new()
      .with_server_addr("127.0.0.1:9178")?
      .with_message_header_mark(&0x1234_u16)
      .with_message_tail_mark(&0x4321_u16)
      .build();
  
  let mut client = LynnClient::new_with_config(config).await.start().await;
  client.send_data(HandlerResult::new_with_send_to_server(1, "custom_protocol_payload".into())).await;
  ```

  **教学要点**：
  - Client 端 `LynnClientConfigBuilder` 的 `with_message_header_mark()` / `with_message_tail_mark()` 方法
  - 自定义协议在 Client 端的完整使用
  - 与现有 `custom_protocol.rs` 配对运行（该示例只展示了 Server 端）
  - 双方使用相同标记才能正确通信

  **⚠️ 注意**：需要确认 `LynnClientConfigBuilder` 是否支持 `with_message_header_mark` 和 `with_message_tail_mark`。—— 已通过代码阅读确认 ✅（`client_config.rs` 第 124-138 行存在这两个方法）

- **验证方式**：运行 `cargo run --example custom_protocol_full`，确认：
  - Server 启动并使用自定义标记
  - Client 启动并使用相同的自定义标记
  - Client 发送的消息正确被 Server 解析（constructor_id, method_id, payload 打印正确）
  - 通信正常完成

## 关于 "两个 tokio runtime 不能嵌套" 问题的处理策略

由于 `#[tokio::main]` 只能有一个，所有示例都采用**单进程、双任务**模式：

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 启动 Server 在后台任务
    let server_handle = tokio::spawn(async {
        let _server = LynnServer::new()
            .await
            .add_router(1, my_handler)
            .start()
            .await;
    });
    
    // 2. 等待 Server 启动
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // 3. 前台运行 Client
    let mut client = LynnClient::new_with_addr("127.0.0.1:9177").await.start().await;
    client.send_data(...).await;
    let response = client.get_receive_data().await;
    
    // 4. 清理
    Ok(())
}
```

## 依赖关系

- 步骤 1、2、3 彼此独立，可以并行执行
- 每个示例在退出时，所有 tokio 任务会自动清理

## 测试策略

- **编译验证**：`cargo check --examples` 验证所有示例编译通过
- **运行验证**：分别运行 3 个示例，确认输出正确
- **回归验证**：`cargo test` 确认现有测试无回归
- **clippy 验证**：`cargo clippy --all-features -- -D warnings` 确认无警告

## 注意事项

1. **端口使用**：
   - `echo_server_client.rs` 使用默认地址 `0.0.0.0:9177`
   - `multi_route_service.rs` 使用默认地址 `0.0.0.0:9177`
   - `custom_protocol_full.rs` 使用 `0.0.0.0:9178`（与现有 `custom_protocol.rs` 一致）

   ⚠️ 同时运行多个示例会导致端口冲突。每个示例应单独运行。

2. **Handler 签名支持**：当前支持的 handler 参数模式。确认 `impl_tuple` 宏对 `(T1, T2)` 的支持：
   - 无参数：`() -> HandlerResult`
   - 单参数：`(InputBufVO) -> HandlerResult` 或 `(ClientsContext) -> HandlerResult`
   - **双参数**：`(InputBufVO, ClientsContext) -> HandlerResult` — 由宏 `impl_system_param_function!(T1, T2)` 支持 ✅

3. **echo 示例中 handler 参数顺序**：由于 `HandlerContext` 同时包含 `input_buf_vo` 和 `clients_context`，宏生成的 `get_param` 按元组顺序从 `state` 中提取。两个 SystemParam 之间无依赖冲突。

4. **Client 接收未知路由的响应**：当发送未知 method_id 时，Server 不生成响应，Client 的 `get_receive_data()` 将会阻塞。示例中使用 `tokio::time::timeout` 包装以避免死等。

5. **禁止顺手重构**：不修改任何现有源代码文件，仅新增 `examples/*.rs` 文件。
