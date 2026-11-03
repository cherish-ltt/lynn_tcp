# KCP 传输契约

## 选择与兼容

- TCP 默认不变；feature `kcp` 默认关闭，双端 builder 的 `with_kcp(KcpConfig)` 显式选择 UDP/KCP；不自动回退、不同时监听。
- TCP 专用的 nodelay、keepalive 和 socket buffer 配置不作用于 KCP；KCP 通过 `KcpConfig::protocol` 设置底层参数，框架读写超时继续生效。
- 业务 handler、AppState、多路由、广播和既有帧格式复用；只改变传输接入。独立角色关闭默认 feature 后使用 `client,kcp` 或 `server,kcp`。
- 固定 kcp-ovo `=0.5.2`，直接使用 `core::kcp::Kcp`，不使用消息式高层 KcpStream/KcpListener。上游消息 API 缺少独立 accept/关闭会话及框架所需的所有权控制。
- Rust 1.99.1；上游精确锁定使 Tokio/bytes 必须与本仓库一致，采用 `=1.52.3` / `=1.11.1`，不能把不同 semver-compatible 精确版本当成可以重复安装。
- 上游 [#36](https://github.com/cherish-ltt/kcp-ovo/issues/36) 已修复；当前 0.5.2 不再强制全局 mimalloc，下游自定义全局分配器可与 `kcp` 共存，已有实际编译/运行回归。上游仍启用 Tokio `full`。没有修改、fork 或发布上游。

## Fast、Normal 与流模式

始终使用底层 `kcp_ovo::core::kcp::Kcp`，不是上游高阶 Stream/Listener API。**消息/流是边界语义，fast/normal 是定时、重传和拥塞参数，两者不是互斥的三种传输。**

| 配置 | nodelay | interval ms | fastresend | nocwnd | stream_mode |
|---|---|---:|---:|---|---|
| 上游 normal/default | false | 100 | 0 | false | false |
| 上游 fast_mode() | true | 20 | 2 | true | false |
| 上游 stream_mode() | true | 20 | 2 | true | true |
| Lynn 默认（自定义 fast） | true | 10 | 2 | true | false |

Lynn 默认 fast 在 send/input 后立即驱动输出，定时 tick 负责重传和空闲检查，不让新数据/ACK 等待 10 ms。使用上游公开 `ts_flush/current/update`，不绕过发送窗口、ACK 或重传算法；normal 保留周期输出。`nocwnd=true` 只关闭拥塞窗口约束，发送/对端接收窗口及本地有界队列仍在；可能降低公平性并加重拥塞，不适合不加带宽约束地部署在共享公网。

```rust
use lynn_tcp::lynn_kcp::{KcpConfig, KcpOptions};

let fast = KcpConfig::default();
let normal = KcpConfig::normal_mode();
let custom = KcpConfig {
    idle_timeout_secs: 60,
    protocol: KcpOptions {
        interval: 10,
        snd_wnd: 128,
        rcv_wnd: 128,
        ..KcpOptions::fast_mode()
    },
};
// Pass the chosen value to both endpoint builders' with_kcp(...).
```

验证约束：MTU 固定 1400，interval 为 1..=5000 ms，fastresend 非负，snd_wnd 非零，rcv_wnd 至少 12（保证完整 16 KiB 块可重组）。接收窗口不是吞吐开关的替代品，增大会按段数增加内存和在途流量。旧版 PR 的 `KcpConfig { idle_timeout_secs: n }` 字面量须增加 `..KcpConfig::default()`。

本适配必须 `stream_mode=false`：HELLO、DATA、EOF 是 KCP 消息封装；直接打开上游流模式会合并/拆分这些边界。因此两端均在启动前**拒绝** stream_mode=true，而非静默忽略。应用仍获得 AsyncRead/AsyncWrite 字节流；将来若采用原生 KCP 流模式，需要单独版本化并实现有界会话封装解析，不能仅翻转 bool。

历史性能根因见 [KCP 性能研究](kcp-performance.md)，当前验收见 [0.5.2 复测](kcp-v0.5.2-validation.md)。已报告的 [kcp-ovo #38](https://github.com/cherish-ltt/kcp-ovo/issues/38) 在 0.5.2 修复并关闭；本项目验证 normal 累加 max、fast 累加半个 RTO，以及使用新 RTO 安排下次重传。未在下游私改算法；修复改变丢包恢复时刻，不能把原版 fast 的宣传值或旧版数据当本适配保证。

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
- 固定 MTU 1400，默认 send/receive window 为 32/128，10 ms tick，自定义 fast 启用 nodelay/fastresend=2/nocwnd。发送 backlog 达到配置 snd_wnd 时暂停读取本地流；单次分块允许最多额外 12 段，队列不会随用户输入无界增长。
- 双向本地 duplex 各最多 64 KiB；服务端每会话 UDP 队列最多 128 包、每包不超过 1400 字节。队列满时丢弃 UDP 包并由 KCP 重传恢复，不 detach 无限发送。接收重组受窗口及分片数校验限制，再进入原有有界帧解析器。
- `send_data()` 仅表示业务队列入队；AsyncWrite 成功仅表示适配缓冲接收字节；`flush()` 等待之前写入字节全部获得对端 KCP ACK；ACK 不等价于对端 handler 成功或持久化。
- `shutdown()` 在 flush 后发送可靠有序 EOF 并等待其确认；Drop 不保证执行 shutdown。丢失最后 ACK 的对端可能仍等重传/idle timeout，因此不承诺双方同时释放。
- TLS 指标计量解密后的框架字节；KCP 指标同样只计框架适配读写，不含 UDP/KCP 头、握手、ACK 或重传。发送帧指标在 ACK flush 后才增加；收到响应后立刻 Drop 可能让服务端尚未记录 sent，应用验收应区分收到响应和对方完成 flush。

## 验证入口

```sh
cargo test --locked --all-features --test kcp_transport --test kcp_process
cargo test --locked --features kcp --test kcp_dependency_contract
cargo test --locked --all-features --lib infrastructure::kcp
cargo run --example kcp_echo --features kcp
LYNN_BENCH_TRANSPORT=tcp cargo bench --features kcp --bench benchmark -- --model 1 --clients 32,128
LYNN_BENCH_TRANSPORT=kcp cargo bench --features kcp --bench benchmark -- --model 1 --clients 32,128
LYNN_BENCH_TRANSPORT=kcp LYNN_BENCH_KCP_MODE=normal cargo bench --features kcp --bench benchmark -- --model 1 --clients 32,128
```

测试包括空/边界/大帧、路由和 AppState、同地址 conv 隔离、总连接/每 IP 配额回收、重连、owner 取消、协议错误关闭、非读取对端背压、半关闭及 user-space UDP 代理丢包/重复/延迟/乱序。独立服务进程黑盒使用公开 LynnClient，验证两种传输同一 handler 的内容一致性；不同 feature 的测试须串行执行或使用不同 target 目录，避免子进程可执行文件被另一构建替换。

压力结果不能预设 KCP 比 TCP 快；同条件比较需要相同进程拓扑、负载、硬件和计时窗口。回环、用户态弱网仿真及 Linux CI 不代表公网、Windows、长期稳定性或生产安全验收。
