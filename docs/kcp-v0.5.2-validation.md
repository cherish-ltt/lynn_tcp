# kcp-ovo 0.5.2 下游复测

日期：2026-11-03。对应 PR #90，基于 `ee84c1935e3f4307300357b83591b6b9ee76e3bf` 更新依赖；不是 lynn_tcp 2.3.0 已发布性能承诺。0.5.0 的根因研究和原始结果继续保留在 [历史研究](kcp-performance.md)，不覆盖或重命名为新版本结果。

发布补记：PR #89/#90 已合入 v2.4.0，合并 main `966479a` 与已验证 `c9abd3f` 文件树一致。本文其余内容保留合入前的测试时间点与原始数据；“待审阅”等历史状态不代表当前状态。升级与建议场景见 [KCP 指南](kcp.md)，版本交付记录见 [v2.4.0 变更日志](update_logs/v2.4.0.md)。

## 1. 依赖及缺陷验证

- 官方 crates.io sparse 索引确认 0.5.2 已发布且未撤回；下载的发布包 SHA-256 为 `1937c022fd6dbd720d971706f8098af8f30d4e52f04f98aeeaa72e30459186fe`，与官方索引及本次 Cargo.lock 一致。
- 仅把 `kcp-ovo =0.5.0` 升到 `=0.5.2`，lock 同步移除 mimalloc/libmimalloc-sys；其余依赖版本、Rust 1.99.1、Tokio 1.52.3、bytes 1.11.1 均不变。没有改动本地生产传输或 benchmark 计时逻辑，没有修改、fork、发布上游。
- [#36](https://github.com/cherish-ltt/kcp-ovo/issues/36) 与 [#38](https://github.com/cherish-ltt/kcp-ovo/issues/38) 均已关闭；本次不仅检查 Issue 状态，还运行下游依赖契约与真实传输回归。

新增 `tests/kcp_dependency_contract.rs`，由最终应用声明 `std::alloc::System`，通过本项目公开 KcpConfig 选择 fast/normal，然后驱动底层 Kcp。所有输出被丢弃、不返回 ACK，直接推进协议时间，测试不依赖真实等待：

| 输入/操作 | 0.5.0 实测 | 0.5.2 实测 |
|---|---|---|
| 应用自行声明全局 allocator 并启用 KCP | 编译失败：`global allocator ... conflicts ... kcp_ovo` | 同一集成测试可编译运行，2 passed |
| fast 连续五次超时的 RTO（含初值，ms） | 200/400/800/1600/3200/6400（负对照首轮 400 != 300） | 200/300/450/675/1012/1518 |
| normal 连续五次超时的 RTO（含初值，ms） | 200/200/200/200/200/200（负对照首轮 200 != 400） | 200/400/800/1600/3200/6400 |
| 超时重传后的下次期限 | 使用旧 RTO；normal 还重复增加余量 | 每次使用 `当前时刻 + 新 RTO`；到期前 1 ms 不重传 |

旧版 RTO 负对照在独立临时 crate 中去掉 allocator 声明、改用相同上游配置，其余断言相同，结果 **0 passed / 2 failed**；不切换主仓库版本、不修改上游。正式测试要求首次期限 fast=200 ms、normal=225 ms，保留 normal 首次余量。时钟回绕、60 秒饱和、死链和 Karn 等上游内部回归未复制到本项目；本项目覆盖实际依赖契约，不重复维护整个 KCP 算法测试集。

## 2. 配置与不变边界

仍使用底层 `kcp_ovo::core::kcp::Kcp`，不是高阶 API；默认自定义 fast：`nodelay=true, interval=10, fastresend=2, nocwnd=true, stream_mode=false`，MTU=1400、snd/rcv window=32/128。新数据及 ACK 即时驱动，周期 tick 负责重传/空闲。normal 为 false/100/0/false，保留周期输出。

消息/流与 fast/normal 是独立维度。框架提供 AsyncRead/AsyncWrite 字节流，但内部 HELLO/DATA/EOF 依赖 KCP 消息边界，故原生 `stream_mode=true` 明确拒绝；0.5.2 不改变该约束。TCP 默认、KCP 默认关闭、TLS+KCP 明确拒绝。完整配置、会话、交付、安全说明见 [传输契约](kcp.md)。

`flush` 仍等待传输 ACK，不等价于业务完成；没有为跑分改成仅本地入队。KCP 无加密/认证，不兼容标准 KCP，`nocwnd=true` 可能牺牲共享链路公平性。这些是公开边界，不是升级能够自动消除的缺陷。

## 3. 功能、黑盒与静态检查

本地 macOS arm64、Rust 1.99.1，所有 feature 组合串行执行，避免同名 `bench_echo_server` 被其他构建覆盖。以下均为本次 0.5.2 结果，不沿用旧 commit 的通过记录：

| 检查 | 命令/覆盖范围 | 本次结果 |
|---|---|---|
| 依赖定向回归 | `cargo test --locked --all-features --test kcp_transport --test kcp_process --test kcp_dependency_contract -- --nocapture` | 12 passed：2 依赖契约、9 真实 KCP 集成、1 进程黑盒 |
| 默认构建/测试 | `cargo build --locked`、`cargo test --locked` | 构建通过，171 passed（含 7 doctest） |
| 全功能构建/测试 | `cargo check --locked --all-features --all-targets`、`cargo test --locked --all-features` | 223 passed（127 lib、89 集成、7 doctest） |
| 静态检查 | `cargo fmt --all -- --check`，默认与全功能 `cargo clippy --locked --all-targets [--all-features] -- -D warnings` | 全部通过 |
| KCP 冒烟 | `cargo run --locked --features kcp --example kcp_echo` | `KCP echo: hello KCP` |
| 指标进程黑盒 | `cargo build --locked --example metrics_example` 后 `python3 tests/metrics_blackbox.py target/debug/examples/metrics_example` | 400/400 请求响应、5/5 连接关闭、收/发各 9200 框架字节，全部精确匹配 |

独立 feature 均运行 `cargo check --locked --no-default-features --features "..." --all-targets` 和 `cargo test --locked --no-default-features --features "..."`：none=25、client=65、server=129、metrics=36、client,tls=81、server,tls=145、kcp=25、client,kcp=77、server,kcp=142、client,server,kcp=194 passed，全部 0 failed。`kcp` 单独没有任何角色，传输 API 同角色门控，依赖契约测试也遵循该边界。

独立进程黑盒分别运行 TCP、KCP fast、KCP normal，各 16 客户端 × 50 次 = **800/800** 逐内容回复，合计 2400/2400。真实 KCP 回归还覆盖 0 至 10 MiB-5 载荷、IPv6、自定义标记、多路由/AppState、损坏输入、连接配额、重连/取消、ACK flush/半关闭、背压、UDP 丢包/乱序/重复及实际服务端指标。全功能测试包含 benchmark 独立请求计数、缺失/错误回复回归；不是仅以退出码证明交付。

覆盖率运行 `cargo llvm-cov --locked --all-features --lcov --output-path /tmp/lynn-kcp-v052/coverage.lcov`，随后 `cargo llvm-cov report --summary-only --fail-under-lines 80`：216 tests passed，**行覆盖 92.68%（4152/4480）**，超过 80%，无覆盖率数据不匹配警告。KCP config/driver/listener/stream 分别为 100%/98.33%/88.04%/94.31%；不包含上游源码覆盖率，不代表安全证明。与历史 92.90% 的小幅差异来自本次实际覆盖执行，不以旧数字替代。

提交前使用既有干净缓存运行 `PREK_HOME=/tmp/lynn-issue-tests/prek-clean prek run --all-files`，13 项全部通过，无扫描 warning/error。容器安装提示 slim 镜像缺少 man 页及证书 rehash 提示，本机 Cargo wrapper 提示已有 sccache，均非 Rust 编译/测试失败；未以环境提示替代验证。自审修正了新增测试的角色门控并通过完整 feature 矩阵；本轮未改变生产传输逻辑。

## 4. 同条件 TCP/KCP 主压测

环境：Debian 13.6 临时容器，OrbStack aarch64，4 逻辑核，Linux `7.2.12-orbstack-00394-gbecdd6130068`，TCP cubic、SACK=1、iproute2 6.15.0、Rust 1.99.1。同一 Debian 镜像 `sha256:7e3898f7b011a107d0ef7393d5f604a6e0c0ff05ac4f2476630a8af21059ec9b`；bridge、仅添加 NET_ADMIN、无宿主挂载，只设置容器 lo，不改 macOS 网络。源码拷贝到容器；release 构建完成且本地回归/覆盖率全部结束后才开始串行压测，本任务测量期间不构建；宿主并非专用物理机，不能排除其他系统负载。

两种传输使用同一 release 构建（含 `kcp` feature），128 B、model 1、1/32 并发、每连接一个在途请求、每格全新独立 server（4 Tokio workers），1 秒预热/5 秒测量、500 ms 尾部接收、外层 timeout 180 秒。默认建连/心跳/关闭配置不变。三轮固定顺序：clean → 20 ms/0% → 1% → 5% → 10% → 20% → 30%，每场景 TCP 后 KCP，非随机顺序。

每次重置 qdisc，非 clean 场景每方向 delay=10 ms、名义 RTT=20 ms，非零丢包为 `loss random X% seed 42`，limit=10000、不限带宽。数据及 ACK 同受包级丢弃；同 seed 不代表同一业务丢包轨迹。qdisc 统计包含建连、预热及两个并发格，不是测量期业务丢失率。42 次命令均 exit=0，84 格全部保留；逐轮计数、退出码、时间戳、p50/p95/p99、qdisc、环境/二进制哈希见 [原始 JSON](benchmark/v2.3.0-arm64-kcp-0.5.2-study.json)。

各列分别取三轮中位数；p99 是三次 p99 的中位数，不是合并原始 RTT 的 p99，只含已收到回复，缺失请求不进入分位数，存在截尾偏差。

| 场景 RTT / 丢包 | 并发 | TCP 响应/s | KCP 响应/s | TCP p50 / p99 ms | KCP p50 / p99 ms |
|---|---:|---:|---:|---|---|
| 无 netem | 1 | 1319.2 | 363.8 | 0.054 / 8.880 | 1.627 / 13.580 |
| 无 netem | 32 | 83873.2 | 25076.6 | 0.141 / 4.843 | 0.598 / 6.118 |
| 20 ms / 0% | 1 | 36.8 | 33.6 | 25.755 / 45.133 | 27.507 / 51.709 |
| 20 ms / 0% | 32 | 1164.4 | 1100.6 | 25.704 / 86.797 | 27.183 / 61.509 |
| 20 ms / 1% | 1 | 35.0 | 33.6 | 25.511 / 46.879 | 27.653 / 78.329 |
| 20 ms / 1% | 32 | 1001.2 | 1072.4 | 25.198 / 258.605 | 27.116 / 80.314 |
| 20 ms / 5% | 1 | 21.4 | 29.4 | 25.280 / 264.111 | 27.946 / 92.168 |
| 20 ms / 5% | 32 | 607.6 | 961.0 | 25.378 / 355.276 | 27.264 / 97.094 |
| 20 ms / 10% | 1 | 10.0 | 26.2 | 26.926 / 381.536 | 28.125 / 87.430 |
| 20 ms / 10% | 32 | 370.2 | 817.0 | 26.166 / 611.403 | 27.480 / 149.463 |
| 20 ms / 20% | 1 | 3.4 | 17.8 | 30.144 / 859.442 | 30.281 / 163.715 |
| 20 ms / 20% | 32 | 123.4 | 542.6 | 27.250 / 1675.954 | 31.820 / 273.783 |
| 20 ms / 30% | 1 | 2.4 | 13.8 | 不可计算（有一轮无回复） | 35.875 / 323.745 |
| 20 ms / 30% | 32 | 53.8 | 399.0 | 33.374 / 2333.969 | 56.193 / 420.965 |

30%/TCP/1 并发第 2 轮为 **1 请求 / 0 回复、0 response/s、latency=null**，其余两轮吞吐 2.6/2.4；保留零值，不以两轮有回复的 p99 冒充三轮中位数。

84 格共 **1,706,037 请求 / 1,705,957 内容校验响应，80 个未回复差值（TCP 76、KCP 4）**。所有 clean、0%、1% 格相等；5%/TCP/32 并发差 1。以下为高丢包三轮累计：

| 丢包 | 并发 | TCP 请求 / 回复 / 差值 | KCP 请求 / 回复 / 差值 |
|---|---:|---|---|
| 10% | 1 | 150 / 150 / 0 | 389 / 389 / 0 |
| 10% | 32 | 5566 / 5559 / 7 | 12056 / 12056 / 0 |
| 20% | 1 | 54 / 53 / 1 | 270 / 270 / 0 |
| 20% | 32 | 1914 / 1890 / 24 | 8407 / 8406 / 1 |
| 30% | 1 | 26 / 25 / 1 | 194 / 194 / 0 |
| 30% | 32 | 862 / 820 / 42 | 6271 / 6268 / 3 |

请求以发送开始时间归属测量窗口、成功本地入队后计数；响应独立计数并校验长度/每字节。尾部等待从 sent_at 计算后才调用接收，可能额外包含入队耗时，并非严格硬截止。未回复可能涉及超时/关闭，主基准没有保存逐请求关闭原因；不能仅凭成功退出宣称完整交付，也不能据此认定永久丢包。

## 5. 与历史 0.5.0 的比较

以下均为 32 并发三轮中位数。两版不是同一时刻测试或同包轨迹重放；0.5.2 同时移除了全局 mimalloc，含 KCP feature 的 TCP 基线也受 allocator 变化影响。因此不能将所有数值变化归因于 RTO，不能据此宣称统计显著提速或回归。

| 场景 | KCP 0.5.0 响应/s | KCP 0.5.2 响应/s | KCP 0.5.0 p99 ms | KCP 0.5.2 p99 ms |
|---|---:|---:|---:|---:|
| 无 netem | 21491.0 | 25076.6 | 6.596 | 6.118 |
| 20 ms / 0% | 1198.0 | 1100.6 | 45.377 | 61.509 |
| 20 ms / 1% | 1152.2 | 1072.4 | 72.850 | 80.314 |
| 20 ms / 5% | 998.4 | 961.0 | 88.749 | 97.094 |
| 20 ms / 10% | 810.2 | 817.0 | 137.018 | 149.463 |
| 20 ms / 20% | 540.0 | 542.6 | 266.186 | 273.783 |
| 20 ms / 30% | 371.4 | 399.0 | 451.742 | 420.965 |

同轮 TCP 对照更适合回答是否符合预期：32 并发、20 ms/0% 时 KCP 为 TCP 的 **94.5%**，1%/5% 为 **1.07/1.58 倍**，10%/20%/30% 为 **2.21/4.40/7.42 倍**。纯回环只有 **29.9%**；1 并发、0%/1% 仍慢于 TCP。因此支持“有 RTT 的小包场景接近 TCP、随机丢包时尤其高并发更有优势”，不支持“所有无丢包场景等于 TCP、所有丢包场景更快”。

0.5.2 是正确性修复，不是普遍提速版本：本轮低丢包吞吐/尾延迟未全面改善，30% 吞吐略升且 p99 降低，20% 变化很小；KCP 总缺失仍为 4（旧高丢包研究为 30% 下 4，新为 20% 下 1、30% 下 3），不能宣称修复 #38 后所有尾部缺失消失。

## 6. 长尾接收诊断与验收

只在隔离容器将 DRAIN 从 500 ms 改为 30000 ms 并重建 benchmark，20%/30% 丢包各三轮、TCP/KCP、1/32 并发，其余参数不变。12 次命令均 exit=0，24 格全部保存于同一 JSON 的 `tail_diagnostic_runs`；生产源码和主测试期限未改变。不是主测试相同轨迹重放，不覆盖原来的 80 个差值，也不混合两种期限比较吞吐。

| 丢包 | 并发 | TCP 请求 / 回复 | KCP 请求 / 回复 |
|---|---:|---|---|
| 20% | 1 | 50 / 50 | 257 / 257 |
| 20% | 32 | 2141 / 2141 | 9189 / 9189 |
| 30% | 1 | 7 / 7 | 193 / 193 |
| 30% | 32 | 838 / 838 | 5697 / 5697 |

诊断 KCP **15336/15336**，TCP **3036/3036** 内容一致；TCP 30%/1 并发第 2/3 轮测量期是 0/0，保留而不当作交付成功样本。此次延长等待后没有观测到丢失，但不能证明主测试所有差值一定晚到，也不能证明任意弱网可靠。完成后 qdisc 确认为 noqueue，已删除本次容器，未触碰其他容器及宿主网络。

**验收判断：按 Issue #76 的功能边界，KCP 接入符合预期，两个已确认的上游缺陷均已在本项目通过 0.5.2 解除，不再等待上游；本轮未发现需新增 Issue 的明确缺陷。** 实际低层 API、自定义 fast、同一业务处理、会话隔离/回收、取消/重连、背压/长度限制、真实指标及兼容构建均通过。PR 仍待管理员审阅合并，不代表本项目已发布。

| 项目 | 当前判断 | 处理归属 |
|---|---|---|
| 数据/ACK 等待 tick、旧基准回填请求数 | 前轮本地修复保留，本轮回归通过 | 本项目 PR #90 已解决 |
| allocator 冲突 #36 | 发布包去除库级 allocator；下游 System allocator 编译与运行通过 | 上游修复已接入，不等待更新 |
| fast/normal 退避及 resendts #38 | 负对照失败、0.5.2 相同契约通过，真实弱网完整复测 | 上游修复已接入，不等待更新 |
| 纯回环落后、低并发低丢包未必更快 | 性能与 ACK flush/用户态调度取舍，未证明算法缺陷 | 如需进一步优化，应在本项目先 profile，不按理论调参掩盖结果 |
| 主测试期限内少数缺失回复 | 如实保留；独立 30 秒诊断全数回复，但不是相同请求轨迹 | 当前证据不足以创建永久丢包 Issue；若部署要求严格期限，应追加逐请求关闭原因与目标 SLA 验证 |
| 原生 stream_mode、TLS+KCP、标准 KCP 互通 | 仍明确不支持，不是静默失效 | 超出当前范围，需要本地另行设计，不是等待上游自动解决 |

未覆盖公网跨主机、Windows、长时间 soak、完整 1024/4096 并发梯度、限带宽拥塞、公平性、突发相关丢包及 CPU/内存 profile。不能把短时回环/容器测试当成生产安全验收；KCP 无认证/加密，不适合直接承载不可信网络中的敏感数据。

## 7. 复跑方法

功能命令见第 3 节与 [传输契约](kcp.md)。包级测试使用 [历史研究第 6 节](kcp-performance.md#6-复跑入口) 的容器构建方式，使用当前依赖/lock；场景循环为 `clean delay20 loss1 loss5 loss10 loss20 loss30`，三轮，单次命令如下：

```sh
# Only inside the dedicated container; never change the host qdisc.
test -f /.dockerenv
# For a nonzero-loss scenario, reset qdisc before each TCP/KCP command:
tc qdisc add dev lo root netem limit 10000 delay 10ms loss random 20% seed 42
LYNN_BENCH_TRANSPORT=kcp LYNN_BENCH_KCP_MODE=fast timeout 180s "$bench" \
  --model 1 --clients 1,32 --warmup 1 --duration 5 --json /results/loss20-kcp-1.json
# Save exit status, start/end timestamps and this snapshot even on failure.
tc -s qdisc show dev lo
```

0% 仅加 `delay 10ms`；clean 不设 qdisc。每次新运行前删除本容器旧 qdisc；复制所有结果后删除 qdisc/本次容器。诊断只在副本修改 `benches/support/model1.rs` 的 DRAIN 为 30000 ms，重新构建并使用独立 `recovery-` 结果名，不能混进主表。

署名：GPT-6-Astra-Ultra
