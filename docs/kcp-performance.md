# KCP fast 配置与性能研究

日期：2026-11-03。本页保留 kcp-ovo `=0.5.0` 的历史研究、当时的问题状态和原始数据，适用 Rust 1.99.1、PR #90；不是 v2.3.0 已发布性能承诺。

**后续 0.5.2 已解决 #36/#38 并完成下游回归，见 [0.5.2 复测](kcp-v0.5.2-validation.md)；当前源码升级至 0.6.0，最新数据与限制见 [0.5.2/0.6.0 配对复测](kcp-v0.6.0-validation.md)。本页“尚未升级/等待上游”等描述仅指升级前，不代表当前状态。**

## 结论

- 原约 20 ms 的 request/response 延迟主要来自适配驱动：新数据和 ACK 等待 10 ms tick；仅打开 `nocwnd` 未消除该延迟。
- 默认改为自定义 fast（true/10/2/true），send/input 后立即调用底层 update 输出；没有把 flush 改成仅入队成功，也没有删除 ACK、窗口或重传。
- 三轮 Linux 容器包级仿真：32 并发、约 20 ms RTT、无主动丢包时，KCP 中位吞吐为 TCP 的 98.5%；1%/5% 丢包时分别高 10.6%/57.4%，p99 明显更低。
- 追加 10%/20%/30% 丢包三轮实测，32 并发时 KCP 吞吐分别为 TCP 的 2.22/4.42/6.93 倍；主测试有 75 个期限内未收到的回复，不能宣称全部交付，详见第 2.1 节。
- **纯回环仍不接近 TCP；1 并发、1% 丢包也没有优势。** 不能从原版 KCP 的 fast 宣传推导所有负载均接近或快于 TCP。
- 当前始终使用底层 `core::kcp::Kcp`，从未使用高阶 Stream/Listener；消息/流与 fast/normal 是不同配置维度，详见 [配置与边界契约](kcp.md)。

## 1. 根因与单因素验证

原提交 `79282d21e3752c5b10e11fa0c87b323c7f50f554` 已使用 nodelay=true、interval=10、fastresend=2，但 nocwnd=false，且仅在 tick 调用 update。上游 send 只入队，input 排入 ACK；update 内部的 ts_flush 仍可阻止立即输出，因此仅修改 fast 参数不足以修复适配调度。

三种变体均使用同一 benchmark、独立 echo server、128 B、model 1、1 秒预热/5 秒测量，macOS arm64 8 逻辑核；每种仅一次，期间可能有其他构建，不作为隔离容量基准。原始记录见 [研究 JSON](benchmark/v2.3.0-arm64-kcp-fast-study.json)。

| 变体 | 并发 | 响应/s | p50 ms |
|---|---:|---:|---:|
| 原配置、定时驱动 | 1 | 48.8 | 19.983 |
| 仅 nocwnd=true、定时驱动 | 1 | 48.0 | 19.845 |
| fast + 即时驱动 | 1 | 711.0 | 1.231 |
| 原配置、定时驱动 | 32 | 1723.6 | 19.653 |
| 仅 nocwnd=true、定时驱动 | 32 | 1441.0 | 20.104 |
| fast + 即时驱动 | 32 | 23492.8 | 1.294 |
| 原配置、定时驱动 | 128 | 7326.2 | 19.094 |
| 仅 nocwnd=true、定时驱动 | 128 | 7593.2 | 18.999 |
| fast + 即时驱动 | 128 | 23215.4 | 4.026 |

定向因果回归 `fast_data_and_ack_do_not_wait_for_the_periodic_tick` 将 tick 拉长到 5000 ms，在真实 UDP 上要求握手、双向数据和两次 ACK flush 在 1 秒内完成。**仅关闭即时输出分支时，0 passed / 1 failed（Elapsed）；恢复后 1 passed / 0 failed。** 使用真实时钟；最初用虚拟时钟搭配真实 UDP 的失败不作为因果证据。

实现边界：仅 nodelay=true 的配置在有效 input/send 后设置公开 ts_flush 并 update；input 前同步 current 供 RTT 采样，周期 tick 继续处理重传和空闲。normal 使用上游 false/100/0/false，保留周期输出。发送背压跟随配置 snd_wnd，接收窗口仍需能容纳完整消息。未修改上游算法。

## 2. 包级 TCP/KCP 对照

环境：Debian 13 临时容器，OrbStack aarch64 Linux `7.2.12-orbstack-00394-gbecdd6130068`，4 逻辑核，Rust `1.99.1 (58cf8f9c3 2026-10-13)`；TCP cubic、SACK=1。只给本次容器 NET_ADMIN，不使用 host 网络、不挂载宿主源码；只修改容器 lo，结束后清理 qdisc 和容器，不修改 macOS 网络。

方法：两种传输使用同一 release 构建、同一 handler、128 B、model 1、并发 1/32；每格全新 server 子进程（4 个 Tokio worker），1 秒预热/5 秒测量，配置 500 ms 尾部接收；关闭 benchmark 日志。尾部等待从发送开始计算剩余时间后才开始接收，实际总期限可能额外包含入队等待，不是严格硬截止。按 clean/delay20/loss1/loss5、每种 TCP 后 KCP 的固定顺序执行三轮，非随机顺序、非统计显著性研究。

netem 对 **TCP 和 UDP 数据包及 ACK** 同样生效，不是 TCP 应用代理；lo 每方向 delay=10 ms，名义 RTT=20 ms，实际 p50 含调度/应用开销。loss 是每包独立随机 1%/5%，seed=42；同种传输每轮重置 qdisc，limit=10000，无带宽限制。相同 seed 不代表两种协议丢失相同业务请求，因其包数/大小/ACK 模式不同。qdisc 统计覆盖两个并发格以及预热和连接建立，不能直接当测量期业务丢失率。

下表分别取三轮吞吐及延迟分位数的中位数；p99 是各轮 p99 的中位数，**不是合并所有 RTT 的 p99**。

| 场景 | 并发 | TCP 响应/s | KCP 响应/s | TCP p50 / p99 ms | KCP p50 / p99 ms |
|---|---:|---:|---:|---|---|
| 无 netem | 1 | 1431.0 | 459.2 | 0.071 / 7.119 | 0.962 / 9.354 |
| 无 netem | 32 | 72295.4 | 21491.0 | 0.140 / 5.395 | 0.648 / 6.596 |
| 20 ms RTT、0% 主动丢包 | 1 | 39.6 | 37.8 | 24.243 / 43.573 | 25.738 / 38.536 |
| 20 ms RTT、0% 主动丢包 | 32 | 1216.0 | 1198.0 | 25.331 / 47.777 | 25.772 / 45.377 |
| 20 ms RTT、1% 丢包 | 1 | 37.6 | 36.4 | 24.468 / 45.921 | 26.373 / 48.952 |
| 20 ms RTT、1% 丢包 | 32 | 1041.6 | 1152.2 | 24.623 / 256.539 | 25.835 / 72.850 |
| 20 ms RTT、5% 丢包 | 1 | 23.0 | 31.0 | 25.194 / 258.456 | 26.694 / 77.621 |
| 20 ms RTT、5% 丢包 | 32 | 634.2 | 998.4 | 24.769 / 354.808 | 26.686 / 88.749 |

48 格合计独立入队 **1,619,132** 次，内容校验通过的响应 **1,619,131** 次。唯一差值是 loss5/TCP/第2轮/32并发：3172 入队、3171 响应；说明一个请求未在尾部期限内收到响应，**不能据此认定 TCP 永久丢失数据**。其余格计数相等也只覆盖本次有限测试窗口。

原始每格计数、p50/p95/p99、时间戳及每次 qdisc 完整统计保留在研究 JSON。例如 loss5/KCP/第3轮累计 sent=38948 包、dropped=2029、结束快照 backlog=4 包，证明发生真实队列丢包而非仅设置参数；不是所有末尾网络包已排空的证明。

### 2.1 追加 10%/20%/30% 丢包（2026-11-03）

代码固定为 `fcd01b730f3e072f04237e30cc4efe387c02f822`，仍使用 kcp-ovo `=0.5.0`，不混入 0.5.1 升级。新建 Debian 13.6 容器，内核/4 核/Rust/TCP cubic/SACK/负载/fast 参数与上一轮一致；`git archive` 拷贝源码、无宿主挂载，不使用 host 网络。release 构建成功后串行测量，不同时在本任务中构建或运行其他性能测试；宿主不是专用物理测试机。每次命令另设 180 秒保护期限，**18 次命令全部 exit=0**，36 格均保留，没有删掉失败/低吞吐轮次。

主测试仍为每方向 10 ms delay、随机丢包、seed=42、limit=10000、不限带宽、128 B、model 1、并发 1/32、1 秒预热/5 秒测量、配置 500 ms 尾部接收。默认心跳 5 秒，单次建连超时 3 秒及原有建连重试保持不变；建连不计入吞吐测量，但 qdisc 包含建连流量。源码、Cargo.lock、release binary 哈希、各轮退出码/时间戳/原始计数/分位数/qdisc 记录保存在 [高丢包原始 JSON](benchmark/v2.3.0-arm64-kcp-highloss-study.json)。

| 20 ms RTT 场景 | 并发 | TCP 响应/s | KCP 响应/s | TCP p50 / p99 ms | KCP p50 / p99 ms |
|---|---:|---:|---:|---|---|
| 10% 丢包 | 1 | 9.4 | 26.2 | 26.482 / 368.295 | 28.735 / 98.079 |
| 10% 丢包 | 32 | 365.0 | 810.2 | 26.114 / 605.454 | 27.363 / 137.018 |
| 20% 丢包 | 1 | 4.0 | 19.2 | 99.558 / 619.202 | 29.737 / 182.502 |
| 20% 丢包 | 32 | 122.2 | 540.0 | 28.441 / 1504.049 | 30.706 / 266.186 |
| 30% 丢包 | 1 | 2.6 | 14.8 | 107.688 / 965.847 | 46.892 / 277.594 |
| 30% 丢包 | 32 | 53.6 | 371.4 | 59.489 / 2151.528 | 64.203 / 451.742 |

各列分别取三轮中位数；p99 仅来自已收到的响应，**不含未回复请求，存在截尾偏差**，尤其 1 并发高丢包样本很少，不适合推导稳定尾延迟。32 并发原始吞吐（响应/s，按轮次）：10% TCP=391.6/365.0/356.6、KCP=838.6/810.2/736.4；20% TCP=112.8/132.8/122.2、KCP=599.2/540.0/528.8；30% TCP=49.8/53.6/60.2、KCP=404.2/371.4/360.2。

三轮累计独立请求/响应如下，不能用成功退出替代交付验收：

| 丢包 | 并发 | TCP 请求 / 响应 / 差值 | KCP 请求 / 响应 / 差值 |
|---|---:|---|---|
| 10% | 1 | 142 / 142 / 0 | 388 / 388 / 0 |
| 10% | 32 | 5573 / 5566 / 7 | 11926 / 11926 / 0 |
| 20% | 1 | 60 / 60 / 0 | 298 / 298 / 0 |
| 20% | 32 | 1858 / 1839 / 19 | 8340 / 8340 / 0 |
| 30% | 1 | 37 / 36 / 1 | 206 / 206 / 0 |
| 30% | 32 | 862 / 818 / 44 | 5683 / 5679 / 4 |

总计 **35373 请求、35298 内容校验通过的响应、75 个差值（TCP 71、KCP 4）**。这些是接收期限内未观测到的回复，未单独记录连接错误原因，不能直接说全部只是“晚到”，也不能认定协议永久丢失。原始文件不将差值抹平。30%/KCP/第3轮 qdisc 实际 sent=11469 包、dropped=4787，最终快照 backlog=0；这只是该快照，不证明所有业务交付。

这些结果支持本次小包、固定 RTT、独立随机丢包下 fast KCP 更有优势；不同 seed、突发相关丢包、拥塞瓶颈与长稳仍未覆盖，不能据此关闭上游 RTO 问题或承诺所有网络都更快。

### 2.2 独立长尾接收诊断（不替换主表）

为检查有限接收期限的影响，只在隔离容器的 `benches/support/model1.rs` 将 DRAIN 从 500 ms 改为 30000 ms，生产传输/配置不变，主仓库源码未修改；重新 release 构建，30% 丢包下再跑三轮、1/32 并发。属于新运行，不是对主表相同丢包轨迹的重放，不能用它覆盖主表的 75 个差值。

| 30% 丢包、30 秒尾部接收 | 三轮请求 / 响应 | 各轮请求 / 响应 |
|---|---|---|
| TCP，1 并发 | 23 / 23 | 11/11、1/1、11/11 |
| TCP，32 并发 | 847 / 846 | 260/259、324/324、263/263 |
| KCP，1 并发 | 208 / 208 | 69/69、70/70、69/69 |
| KCP，32 并发 | 5266 / 5266 | 2087/2087、1740/1740、1439/1439 |

6 次命令全部 exit=0；KCP 三轮合计 **5474/5474** 逐内容回复，TCP **870/869**，仍有一个未回复。该结果说明延长接收等待值得区分，但**不能证明主表差值全部都会恢复**；TCP 遗留差值未记录单请求关闭原因，可能受重传/框架心跳关闭等影响，未将其强行归因为内核或本地协议缺陷。原始诊断单列为 JSON 的 `tail_diagnostic_*`，不展示混合期限的吞吐对比。qdisc 清理后确认为 noqueue，本次容器已删除。

## 3. 为什么不保证纯回环等同 TCP

**已核实的实现差异：** KCP 在用户态协议任务、duplex 和 listener 通道之间传递数据，并有独立 UDP 数据/ACK；每帧 flush 等 KCP ACK。TCP 的 flush 不等远端 ACK，数据路径由内核实现。即使同一业务 API，两者交付边界/调度成本也不相同。

**尚未量化的因素：** 内存复制/分配、mimalloc、运行时调度及每包开销没有用 CPU profiler 分别归因。本次只确认修复定时等待，不能把所有剩余差距都归咎于某一项。小包 ping-pong 下放大发送窗口也不能凭空增加每连接在途请求数；没有为跑分关闭 ACK 等待或改变业务语义。

**部署取舍：** nocwnd=true 关闭拥塞窗口控制而非流控，可能牺牲共享网络公平性；无带宽上限、有限随机丢包的 netem 结果不证明瓶颈拥塞下仍更好。需要部署方在真实带宽/RTT/丢包分布下验证。

## 4. 旧 benchmark 证据纠正

原 model 1 将 responses 加入 requests，未独立统计请求，也未校验 echo 内容。因此此前 PR #89/#90 和历史压测记录（含归档 `*-issue76.json`）中“请求=响应”**不能证明无丢失**；历史文件保留原样，其响应吞吐/延迟仍是当时观测，不重写为新结果。精确交付证据来自另外的 churn 与黑盒断言，不来自旧压测等计数。

现在请求以发送开始时间归属测量窗口，成功本地入队才计数，响应独立计数且验证长度/每字节内容；仍不等同线上发送包数。共用真实 model-1 实现的 `tests/benchmark_accounting.rs` 验证：不回复为 1/0，正确尾部回复为 1/1，错误 payload 导致任务失败。model 2 仍有不同入队/接收窗口及处理许可拒绝，未将它作为无丢失证据或本次 TCP/KCP 对照。

## 5. 上游 RTO 差异

已对已发布 0.5.0 构造丢弃全部输出、直接推进底层时间的最小复现，并静态核实上游 main `317e949712d5111251485f00636f3b6b9676940e`；报告为 [kcp-ovo #38](https://github.com/cherish-ltt/kcp-ovo/issues/38)。

| 模式 | now / RTO / next resendts 序列（ms） |
|---|---|
| normal | 0/200/225 → 225/200/450 → 450/200/675 → 675/200/900 |
| fast | 0/200/200 → 200/400/400 → 400/800/800 → 800/1600/1600 |

上游 fast 将 RTO 翻倍、normal 不累加，且 resendts 用更新前 RTO；[原版 C 固定提交](https://github.com/skywind3000/kcp/blob/b1a7a2101dcbb96017681a500d6b82bbe5a88766/ikcp.c) 则 normal 加 max、fast 加半个 RTO，再用新 RTO 安排期限。它影响连续丢包重传，不是无丢包约 20 ms 的根因；本次实测包括这个未修复差异。没有修改、fork 或发布上游，修复需要另行确认。

## 6. 复跑入口

```sh
cargo test --locked --all-features --test benchmark_accounting --test kcp_process
cargo test --locked --all-features --lib infrastructure::kcp
cargo run --locked --features kcp --example kcp_echo
cargo build --locked --release --features kcp --bin bench_echo_server
LYNN_BENCH_TRANSPORT=kcp LYNN_BENCH_KCP_MODE=fast cargo bench --locked --features kcp --bench benchmark -- --model 1 --clients 1,32 --warmup 1 --duration 5 --json /tmp/kcp-fast.json
# Replace fast with normal to test the upstream normal preset on both endpoints.
```

包级研究须在独立 Linux 容器中复跑：使用 Debian 13、NET_ADMIN、**不得使用 --network=host**，将源码拷贝到 `/work`（不挂载宿主写目录），安装 Rust 1.99.1 / iproute2 / C 构建工具后执行下列命令。输出 `/results` 复制出容器后，仅删除本次容器。该拓扑和循环为本次实际执行方式；复跑不保证相同数值。

```sh
# Inside the dedicated container only; never run tc against the host.
set -eu
test -f /.dockerenv
cd /work
mkdir -p /results
cargo build --locked --release --features kcp --bin bench_echo_server
cargo bench --locked --features kcp --bench benchmark --no-run
bench=$(find target/release/deps -type f -name 'benchmark-*' ! -name '*.d' | head -1)
trap 'tc qdisc del dev lo root 2>/dev/null || true' EXIT
for round in 1 2 3; do
  for scenario in clean delay20 loss1 loss5; do
    for transport in tcp kcp; do
      tc qdisc del dev lo root 2>/dev/null || true
      case "$scenario" in
        clean) ;;
        delay20) tc qdisc add dev lo root netem limit 10000 delay 10ms ;;
        loss1) tc qdisc add dev lo root netem limit 10000 delay 10ms loss random 1% seed 42 ;;
        loss5) tc qdisc add dev lo root netem limit 10000 delay 10ms loss random 5% seed 42 ;;
      esac
      name="$scenario-$transport-$round"
      LYNN_BENCH_TRANSPORT="$transport" LYNN_BENCH_KCP_MODE=fast "$bench" \
        --model 1 --clients 1,32 --warmup 1 --duration 5 \
        --json "/results/$name.json" > "/results/$name.log" 2>&1
      tc -s qdisc show dev lo > "/results/$name.qdisc"
    done
  done
done
```

第 2.1 节复用同一容器准备与构建步骤，将循环的场景替换为 `loss10 loss20 loss30`，case 对应 `loss random 10%/20%/30% seed 42`；每次 benchmark 外包 `timeout 180s`，保存 `$?`、开始/结束 Unix 秒和 `tc -s qdisc show dev lo`。保护超时不能当作通过，非零退出时也须保留日志并继续其余轮次。其他参数及顺序不变；原始 JSON 保留本轮每次实际退出码。

复跑第 2.2 节时，只在隔离副本将 `benches/support/model1.rs` 的 `Duration::from_millis(500)` 改为 `Duration::from_millis(30000)`、重建 benchmark，只选 30% 丢包；仍三轮、两种传输、1/32 并发，输出使用独立 `recovery-` 前缀。不要将该诊断改动带入主测试或生产源码。

## 7. 验证边界

功能提交 `fcd01b7` 的 `cargo test --locked --all-features` 最终 221 passed（含 7 doctest），0 failed；`cargo llvm-cov --locked --all-features --summary-only --fail-under-lines 80` 执行 214 tests，通过，行覆盖 92.90%（4162/4480），driver 98.33%。未排除新增生产代码；覆盖率不含上游实现或安全证明。

配置单元验证 fast/normal/custom 和非法参数；真实 UDP 回归覆盖即时输出、normal/custom 小窗口大消息有序传输、ACK flush、EOF、背压及取消。独立进程黑盒分别执行 TCP、KCP fast、KCP normal，各 16 并发 × 50 次逐内容校验。

全量复测曾发现既有 TLS Drop 用例仅接受 UnexpectedEof、实际返回 ConnectionReset；定向重跑原断言通过，说明存在调度相关差异。按既有 Drop 不保证 TLS close_notify 的契约，测试现同时接受这两种明确关闭结果，保留 2 秒关闭期限与 sender 释放检查，不改生产代码、不接受任意错误。

未执行公网跨主机、Windows、长时间 soak、1024/4096 KCP 完整梯度、带宽瓶颈、公平性及突发相关丢包研究；未量化 CPU/内存热点。KCP 无认证/加密，原生 stream_mode 仍明确拒绝，全局 allocator 冲突和上游 RTO Issue 仍存在；不将性能测试视为生产安全验收。

## 8. 0.5.0 接入验收与问题归属（历史）

**按 Issue #76 的功能与兼容性范围，接入符合预期；按“所有负载都接近或优于 TCP”的性能要求，不成立，也不是该 Issue 的承诺。** TCP 默认保留、KCP 可选、双端共用 handler/路由/状态、底层 API 接入、fast/normal/custom 配置、有界帧和队列、会话/配额/取消/重连及真实指标已有自动化验证。PR #89/#90 仍需管理员审阅合并，测试完成不代表已发布。

| 项目 | 判断与当前状态 | 处理归属 |
|---|---|---|
| 新数据/ACK 等待 tick | 明确的本地适配性能问题，已通过即时驱动修复并做负对照回归 | 本项目已解决，PR #90 / `fcd01b7`，不等待上游 |
| 旧压测请求由响应数回填 | 明确的本地测量缺陷，已独立计数并校验内容，撤回旧无丢失推论 | 本项目已解决，PR #90 / `fcd01b7`，相关评论已纠正 |
| 全局 allocator 冲突 | 锁定的 0.5.0 存在确定的下游编译冲突；上游 [#36](https://github.com/cherish-ltt/kcp-ovo/issues/36) 已关闭、0.5.1 已发布修复 | 不必等待上游；本项目后续升级精确依赖并完整验证即可，本轮为保持压测可比未升级 |
| fast/normal RTO 与 resendts | 明确的上游算法/说明差异；[#38](https://github.com/cherish-ltt/kcp-ovo/issues/38) 仍开放且已认领处理中，发布的 0.5.1 也保留该分支 | 需上游确认/修复并发布，再由本项目升级重测；不能只换 high/low-level API 或 interval 解决 |
| 原生 stream_mode、TLS+KCP、标准 KCP 互通 | 明确不支持且拒绝错误配置/记录线路差异，不是静默失效；框架仍可使用字节流 | 现有范围内不是待修缺陷，若要求支持需本地单独设计，不应假设上游升级自动提供 |
| 纯回环速度、公平性和高丢包长尾 | 实测性能及部署取舍；窗口、用户态任务和 ACK flush 语义不同，不能由跑分单独认定协议缺陷 | 本轮不承诺普遍 TCP parity；上游 RTO 修复后仍需复测，不能预先承诺改善幅度 |

2026-11-03 实时核验：官方 crates.io sparse 索引 0.5.1 未撤回且不再依赖 mimalloc；下载发布包的 SHA-256 为 `1f113852ae221561080d3bbeb42056b0008f7a7535e82fe5b1750eebeefbc5d8`，与索引一致，包内库源码无全局 allocator，RTO 分支仍与 #38 一致。这里是注册表/源码验证，**不是本项目已升级或完成 0.5.1 组合验证**。

小的工具环境提示、Drop 不保证 TLS close_notify 等明确契约不作为接入失败；但 allocator 编译冲突和 RTO 差异不能当作小瑕疵略过。高丢包下有限接收期限的计数差值本身不足以新增“可靠性丢数据”Issue，必须先证明在连接健康、等待恢复后仍丢失。
