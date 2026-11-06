# KCP 配置、迁移与场景指南

适用 lynn_tcp **v2.6.0**，延续 v2.4.2 锁定的 kcp-ovo **0.8.0**；v2.4.1 使用 0.6.0，v2.4.0 使用 0.5.2。配置和安全边界以本页为准；v2.4.2 升级证据见 [0.8.0 验证](kcp-v0.8.0-validation.md)。[0.5.2/0.6.0 配对复测](kcp-v0.6.0-validation.md) 与 [0.5.2 复测](kcp-v0.5.2-validation.md) 保留为历史证据。新的 [KCP/TCP+TLS/KCP+TLS 对照](kcp-tls-performance.md) 采样于 v2.5.0 开发快照，存档保留原版本与哈希；v2.6.0 的集成和版本更新不代表重新测量。

## 从 TCP 或早期 PR 迁移

1. 使用 Rust 1.99.1，更新依赖与 lock；本项目继续精确锁定 Tokio/bytes 为 `=1.52.3` / `=1.11.1`，即使保持 TCP 也需验证下游依赖解析。kcp-ovo 0.8.0 已放宽为兼容版本范围，但本项目的精确约束仍有效；应用直接依赖 Tokio/bytes 时须与此组合兼容。
2. 双端显式启用 `kcp`，通过公开 builder 选择同一适配；只启用 feature 不会把默认 TCP 自动改成 KCP。无需直接依赖 kcp-ovo 或替换业务 handler。
3. 对应地址须允许 UDP 流量；TCP 端口可达不代表 UDP 可达，框架不会穿透 NAT 或自动回退 TCP。生产灰度保留独立 TCP/TCP+TLS 实例和部署配置，客户端明确选择目标传输；不要把敏感数据从 TLS 迁移为明文 KCP。
4. 从默认 fast 和窗口 32/128 开始，保留既有协议标记、心跳及应用层回复检查，再按下文逐项测量调整。旧 PR 中 `KcpConfig { idle_timeout_secs: n }` 字面量补 `..KcpConfig::default()`；不继续使用旧 tick-only 性能结果或 kcp-ovo 0.5.0。
5. 连接建立后确认 `client.is_connected()`，处理 `server.start().await` 的错误；`build()` 本身不验证 KCP 组合。非法参数在双端传输初始化时拒绝，客户端连接失败沿用现有重试/未连接语义，不把 builder 成功当作已连通。
6. 删除对框架自动指标的重复手工累加，执行真实 echo、断开/取消及指标增量验收；严格交付场景必须先取得应用层确认，再释放 owner，见 [指标迁移](../METRICS.md#自动采集边界与迁移)。

已选定 KCP 且需要业务流加密时，双端改启用 `kcp-tls` 并同时配置 KCP/TLS；详细配对配置和迁移边界见 [KCP+TLS](kcp-tls.md)。仅增加 feature 不会启用加密，原明文 KCP 配置保持原样。

双端依赖（默认同时启用 server/client；独立角色设置 `default-features = false`）：

```toml
[dependencies]
lynn_tcp = { version = "=2.6.0", features = ["kcp"] }
tokio = { version = "=1.52.3", features = ["macros", "rt-multi-thread"] }
```

配置片段仅构造两端配置；启动及逐内容 echo 的完整可运行代码见 [kcp_echo](../examples/kcp_echo.rs)：

```rust
use lynn_tcp::{
    lynn_client::LynnClientConfigBuilder,
    lynn_kcp::KcpConfig,
    lynn_server::LynnServerConfigBuilder,
};

fn main() -> lynn_tcp::Result<()> {
    let kcp = KcpConfig::default();
    let _server = LynnServerConfigBuilder::new()
        .with_addr("127.0.0.1:9177")?
        .with_kcp(kcp.clone())
        .build();
    let _client = LynnClientConfigBuilder::new()
        .with_server_addr("127.0.0.1:9177")?
        .with_kcp(kcp)
        .build();
    Ok(())
}
```

## 建议场景与调整顺序

下表数值来自 [历史 0.5.2 复测](kcp-v0.5.2-validation.md)，不是 0.8.0 测量值；历史新旧配对结果见 [0.6.0 复测](kcp-v0.6.0-validation.md)。这些结果不能直接用作本版本的性能验收。

| 场景 | 建议 | 依据与限制 |
|---|---|---|
| 回环、稳定局域网、低丢包或吞吐优先 | 优先 TCP | 历史纯回环 KCP 仅为 TCP 的 29.9%；大文件/持续流未测，不预设 KCP 优势 |
| 有 RTT、受控 UDP 链路、延迟敏感小消息 | 评估默认 fast | 128 B、32 并发、20 ms RTT 无丢包为 TCP 的 94.5%；1 并发低丢包仍可能落后 |
| 独立随机丢包明显的小消息交互 | 目标环境 A/B 验证 KCP fast | 同轮 10%/20%/30% 丢包吞吐为 TCP 的 2.21/4.40/7.42 倍，不代表突发丢包或拥塞链路 |
| 需要认证、加密，或不可信共享公网 | 优先 TCP+TLS；已选 KCP 的应用可评估 `kcp-tls` | TLS 只保护业务流，外层 KCP 控制报文仍不认证；fast nocwnd 存在公平性/拥塞风险 |
| 需要保守参数基线或排查 fast 取舍 | 对照 normal | 开启拥塞窗口约束、100 ms 周期输出，可能增加时延；未单独验证其公网公平性 |

先保持默认参数，用真实 payload/并发、目标 RTT/带宽和丢包分布比较 TCP/fast/normal；同时记录吞吐、尾延迟、请求/回复差值、连接错误及资源占用，而非只看响应/s。该历史研究采用固定顺序短时测试，0.6.0 配对复测交错新旧顺序；两者均不是统计显著性或容量上限证明。

确有带宽时延积/发送窗口瓶颈时再逐步增大 `snd_wnd/rcv_wnd`；每连接仅一个在途小请求时，增大窗口通常不能解决调度或 ACK 等待成本。interval 更小会增加唤醒与 CPU 成本，且本地 fast 已即时驱动新数据/ACK，不应盲目改成 1 ms。MTU 必须保持 1400；不得以放大队列、关闭 ACK 等待或启用原生流模式来换取跑分。

让 `idle_timeout_secs` 覆盖预期心跳/调度间隔及弱网停顿，并一起检查框架心跳、读写和客户端连接超时；只增大 KCP idle 不能阻止更短的框架超时先关闭连接。缩短 idle 可加快消失对端回收，但会增加暂时断网时的误断。normal/custom 也不提供自动安全或交付保证。

## 选择与兼容

- TCP 默认不变；feature `kcp` 默认关闭，双端 builder 的 `with_kcp(KcpConfig)` 显式选择 UDP/KCP；不自动回退、不同时监听。
- `kcp-tls` 同样默认关闭，并启用 `kcp` 和 `tls`；只有该 feature 加双端 `with_kcp`/`with_tls` 才选择 TLS-over-KCP。仅启用 `kcp,tls` 仍拒绝运行时同时配置，原 TCP+TLS 不受影响。
- TCP 专用的 nodelay、keepalive 和 socket buffer 配置不作用于 KCP；KCP 通过 `KcpConfig::protocol` 设置底层参数，框架读写超时继续生效。
- 业务 handler、AppState、多路由、广播和既有帧格式复用；只改变传输接入。独立角色关闭默认 feature 后使用 `client,kcp` 或 `server,kcp`。
- 固定 kcp-ovo `=0.8.0`，直接使用 `core::kcp::Kcp`；继续由本适配管理 accept、会话及 driver 所有权，不接入上游高层 KcpStream/KcpListener 或 drain API。
- `KcpConfig.protocol` 暴露上游配置类型；应用若另外依赖 kcp-ovo 0.6.0 或更早版本并直接传入配置，需同步升级至 0.8.0，或改用本库的 `lynn_kcp::KcpOptions` 重导出，避免两个版本的 Rust 类型不匹配。普通 builder/默认配置与线路格式不变。
- 上游字节 API 改用标准库类型：头部解析接收 `&mut &[u8]`，接收及编码/输出返回 `Vec<u8>`；本适配保留完整数据报供 `Kcp::input` 使用，并严格校验声明长度与实际 payload，继续维护 HELLO/DATA/EOF 消息边界。
- Rust 1.99.1；本项目保留 Tokio/bytes 的 `=1.52.3` / `=1.11.1` 精确约束。上游 0.8.0 使用兼容版本范围，本项目并未随之放宽自身依赖。
- 上游 [#36](https://github.com/cherish-ltt/kcp-ovo/issues/36) 已修复；0.5.2 起不再强制全局 mimalloc，下游自定义全局分配器可与 `kcp` 共存，已有实际编译/运行回归。上游仍启用 Tokio `full`。没有修改、fork 或发布上游。

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

历史性能根因见 [KCP 性能研究](kcp-performance.md)，当前依赖兼容验收见 [0.8.0 验证](kcp-v0.8.0-validation.md)，历史配对测量见 [0.5.2/0.6.0 复测](kcp-v0.6.0-validation.md)。已报告的 [kcp-ovo #38](https://github.com/cherish-ltt/kcp-ovo/issues/38) 在 0.5.2 修复并关闭，0.8.0 保留该策略；本项目验证 normal 累加 max、fast 累加半个 RTO，以及使用新 RTO 安排下次重传。未在下游私改算法；不能把上游微基准或旧版数据当本适配端到端性能保证。

## 安全及互操作

明文 KCP 无加密、无认证，不适合直接承载不可信网络上的敏感数据。显式启用 `kcp-tls` 后可在 KCP 字节流上运行 TLS 1.3，复用 CA、名称校验和可选 mTLS；安全模式拒绝无验证开关，禁用 0-RTT 和会话恢复。业务流之外的 KCP 头、ACK、HELLO、DATA/EOF 类型仍未认证。服务端通过已有连接/速率限制约束资源，但不提供防源地址伪造、cookie 握手、抗放大或公网 DoS 承诺；不要把 KCP ACK 当作认证或业务交付证明。完整边界见 [KCP+TLS](kcp-tls.md)。

kcp-ovo 使用自有 28 字节大端 KCP 头，不兼容原版 24 字节 KCP。本适配进一步添加可靠的首消息（明文 `\0lynn-kcp/1`，KCP+TLS `\0lynn-kcp/2`）及 DATA/EOF 封装，因此只与使用本适配且选择相同模式的对端互通，不直接兼容上游高层 API。模式不匹配会失败，不自动降级；首消息本身不承担认证。客户端每次连接生成新的随机非零 32 位 conv，这不是安全令牌。

## 会话与所有权

- 服务端首先校验单个数据报长度、非零 conv、Push/sn=0/frg=0 和首消息，再获取现有总连接/每 IP/速率许可，之后才创建 driver 和分派任务。首次握手数据报可能因网络丢失被重传。
- 同一地址当前只允许一个 conv；不同 conv 不会顶替仍存活会话，地址重用须等待旧会话关闭/空闲回收。关闭的映射会在收到数据报或 100 ms 清理周期移除。其他地址独立会话不受影响。
- 每条连接的 LynnStream 持有 driver JoinHandle；释放或取消 owner 会中止 driver，克隆业务 sender 不延长连接生命周期。服务端释放后 listener 通道关闭，driver 与配额随后回收。
- 明文客户端须收到对端可靠首消息后才进入 connected；KCP+TLS 还须完成 TLS 握手并收到加密的服务端认证确认。安全会话在认证前不进入业务在线表或 Handler。客户端单次连接期限覆盖全部阶段，服务端从握手准入开始沿用 10 秒总期限并在 worker 外限并发；地址绑定错误通过服务端 `start()` 返回。
- `idle_timeout_secs` 默认 60，非零，按有效同 conv 数据报进展续期；UDP 不能立即感知远端 Drop。框架原有读/写超时、心跳、配额及每连接处理许可继续生效。请让 KCP idle timeout 大于预期心跳和调度间隔。
- 传输错误保留到流上；不以 EOF 掩盖错误。普通远端 EOF 仍走现有响应排空路径；强制取消不排空。

## 长度、背压与交付

- 框架完整 body 上限仍为 10 MiB（业务 payload 最大 10 MiB - 5）；适配把字节流按最多 16 KiB 分块，每块另加 1 字节 DATA 类型，远小于上游分片/接收窗口上限。
- 固定 MTU 1400，默认 send/receive window 为 32/128，10 ms tick，自定义 fast 启用 nodelay/fastresend=2/nocwnd。发送 backlog 达到配置 snd_wnd 时暂停读取本地流；单次分块允许最多额外 12 段，队列不会随用户输入无界增长。
- 双向本地 duplex 各最多 64 KiB；服务端每会话 UDP 队列最多 128 包、每包不超过 1400 字节。队列满时丢弃 UDP 包并由 KCP 重传恢复，不 detach 无限发送。接收重组受窗口及分片数校验限制，再进入原有有界帧解析器。
- `send_data()` 仅表示业务队列入队；AsyncWrite 成功仅表示适配缓冲接收字节；`flush()` 等待之前写入字节全部获得对端 KCP ACK；ACK 不等价于对端 handler 成功或持久化。
- 明文 `shutdown()` 在 flush 后发送可靠有序 EOF 并等待其确认；KCP+TLS 先发送 TLS close_notify，再推进底层 EOF。缺少 close_notify 的 TLS 截断必须报错。Drop 不保证执行 shutdown。丢失最后 ACK 的对端可能仍等重传/idle timeout，因此不承诺双方同时释放。
- TLS 指标计量解密后的框架字节；KCP 指标同样只计框架适配读写，不含 UDP/KCP 头、握手、ACK 或重传。发送帧指标在 ACK flush 后才增加；收到响应后立刻 Drop 可能让服务端尚未记录 sent，应用验收应区分收到响应和对方完成 flush。

## 验证入口

```sh
cargo test --locked --all-features --test kcp_transport --test kcp_process
cargo test --locked --features kcp --test kcp_dependency_contract
cargo test --locked --all-features --lib infrastructure::kcp
cargo test --locked --features kcp-tls --test kcp_tls_transport --test readme_kcp_tls_config
cargo run --example kcp_echo --features kcp
LYNN_BENCH_TRANSPORT=tcp cargo bench --features kcp --bench benchmark -- --model 1 --clients 32,128
LYNN_BENCH_TRANSPORT=kcp cargo bench --features kcp --bench benchmark -- --model 1 --clients 32,128
LYNN_BENCH_TRANSPORT=kcp LYNN_BENCH_KCP_MODE=normal cargo bench --features kcp --bench benchmark -- --model 1 --clients 32,128
```

测试包括空/边界/大帧、路由和 AppState、同地址 conv 隔离、总连接/每 IP 配额回收、重连、owner 取消、协议错误关闭、非读取对端背压、半关闭及 user-space UDP 代理丢包/重复/延迟/乱序。独立服务进程黑盒使用公开 LynnClient，验证两种传输同一 handler 的内容一致性；不同 feature 的测试须串行执行或使用不同 target 目录，避免子进程可执行文件被另一构建替换。

压力结果不能预设 KCP 比 TCP 快；同条件比较需要相同进程拓扑、负载、硬件和计时窗口。回环、用户态弱网仿真及 Linux CI 不代表公网、Windows、长期稳定性或生产安全验收。
