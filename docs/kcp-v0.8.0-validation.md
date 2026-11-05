# kcp-ovo 0.8.0 升级验证

日期：2026-11-05。关联 [Issue #97](https://github.com/cherish-ltt/lynn_tcp/issues/97)，目标版本 lynn_tcp 2.4.2；配置与行为契约见 [KCP 指南](kcp.md)。

## 依赖来源与范围

- 从 kcp-ovo `=0.6.0` 升级至官方 `=0.8.0`，发布说明见 [上游 v0.8.0](https://github.com/cherish-ltt/kcp-ovo/releases/tag/v0.8.0)。
- crates.io 包 checksum：`1abcb820bb907ee04bc4504421fe8ad6affddf93c673272524884f46e238f11e`；对应 tag 提交 `7d5e2cd81c7340ea3c13d169873d218f08f955cb`。发布包的 16 个 `src/` 文件与 `Cargo.toml.orig` 已同 tag 比对一致。
- Rust 1.99.1、Tokio `=1.52.3`、bytes `=1.11.1` 不变；上游允许兼容依赖范围，不改变本项目自身精确约束。
- 只迁移底层字节 API 与对应回归。继续使用 `core::kcp::Kcp`；上游高层 Stream/Listener/drain API 不在本次接入范围。

## API 迁移与保持的边界

| 接入点 | 0.6.0 | 0.8.0 适配 |
|---|---|---|
| 头部解析输入 | `&mut Bytes` | `&mut &[u8]`；解析游标与完整数据报分开持有 |
| 接收与编码/输出 | 上游 `Bytes` / `BytesMut` 类型 | `Vec<u8>`；本适配保持使用标准库类型 |
| HELLO 准入 | 声明长度必须等于实际 payload | 保留相等校验，不把解析成功当作有效握手 |
| 配置类型 | 上游 0.6.0 `KcpConfig` | 上游 0.8.0 类型；下游同步升级或使用 `lynn_kcp::KcpOptions` |

上游切片解析只消费头部并返回剩余切片，不替调用方保证剩余数据长度等于头部声明值。完整数据报仍进入 `Kcp::input`，避免只传 payload 丢失头部，或截断剩余切片后放过尾随字节。

默认 TCP、KCP opt-in、自定义 fast（true/10ms/2/true）、normal、HELLO/DATA/EOF 消息模式、10 MiB body 上限、有界队列、owner 取消和 ACK flush 不变。传输 ACK 不代表业务交付；Drop 不保证排空。

## 验证记录

升级前，0.6.0 的四项 `kcp_dependency_contract` 回归通过。它们覆盖应用自选全局分配器、fast/normal 的 RTO 退避和期限、输入缓冲复用、单段/多段接收，以及乱序/重复报文后的窗口恢复。

仅将依赖改为 0.8.0、保留旧调用时，`cargo check --locked --features kcp --all-targets` 在 `driver` 和 `listener` 两处头部解析调用产生 `E0308`：传入 `&mut Bytes`，而上游要求 `&mut &[u8]`。完成适配后，同一命令通过；这是上游 API 类型迁移要求，不是运行时协议缺陷。

| 当前 0.8.0 验证 | 结果 |
|---|---|
| `cargo check --locked --features kcp --all-targets` | 通过 |
| KCP 模块单元测试 | 11 passed |
| `kcp_dependency_contract` | 4 passed |
| `kcp_process` 独立进程测试 | 1 passed |
| `kcp_transport` 集成测试 | 9 passed |
| 默认 `cargo test` | 171 passed，含 doctest |
| 全 feature 测试 | 218 tests + 7 doctests，共 225 passed |
| metrics 黑盒 | 400 请求/400 回复，收发各 9200 B，5/5 连接关闭 |
| `cargo package --locked --allow-dirty --features kcp` | 通过；152 文件、1.7 MiB，包内 v2.4.2 + kcp-ovo 0.8.0 构建成功 |
| `prek run --all-files` | 独立 `PREK_HOME` 下 13 个 hooks 全通过，退出码 0，无 warning |
| 独立覆盖率运行 | 218 tests passed；生产代码行覆盖 93.05%（4166/4477），超过 80% 门槛，映射 warning 经下述完整性核对 |

25 项 KCP 针对性回归全部通过；覆盖 27 字节短头、HELLO 声明长度偏短/偏长、尾随字节及后续合法握手。driver 回归 `handshake_and_invalid_datagram_errors_terminate_worker` 同时确认握手与非法报文错误会结束 worker。

关闭默认 feature 后，以下十种组合的 `cargo check` 与 `cargo test` 均通过；测试数含 doctest，全部 0 failed / 0 ignored：

| feature 组合 | 通过测试数 |
|---|---:|
| 无 feature | 25 |
| `client` | 65 |
| `server` | 129 |
| `metrics` | 36 |
| `client,tls` | 81 |
| `server,tls` | 145 |
| `kcp` | 25 |
| `client,kcp` | 79 |
| `server,kcp` | 144 |
| `client,server,kcp` | 196 |

CI 和正式发布需另行核验，发布后证据关联 [Issue #97](https://github.com/cherish-ltt/lynn_tcp/issues/97)。复跑入口：

```sh
cargo test --locked --features kcp --test kcp_dependency_contract
cargo test --locked --all-features --lib infrastructure::kcp
cargo test --locked --all-features --test kcp_transport --test kcp_process
```

### 覆盖率与 LLVM 映射警告

清理覆盖率产物后运行 `cargo llvm-cov --locked --all-features --fail-under-lines 80 --summary-only`，218 项测试通过，覆盖率命令退出码 0。生产代码行覆盖率为 **93.05%（4166/4477）**，不含上游依赖、测试和示例代码。

LLVM 23.1.1 仍报告 `663 functions have mismatched data`；清理重跑未消除该警告，不能写作无 warning。逐项诊断确认 663 条均为 44 个唯一符号的 hash=0 未使用函数占位映射；44/44 符号在原始 profdata 中恰有一个非零 hash 记录，其实际导出执行计数全部相等且大于 0（1–4757）。保持原始 profile 与同一组 29 个 objects，实测导出和官方 `--empty-profile` 基线的 3676 个函数实例名称、文件与 region 坐标/类型完全一致；38 个生产文件的分母逐项与空 profile 及完整单元测试映射基线一致，合计 4477 行、6685 regions、573 函数。因此本次覆盖率没有因这些占位警告缩小生产代码分母或丢失相关真实计数，按核对后的结果通过 80% 门槛。

## 0.6.0 / 0.8.0 混合版本互通

基线为 lynn_tcp v2.4.1 提交 `8e1fb8a0a16e1774ffdea6fddadf098e8dc06cad`（kcp-ovo 0.6.0），候选为本次 v2.4.2 工作区快照（kcp-ovo 0.8.0）；验证后 64 个生产 Rust/Cargo 文件与工作区一致。macOS 27.2 arm64、Rust/Cargo 1.99.1 下，使用独立源码和 target 目录串行构建，每格启动全新独立服务进程。

相同临时测试客户端为每格建立 16 条连接，每连接发送 50 个 4 B payload 和 1 个 16,385 B 分片 payload。请求按成功 `send_data` 计数，响应须完整逐字节匹配后独立计数。

| 客户端 → 服务端 kcp-ovo | 模式 | 请求/精确回复 | 大 payload 请求/回复 | 退出码 |
|---|---|---:|---:|---:|
| 0.8.0 → 0.6.0 | fast | 816/816 | 16/16 | 0 |
| 0.8.0 → 0.6.0 | normal | 816/816 | 16/16 | 0 |
| 0.6.0 → 0.8.0 | fast | 816/816 | 16/16 | 0 |
| 0.6.0 → 0.8.0 | normal | 816/816 | 16/16 | 0 |

四格首次运行均通过，总计 3264 请求/3264 精确回复，其中 64 个大 payload，双向各 1,061,440 B payload；无客户端失败或超时。被测二进制 SHA-256：

| kcp-ovo | 二进制 | SHA-256 |
|---|---|---|
| 0.6.0 | 测试客户端 | `18531e8630186b4fc77398cb5d7ac9eae49e90215296babb227f9342e70702a4` |
| 0.6.0 | echo server | `07d62d59850d5b8b715ae745faea415a0dd833736fe1b125bcdc06bf420ea261` |
| 0.8.0 | 测试客户端 | `6205b70c19f3fa57bab7379c9de4ef3747989df4d146981631e8ca8ecdf17c3e` |
| 0.8.0 | echo server | `cb04d71f9fcaf2205296fa7f58196acb4fb607af0b3d7c7fb64afca1cece0310` |

这是有限回环互通证据，没有施加丢包/延迟，不构成性能基准、公网/Windows 验收或生产滚动升级保证；服务进程完成交互后由测试清理，不代表跨版本优雅关闭确认测试。

## 性能与证据边界

本次未进行 0.6.0/0.8.0 配对性能测量，不证明吞吐或延迟改善。历史 [0.5.2/0.6.0 配对复测](kcp-v0.6.0-validation.md)、更早报告和 `docs/benchmark/` 原始数据保持原样；文件名、版本、二进制哈希、失败/缺失回复记录均不改写为本版本结果。

公网跨主机、Windows、长时间 soak、拥塞公平性、突发相关丢包及 CPU/内存 profile 未验证。上游源码与包校验、下游测试、GitHub Release 和 crates.io 发布是不同证据，不能相互替代。

署名：GPT-6-Astra-Ultra
