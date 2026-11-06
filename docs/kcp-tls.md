# KCP + TLS 1.3 配置与边界

`kcp-tls` 在本项目可靠、有序的 KCP 字节流上运行 TLS 1.3，再承载既有 Lynn 帧、心跳和业务 Handler。它复用精确锁定的 kcp-ovo 0.8.0、tokio-rustls 0.26.5 和 rustls 0.23.45，不引入新的密码库。

该模式适合已根据目标网络实测选择 KCP，同时需要业务内容保护和证书身份校验的应用。没有 UDP 需求时仍可使用 TCP+TLS。TLS-over-KCP 不认证外层 KCP 控制报文，不是完整的安全 UDP 协议，也不提供抗 DoS 或业务交付保证。

## 兼容矩阵

feature 只决定可编译能力，builder 决定运行时传输；单实例只监听所选传输。

| 编译 feature | 运行时配置 | 结果 |
|---|---|---|
| 默认 | 不配置 KCP/TLS | TCP |
| `tls` | `with_tls` | TCP+TLS 1.3 |
| `kcp` | `with_kcp` | 原明文 KCP，HELLO `\0lynn-kcp/1` |
| `kcp,tls`，不启用 `kcp-tls` | 同时配置 KCP/TLS | 显式拒绝 |
| `kcp-tls` | 仅 `with_kcp` | 原明文 KCP |
| `kcp-tls` | 仅 `with_tls` | TCP+TLS 1.3 |
| `kcp-tls` | 同时配置 KCP/TLS | KCP+TLS，HELLO `\0lynn-kcp/2` |

`kcp-tls` 默认关闭，启用它会同时启用 `kcp`、`tls`。独立角色应使用 `default-features = false` 和 `features = ["client", "kcp-tls"]` 或 `["server", "kcp-tls"]`。双端必须使用本适配和同一种模式；不兼容标准 KCP、上游高层 Stream/Listener 或旧明文对端，没有模式自动探测或降级。

## 双端配置与迁移

可编译的双端配置示例在 [英文 README](../README.md#kcp--tls-13-feature-kcp-tls) 和 [中文 README](../README_ZH.md#kcp--tls-13feature-kcp-tls)，由 `readme_kcp_tls_config` 回归保证一致。

1. 两端启用 `kcp-tls`，保留原 `with_kcp(KcpConfig)`、帧标记、路由和状态注册。
2. 服务端同时添加 `with_tls(TlsServerConfig::new("server.pem", "server.key"))`，使用有效 PEM 证书链和对应私钥。
3. 客户端同时添加 `with_tls(TlsClientConfigBuilder::new().with_ca_cert_path("ca.pem").build())`。CA 是必需的，不读取系统信任库作为隐式后备。
4. 默认验证连接地址的 IPv4/IPv6 IP SAN；验证 DNS 身份时显式设置 `server_name`。地址仍用于 UDP 连接，名称只用于 TLS 身份校验。
5. mTLS 在服务端使用 `with_client_ca("client_ca.pem")`；客户端通过现有 TLS builder 同时配置证书和私钥。只有一项、错误链、过期证书、错误 CA/名称均导致失败。
6. 先取得 `server.startup_handle()`，再启动服务并等待真实绑定地址；处理 `start() -> Result<()>`。客户端仍沿用 `start()` 与连接状态 API，配置或认证失败不会开放可用 sender。builder 构造成功不代表证书已加载或认证成功。

安全模式拒绝 `danger_accept_invalid_certs=true`；既有 TCP+TLS 的开发专用开关不变。密钥只从应用指定文件加载，示例不包含固定私钥，测试临时生成并清理证书。不要记录密钥、TLS secrets、凭据或解密 payload。

## 认证、期限与资源所有权

客户端的一次尝试依次完成 KCP HELLO、TLS 1.3 握手、读取加密的固定服务端确认。TLS 1.3 客户端自身握手完成时，服务端仍可能拒绝其最终 mTLS 身份，所以只有收到服务端认证确认后才允许报告 Connected。确认内容不是业务帧，不进入 Handler 或业务字节指标。

`with_connect_timeout_secs` 对上述三个阶段使用一个总期限，重试使用新 KCP conv 和全新 TLS 会话。安全模式禁用恢复、tickets、0-RTT 和 half-RTT 应用数据；不自行管理会话密钥或 nonce。

`subscribe_connection_state()` 的首次 `Connected { generation: 1 }` 也必须等待加密确认。已建立连接中断后先进入 `Reconnecting`，再次完成全部认证后发布递增代次的 `Connected`；重试耗尽发布 `Stopped` 并关闭用户通道。订阅只保留最新快照，可能合并中间变化；未启动或首次认证失败没有订阅，owner 释放后订阅关闭，缓存的 `Connected` 不代表仍然在线。

服务端先限制同时握手数（配置的 worker 数），再保留全局/每 IP 配额，使用受 owner 取消信号管理的任务执行握手，业务 worker 持续处理已认证连接。从安全会话准入开始的总期限为 10 秒，收到数据不会为 TLS 握手续期。认证完成前，连接不会进入可广播的在线表，不执行 Handler。错误、超时或取消会归还配额并释放流，进而取消 KCP driver。

`startup_handle` 只说明监听及服务端初始化完成，不表示客户端 TLS 已认证。UDP 不能立即发现远端 Drop，存活会话还受 KCP idle、框架读写/心跳期限约束；配置应覆盖预期网络延迟和调度停顿。

## 数据预算、刷新与关闭

TLS 记录位于 KCP 字节流之上，可跨多个 DATA 消息和 UDP 包，不把整条 TLS record 直接当作 UDP 数据报。应用 body 上限保持 10 MiB（payload 最大 10 MiB - 5），UDP/KCP 数据报保持最多 1400 字节。

| 缓冲来源 | 边界与计量口径 |
|---|---|
| KCP 本地 duplex | 每方向最多 64 KiB |
| 每会话 UDP 接收队列 | 最多 128 包，每包最多 1400 字节 |
| KCP 分块与窗口 | DATA 分块最多 16 KiB；backlog 达发送窗口后背压，单块最多额外 12 段；接收重组受窗口限制 |
| rustls 发送缓冲 | 锁定版本对待发明文、待发 TLS 数据分别默认设置 64 KiB 限制；不调用无界 `set_buffer_limit(None)` |
| TLS 接收和握手状态 | 额外记录、握手、证书链和密码状态；不计入上述 64 KiB，实际分配还取决于证书与读写进度 |
| 业务帧与队列 | 保留原帧解析、发送队列及 handler 持有的数据；10 MiB 是帧边界，不是连接总内存上限 |

以上是缓冲结构的边界，不是连接总 RSS 上限。TLS 层带来额外分配和复制；压力测试应同时观察进程内存及排队情况，不能只根据单个缓冲限制推算总容量。

逐帧写入会刷新 TLS record，并等待底层 KCP ACK flush；单条小消息不依赖下一条业务消息或心跳才发出。写入/刷新失败结束连接，不重试可能已经部分发送的业务帧。慢读端会将背压传回有界队列，取消可结束阻塞。

正常关闭先发送 TLS close_notify，再执行 KCP 有序 EOF 及 ACK 等待。缺少 close_notify 的底层 EOF 是 TLS 异常截断，不能冒充认证后的干净结束。Drop/abort 仍为强制取消，不保证执行关闭或排空。KCP ACK 只表示外层传输确认且本身不认证，应用须另行确认业务成功或持久化。

[原会话句柄](session-handles.md) 的私发复用同一条已认证 KCP+TLS 连接和 writer，不转发给重连后的新会话；入队仍不代表交付。`SessionObserver::closed()` 同时等待 TLS 读写两端及底层 KCP driver 真正释放，不能以收到 EOF 或发出 abort 代替。句柄和观察器不保活服务端、AppState 或 UDP 监听。

自动网络指标统计 TLS 解密后的 Lynn 框架明文字节（含帧头尾），不包含认证确认、TLS/KCP/UDP 头、握手或重传；发送帧计数仍在完整刷新后增加。收到应用回复与远端完成 ACK flush 是不同事件。

## 安全覆盖与剩余边界

TLS 校验业务记录完整性、身份和连接内记录序列；篡改密文或把旧连接的应用记录重放到新连接不能生成已认证业务内容。抓包搜索不含明文只能辅助检查，不能替代 TLS 协议、证书和负面测试。

IP/端口、包长/时序、conv、KCP 头/ACK/HELLO/DATA/EOF 控制类型仍可见，主动攻击者仍能干扰它们、制造丢弃或占用握手资源。固定 HELLO 标记与随机 conv 不承担认证。没有 cookie、防源地址伪造、抗放大、拥塞公平性或公网可用性承诺；如果要求认证整个数据报，需要另行评估 DTLS 等方案。

## 验证入口

```sh
cargo test --locked --features kcp-tls --test kcp_tls_transport --test readme_kcp_tls_config
cargo test --locked --features kcp-tls --lib infrastructure::kcp::tls_tests
cargo test --locked --no-default-features --features client,server,kcp,tls --test kcp_transport
cargo check --locked --no-default-features --features client,kcp-tls --all-targets
cargo check --locked --no-default-features --features server,kcp-tls --all-targets
```

回归入口覆盖真实 UDP 双端 echo、路由/AppState/广播、空和最大帧、分片/粘包及错误帧、精确明文字节统计、CA/名称/mTLS/中间证书链、明文模式拒绝、TLS 1.3/full handshake、密文篡改与跨连接重放、慢读背压、close_notify/异常截断及丢包/重复/乱序/延迟代理。客户端状态回归覆盖加密确认、真实服务重启、代次递增、重试耗尽与 owner 释放；会话回归覆盖 mTLS 私发隔离、正常关闭及 owner 取消，并独立验证观察器等待 KCP driver。CI 保留不启用安全组合的拒绝分支，并分别构建默认、全 feature 和独立角色。

独立进程的明文 KCP、TCP+TLS 和 KCP+TLS 同条件对照见 [性能报告](kcp-tls-performance.md)，以其中的实际命令、二进制哈希、原始数据和失败轮次为准。

这些回归与回环进程比较不能证明公网、Windows、长期压力或抗 DoS 能力。历史 KCP 性能数据仍属于原来的依赖、二进制和线路模式，不能作为 KCP+TLS 性能结果复用。
