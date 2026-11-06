# 服务端限时排空

`server.start_with_shutdown(signal, grace).await` 接受应用提供的关闭 future 和一个总期限，
返回 `lynn_tcp::Result<ShutdownOutcome>`。操作系统信号、容器通知及应用停机策略由应用管理。
初始化成功后才观察关闭信号；配置、监听绑定及 TLS 初始化错误保持原类型返回。

## 准入和关闭顺序

1. 框架观察到信号时开始计算一次总期限，并关闭准入。新 TCP 接入、待决 TLS 握手、新 KCP
   会话、入站业务请求、后台广播及 SessionHandle 定向推送均不再接收。已完成握手但尚未注册的连接也不能越过边界。
2. 请求以取得处理许可并成功进入工作队列为准。已读但未入队的数据、半帧和边界后的持续输入
   不保证处理。客户端应通过业务协议协调停发。
3. 全实例已接受的 Handler 及中间件链完成，包含最终响应入队，然后各连接排空写队列并关闭。
   这一步使用全实例计数，允许连接 A 的 Handler 完成发往连接 B 的响应；空闲 B 不会先关闭。
4. TCP 完成写入及 shutdown；TLS 完成 flush 和 `close_notify`；KCP 保留已有会话的 UDP 分派、
   DATA/ACK/EOF 和重传，直到本地输出与 EOF 获得传输 ACK；监听器在传输两半及 driver 实际
   释放前继续存活。KCP+TLS 先发送 `close_notify` 再等待有序 KCP EOF 确认，待决认证在
   准入边界处取消。以上步骤仍受同一总期限约束。
5. 结束或超时后取消剩余 worker 和后台任务，并等待可取消任务、传输、KCP driver、连接配额和
   框架持有的 AppState 引用释放后返回；会话观察器已能观察到本地 transport 两端及 KCP driver
   的最终关闭。外部持有的 AppState 引用仍由应用管理。

正常对端 EOF 仍只等待该连接自己的请求；它与全实例停机并发时，关闭标记的入队和停止准入
使用同一边界。已经成功入队的响应会在队列封闭后继续写出。错误帧、心跳驱逐和传输错误保持
原有终止行为；它们不会被排空流程恢复，也不能保证发往已关闭目标的回复。

## 结果与期限

| 结果 | 含义 |
|------|------|
| `ShutdownOutcome::Drained` | 该次排空中已接受工作、响应写入和传输关闭均完成，后台任务已释放 |
| `ShutdownOutcome::Incomplete` | 排空期间 Handler、中间件、响应入队或传输操作失败；错误已记录，任务已回收 |
| `ShutdownOutcome::TimedOut` | 同一总期限耗尽，转入强制取消；不承诺剩余请求或响应已完成 |
| `Err(LynnError)` | 启动失败，保留实际错误类型 |

Handler、发送队列背压、逐帧写入/flush、TLS 关闭、KCP ACK 及任务退出共享一个总预算，
不会因数据进展或进入下一阶段重置。`Duration::ZERO` 明确请求强制取消并返回 `TimedOut`。
更短的现有单次 I/O 超时仍生效，发生在排空期间的错误返回 `Incomplete`。

Tokio 仅支持协作式取消。期限到达会立即发起取消，等待任务释放可能有调度延迟；永不让出执行权
的同步 Handler 或阻塞析构无法被 API 抢占。应用仍应避免在异步任务中同步阻塞。

`Drained` 不是业务交付确认。TCP/TLS 写出不代表对端已处理；KCP ACK 只表示传输层收到。
副作用不会回滚，超时不会自动重试可能已经部分写出的帧。

## 运行示例

```sh
cargo run --example graceful_shutdown --no-default-features --features server
```

示例使用实际绑定的临时 loopback 地址、自带 TCP 客户端和有界退出：等 Handler 已接收请求后
发关闭信号，再释放 Handler，校验完整回复及 EOF，最后检查 `Drained`。

`start()`、Drop、取消 `start()` 或取消 `start_with_shutdown()` future 保留立即取消语义，
这些路径不承诺排空。后台广播和定向推送的入队结果也不保证交付；排空边界后调用广播明确返回
错误，合法长度的 `SessionHandle::try_send` 返回 `Closed`，两者都不能越过停止准入边界。

## 回归覆盖

`tests/server_draining.rs` 验证接收边界、跨连接响应、中间件返回阶段、EOF、零期限、启动错误、
超时及 future 取消。`tests/server_draining_tls.rs` 使用真实 TLS 校验证书、完整响应、
`close_notify` 和待决握手取消。内部测试覆盖全局/每 IP 配额、实际任务及 AppState 释放、
部分写入、满队列、flush/shutdown 阻塞、关闭错误、KCP 正常 DATA/EOF 与丢弃 DATA/EOF ACK。
任务析构探针另验证未轮询取消、等待中取消、正常完成和 panic 时，资源释放先于完成通知；
跨线程读半部析构门闩另验证 TCP 和真实 KCP EOF ACK 后的完成通知不会提前关闭监听器，
实际 KCP listener 异常关闭仍须记录失败；
EOF 等待队列容量后进入 draining 时仍须经过全局屏障，期间队列关闭明确记录失败。
组合回归验证定向推送在排空边界后返回 `Closed`，已有满队列也不能继续准入；TCP/TLS/KCP/KCP+TLS 的
排空或超时返回前，会话观察器须已通知实际 transport 两端和 driver 释放。
`src/infrastructure/kcp/tls_draining_tests.rs` 使用真实 mTLS 校验停机前已入队的定向推送、广播和
响应，观察干净 TLS EOF 和 `Drained`；阻断客户端到服务端的 UDP（包含 ACK）时，即使客户端已读到响应，仍须返回
`TimedOut`。待决认证取消另在 owner 保持存活时验证 driver 与连接许可回收。
