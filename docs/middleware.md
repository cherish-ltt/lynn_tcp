# 异步请求中间件

`server` feature 提供 `lynn_middleware`。中间件可以用于鉴权、请求数据注入、超时及响应处理；旧 `add_router` 和 Handler 签名无需迁移，不注册中间件时仍走原 Handler 快路径。

## 注册与顺序

```rust
use lynn_tcp::{
    lynn_middleware::{MiddlewareChain, MiddlewareRequest, Next},
    lynn_tcp_dependents::HandlerResult,
};

async fn passthrough(
    request: MiddlewareRequest,
    next: Next,
) -> lynn_tcp::Result<HandlerResult> {
    next.run(request).await
}

// server.with_middleware(global)
//     .add_router_with_middleware(1, handler, MiddlewareChain::new().with(passthrough))
```

`with_middleware` 追加全局层，`MiddlewareChain::new().with(m1).with(m2)` 构造路由层。可以注册 async 函数或捕获配置的闭包；函数为 `Send + Sync + 'static`，返回的 Future 为 `Send + 'static`。全局层始终包住路由层，注册顺序即进入顺序，不取决于先添加路由还是全局层：

```text
进入：G1 -> G2 -> R1 -> R2 -> Handler
返回：G1 <- G2 <- R1 <- R2 <- HandlerResult
```

`start()` 前冻结执行计划，不提供热插拔或隐式去重。同一层注册两次会运行两次。重复 `method_id` 告警并覆盖整个 Handler 与本地链；后续普通 `add_router` 清除旧本地链，但全局层保留。

`Next::run(self, request)` 消费 Next 与请求，只能继续一次；它们没有公共构造器和 `Clone`，不提供重试或重新路由。并发请求可同时调用同一层，应用共享状态需自行同步；中间件不会引入新的同连接执行顺序保证。

## 请求数据与响应

| 入口 | 语义 |
|---|---|
| `request.method_id()` / `peer_addr()` | 原始匹配路由与连接来源，只读；地址不是认证身份 |
| `request.payload()` | 只读业务字节，不含帧标记和协议元数据，不推进 Handler 的 `InputBufVO` 游标 |
| `request.app_state::<T>()` | 读取服务端共享 `AppState<T>`，未注册返回 `None` |
| `request.insert(value)` / `get::<T>()` | 本请求独立的类型数据；`T: Send + Sync + 'static`，返回 `Option<Arc<T>>`；同类型替换，使用 newtype 区分不同含义 |
| Handler 参数 `RequestContext` | 只读 `method_id()`、`peer_addr()` 与 `get::<T>()`；payload 继续从旧 `InputBufVO` 参数读取，缺失数据返回 `None` |
| `result.response_view()` | 无发送返回 `None`；否则只读 `method_id()`、`payload()`、`target_addrs()` |

请求类型表只在首次插入时分配，到达 Handler 时冻结。没有中间件或没有插入数据时，上下文有效且为空；不能把“当前用户”写入共享 `AppState`。现有 Handler `AppState<T>` 缺失时的 panic 语义保持不变。把请求交给 Next 后，外层不能再修改或观察下游追加的数据；返回阶段需要的信息应提前保存副本或 Arc。

中间件可以返回原响应、`HandlerResult::new_without_send()`，或用 `new_with_send` 构造替换响应。替换仍校验 **payload 不超过 10 MiB 减 5 字节**，不开放可变字段或原始帧。多目标结果只经过一次返回链，随后编码一次并逐目标入队；返回 `Ok` 或入队成功均不是交付证明。

## 短路与错误

| 情况 | 内层与 Handler | 已进入外层及最终行为 |
|---|---|---|
| 正常调用 Next | Handler 至多执行一次 | 逆序返回并发送最终结果 |
| 不调用 Next，返回 `Ok(拒绝响应)` | 不执行 | 外层正常返回，发送应用定义的拒绝响应 |
| 返回 `Ok(new_without_send())` | 不执行剩余内层 | 外层仍可改写；最终 no-send 不发送 |
| 返回 `Err(LynnError)` | 停止向内执行 | 外层可映射；未处理 Err 在根部记录一次并计一次 dropped，不自动回发内部错误、不关闭健康连接、不重试 |
| timeout | 释放期限内包裹的 Future | 按最终 Ok/Err 处理，不回滚副作用 |
| middleware、参数解析或 Handler panic | 既有任务边界隔离可展开的 panic | 不保证 async 返回逻辑执行，释放许可，其他请求继续；不承诺恢复进程 abort |
| owner 取消 | 释放整链 Future | 不保证 async 清理或排空，不伪造成功 |

框架没有 HTTP 状态码、通用拒绝帧或默认认证策略。应用负责其业务协议，并显式处理缺失身份。错误映射应置于可能报错的层外，例如 `观测 -> 错误映射 -> 超时 -> 鉴权 -> Handler`。不要把内部错误文本、凭据或明文 payload 写入响应/日志。

## 可运行示例与迁移

```sh
cargo run --example middleware_example
```

示例注册全局观测/错误映射与路由超时/鉴权，读取共享策略，将 `Principal` 注入 `RequestContext`，依次验证成功、鉴权拒绝、超时和同一连接后续成功；结束时取消并等待服务端任务。`public-demo:` 是公开演示标记，明文 loopback 不承载秘密；实际凭据应使用校验证书的加密传输，参见 `cargo run --example tls_example --features tls`。KCP 本身不提供加密认证。

迁移时保留业务 Handler，将重复校验移入路由层；只在通过验证后 `insert(Principal)` 并调用 Next。Handler 用 `RequestContext::get` 读取，缺失显式拒绝，不授予默认身份。无需修改其他 Handler 的返回类型或最多 15 个参数的现有支持。

## 超时、资源与观测边界

示例用 Tokio `timeout` 包裹 `next.run(request)`，期限仅覆盖该层以内的中间件与 Handler，不包括排队、本层前置操作、外层返回操作、最终入队与网络发送。框架不添加默认处理超时；读写超时仍负责传输。超时/取消要求 Future 让出执行权，无法抢占阻塞 CPU 或同步死锁，也不能撤销数据库写入或应用自行启动的任务；事务、幂等和副作用由应用设计。

处理许可覆盖排队、整链以及最终响应入队；正常读取 EOF 等待这整段完成。每层在原受 owner 管理的任务内执行，不创建额外任务池或无界队列。中间件仅处理命中路由的普通服务端请求，心跳、未知路由/constructor、畸形帧不进入；后台主动广播也不经过入站链。中间件是可信应用代码，不是插件沙箱。

`processing_duration_seconds` 仅计终端 Handler 与参数解析；短路没有 Handler 样本，Handler panic/取消由计时器 Drop 记录截至终止的耗时。示例的 tracing 记录其包裹链的正常/错误返回及耗时，不包含入队/发送，也不是可靠审计日志；panic/取消不会执行它的 async 返回代码。映射后的业务拒绝在最外层表现为 `Ok`，不能当作业务成功。`sent` 和网络字节仍只由真实传输边界统计，详细口径见 [METRICS](../METRICS.md)。

无中间件时不构造链请求对象或请求类型表；非空链需要异步类型擦除的分配，不承诺零成本。性能对照与复测方法见 [middleware-performance.md](middleware-performance.md)。
