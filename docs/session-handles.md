# Original-session push and close observation

The `server` feature exports `SessionHandle`, `SessionObserver`, `SessionId`, and
`SessionSendOutcome` from `lynn_tcp::lynn_server`. `RequestContext::session()` and
`MiddlewareRequest::session()` refer to the original connection captured when the
request entered the processing queue. Cloning that handle does not query the online
address table. A delayed request keeps the old identity after a reconnect, and different
server instances never share identities. `SessionId` supports `Clone`, `Eq`, and `Hash`;
it is process-local and is not a user identity or a serializable credential.

Call `try_send(method_id, Bytes)` after registration has returned. `Enqueued` means one
non-waiting enqueue to that connection's bounded writer queue; `Full` and `Closed`
explicitly reject the push. No retry or offline storage is provided. A concurrent close
can discard an accepted frame. Server draining rejects new pushes as `Closed` while keeping
earlier enqueues in the drain. Payload validation happens first: empty payloads are legal,
and payloads above 10 MiB minus 5 bytes return `LynnError::Protocol` without harming the
connection. The normal writer applies instance frame markers and handles TCP, TLS, KCP and
KCP+TLS; sent counters advance after write and flush, not when the frame is queued. Rejected
queues count as dropped messages. Existing handler responses retain waiting backpressure.
Outgoing pushes bypass incoming middleware. Applications must authorize a request before
granting or storing private push capability.

`handle.observer()` grants only close observation. It exposes `id()`, `is_closed()` and
`closed().await`; clones and repeated waits, including waits started after closure, see the
same persistent terminal state. Cancelling one wait does not consume the notification.
Neither the sender nor observer keeps the server owner, transport, tasks or AppState alive.
The framework does not call or await application cleanup code.

Close means both local transport halves and any KCP driver have been released. Normal read EOF first drains
accepted requests, their middleware and response enqueueing, then completes writer
shutdown. EOF alone is not close. Writer errors, read errors/timeouts, heartbeat eviction
and owner cancellation retain their existing termination policies. A notification is not a
delivery receipt, an immediate physical-disconnect signal, or a guarantee that every handler
side effect has stopped. Valid-size sends made after observing close return `Closed`.
For [KCP+TLS](kcp-tls.md), private pushes use the same authenticated session; close waits
for both TLS halves and the underlying KCP driver to be released.

**Do not wait for final close inside a handler or middleware that still holds that
connection's request permit.** Normal EOF needs that request to return before it can drain.
Register the handle, return, and await close in an application-owned background task.
When cleaning a subscription keyed by your own user ID, remove it only if the stored
`SessionId` still equals the closing observer's ID; a new connection may have replaced it.

Run the bounded, self-contained public demo with:

```sh
cargo run --example session_push
```

It waits for each registration response, sends private background results, and verifies the
clients receive them. Two connections replace the same demonstration subscription; closing
the old connection preserves the new record, and closing the new connection removes it.
It binds directly to loopback port `0` and awaits `startup_handle()` for the actual address.
This demo does not authenticate users. Enable `server,client` for it when default features
are disabled.

## 原会话定向发送与关闭观察

`server` feature 在 `lynn_tcp::lynn_server` 导出 `SessionHandle`、`SessionObserver`、
`SessionId` 和 `SessionSendOutcome`。请求或中间件通过 `session()` 取得入队时绑定的原会话；
不在 Handler 执行时按地址重查。延迟执行的旧请求和重连后的新请求身份不同，不同服务端实例
也相互隔离。`SessionId` 支持克隆、相等比较和哈希，只用于进程内比较，不是用户身份或凭据。

Handler 注册并返回后，后台调用 `try_send(method_id, Bytes)`。`Enqueued` 仅表示向原会话有界
队列非等待入队，`Full` 和 `Closed` 明确拒绝；没有重试、离线存储或交付保证。并发关闭可能
丢弃已入队消息。服务端排空边界后的推送返回 `Closed`，边界前已入队消息仍参与排空。
长度先校验：空 payload 合法，超过 10 MiB 减 5 字节返回 `LynnError::Protocol`，
不关闭健康连接。复用实例帧标记及 TCP/TLS/KCP/KCP+TLS 共用 writer，发送指标在写完并 flush 后增加，
队列拒绝计入 dropped；普通 Handler 响应仍等待背压。主动发送不经过入站中间件，应用必须先
完成授权再授予或保存私人推送能力。

`observer()` 可单独授予观察权限，提供 `id()`、`is_closed()` 与 `closed().await`。终态保留，
关闭前后开始等待、多观察者以及取消等待后重试均有效。句柄和观察器不保活 owner、I/O、任务
或 AppState；框架不调用或等待应用的清理逻辑。

关闭表示本地读写两端及 KCP driver 已经释放。正常读 EOF 先排空已接收的请求链及响应入队，再完成 writer
shutdown，EOF 本身不是终态。写错误、读错误/超时、心跳驱逐和 owner 取消保留现有策略。
通知不代表交付、物理断网立即可见或所有 Handler 副作用已停止；观察关闭之后的合法长度推送
返回 `Closed`。[KCP+TLS](kcp-tls.md) 私发使用同一条已认证会话，关闭须等待 TLS 读写两端和底层
KCP driver 释放。仍持有该连接处理许可的 Handler/中间件不得等待本会话最终关闭，否则会与正常
EOF 排空互相等待。应先返回，由独立后台任务等待；按业务用户键清理时，先比较保存的 SessionId，
防止旧会话清理删除重连记录。上方有界示例让两个连接注册同一演示键，验证旧连接关闭保留新记录、
新连接关闭移除记录；示例直接绑定 loopback 端口 `0`，通过 `startup_handle()` 等待实际地址，
使用公开演示注册，不包含用户认证。
