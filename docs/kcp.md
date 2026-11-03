# KCP 传输契约

## 选择与兼容

- TCP 默认不变；feature `kcp` 默认关闭，双端 builder 的 `with_kcp(KcpConfig)` 显式选择 UDP/KCP；不自动回退、不同时监听。
- TCP 专用的 nodelay、keepalive 和 socket buffer 配置不作用于 KCP；KCP 参数采用本文说明的固定配置，框架读写超时继续生效。
- 业务 handler、AppState、多路由、广播和既有帧格式复用；只改变传输接入。独立角色关闭默认 feature 后使用 `client,kcp` 或 `server,kcp`。
- 固定 kcp-ovo `=0.5.0`，直接使用 `core::kcp::Kcp`，不使用消息式高层 KcpStream/KcpListener。上游消息 API 缺少独立 accept/关闭会话及框架所需的所有权控制。
- Rust 1.99.1；上游精确锁定使 Tokio/bytes 必须与本仓库一致，采用 `=1.52.3` / `=1.11.1`，不能把不同 semver-compatible 精确版本当成可以重复安装。
- 上游无条件声明全局 mimalloc，即使关闭其 `stream` feature 也保留；下游自定义全局分配器与 `kcp` 无法共存。未修改、fork 或发布上游。默认不开 KCP 时没有这个额外分配器。

## 安全及互操作

KCP 无加密、无认证，不适合直接承载不可信网络上的敏感数据。TLS+KCP 明确拒绝；现有 TCP+TLS 仍可用。服务端通过已有连接/速率限制约束资源，但不提供防源地址伪造、cookie 握手、抗放大或公网 DoS 承诺；不要把一次可达性握手当作认证。

kcp-ovo 使用自有 28 字节大端 KCP 头，不兼容原版 24 字节 KCP。本适配进一步添加可靠的 `\0lynn-kcp/1` 首消息及 DATA/EOF 封装，因此只与使用本适配的对端互通，不直接兼容上游高层 API。客户端每次连接生成新的随机非零 32 位 conv，这不是安全令牌。

## 会话与所有权

- 服务端首先校验单个数据报长度、非零 conv、Push/sn=0/frg=0 和首消息，再获取现有总连接/每 IP/速率许可，之后才创建 driver 和分派任务。首次握手数据报可能因网络丢失被重传。
- 同一地址当前只允许一个 conv；不同 conv 不会顶替仍存活会话，地址重用须等待旧会话关闭/空闲回收。关闭的映射会在收到数据报或 100 ms 清理周期移除。其他地址独立会话不受影响。
- 每条连接的 LynnStream 持有 driver JoinHandle；释放或取消 owner 会中止 driver，克隆业务 sender 不延长连接生命周期。服务端释放后 listener 通道关闭，driver 与配额随后回收。
- 客户端握手须收到对端可靠首消息后才进入 connected；连接超时和重连间隔沿用现有配置，地址绑定错误通过服务端 `start()` 返回。
- `idle_timeout_secs` 默认 60，非零，按有效同 conv 数据报进展续期；UDP 不能立即感知远端 Drop。框架原有读/写超时、心跳、配额及每连接处理许可继续生效。请让 KCP idle timeout 大于预期心跳和调度间隔。
- 传输错误保留到流上；不以 EOF 掩盖错误。普通远端 EOF 仍走现有响应排空路径；强制取消不排空。

## 长度、背压与交付

- 框架完整 body 上限仍为 10 MiB（业务 payload 最大 10 MiB - 5）；适配把字节流按最多 16 KiB 分块，每块另加 1 字节 DATA 类型，远小于上游分片/接收窗口上限。
- 固定 MTU 1400，KCP send/receive window 为 32/128，10 ms update，启用 nodelay/fastresend=2，保留拥塞控制。发送 backlog 达到 32 段时暂停读取本地流；单次分块允许最多额外 12 段，队列不会随用户输入无界增长。
- 双向本地 duplex 各最多 64 KiB；服务端每会话 UDP 队列最多 128 包、每包不超过 1400 字节。队列满时丢弃 UDP 包并由 KCP 重传恢复，不 detach 无限发送。接收重组受窗口及分片数校验限制，再进入原有有界帧解析器。
- `send_data()` 仅表示业务队列入队；AsyncWrite 成功仅表示适配缓冲接收字节；`flush()` 等待之前写入字节全部获得对端 KCP ACK；ACK 不等价于对端 handler 成功或持久化。
- `shutdown()` 在 flush 后发送可靠有序 EOF 并等待其确认；Drop 不保证执行 shutdown。丢失最后 ACK 的对端可能仍等重传/idle timeout，因此不承诺双方同时释放。
- TLS 指标计量解密后的框架字节；KCP 指标同样只计框架适配读写，不含 UDP/KCP 头、握手、ACK 或重传。发送帧指标在 ACK flush 后才增加；收到响应后立刻 Drop 可能让服务端尚未记录 sent，应用验收应区分收到响应和对方完成 flush。

## 验证入口

```sh
cargo test --locked --all-features --test kcp_transport --test kcp_process
cargo test --locked --all-features --lib infrastructure::kcp
cargo run --example kcp_echo --features kcp
LYNN_BENCH_TRANSPORT=tcp cargo bench --features kcp --bench benchmark -- --model 1 --clients 32,128
LYNN_BENCH_TRANSPORT=kcp cargo bench --features kcp --bench benchmark -- --model 1 --clients 32,128
```

测试包括空/边界/大帧、路由和 AppState、同地址 conv 隔离、总连接/每 IP 配额回收、重连、owner 取消、协议错误关闭、非读取对端背压、半关闭及 user-space UDP 代理丢包/重复/延迟/乱序。独立服务进程黑盒使用公开 LynnClient，验证两种传输同一 handler 的内容一致性；不同 feature 的测试须串行执行或使用不同 target 目录，避免子进程可执行文件被另一构建替换。

压力结果不能预设 KCP 比 TCP 快；同条件比较需要相同进程拓扑、负载、硬件和计时窗口。回环、用户态弱网仿真及 Linux CI 不代表公网、Windows、长期稳定性或生产安全验收。
