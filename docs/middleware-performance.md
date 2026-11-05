# 中间件性能对照

本对照衡量空链兼容成本与非空链开销，不预设吞吐提升或合格阈值。基准采用独立服务端进程、TCP loopback、相同 echo Handler、4 个服务端 Tokio worker；客户端沿用 `benches/benchmark.rs` 的 model 1，每连接仅一个在途请求，校验响应内容并独立累计成功请求入队与响应计数。

## 场景与计数范围

| 模式 | 服务端行为 |
|---|---|
| baseline | `3011b076e92d24e797f72f9125cc0cc431e2237a`（已合入主动广播），无中间件 |
| empty | 新实现，不注册中间件 |
| pass | 单层直接调用 Next |
| composed | 观测 -> 1 秒 timeout -> 检查公开标记并插入 Principal -> echo |

两版使用同一份 `examples/middleware_bench_server.rs` 和 System 分配计数器；旧版副本仅替换中间件配置模块，保留相同 echo、原子请求计数、服务端配置和停止控制。计数器对成功 `alloc`、`alloc_zeroed`、`realloc` 计一次调用并累加请求尺寸，不统计失败请求，不把 realloc 尺寸解释为净增长。所有模式使用同一计数器，但原子计数会扰动吞吐，分配更多的模式也承担更多计数开销；结果不等同于无计数器的生产性能。

分配和 `getrusage` 的 CPU/峰值 RSS 统计覆盖整个服务端生命周期，包含启动、连接、预热、测量及停止。分配/Handler 是全进程总量除以服务端实际处理数，**不是单个 Handler 的精确分配数或净存活内存**；峰值 RSS 在 macOS 的单位为字节，Linux 转换 KiB 为字节，其他平台的资源字段为 `null`。吞吐/RTT 只统计测量窗口发出的请求（有限结束排空），不包含启动与预热。相同 CPU、allocator 或本机结果不能外推其他平台、并发模型、业务 Handler 或网络。

没有 tracing subscriber，组合链仅计时和调用禁用的 tracing 事件，不包含日志落盘成本。基准的公开字节标记检查只是固定输入上的分支，不代表生产鉴权安全性、密码验证或数据库开销。

## 复测

先完成所有构建，停止其他构建/测试后再串行测量。每格启用全新 server，三轮旋转场景顺序；失败轮次、退出码、日志及请求/响应差值均保留。model 1 保存每客户端的完成状态与预热期失败标记，任一提前断线、超时、内容错误或无测量请求都会使该格失败；服务端停止失败也保留已完成的客户端计数并以非零状态退出。以下路径由复测者选择：

```sh
mkdir /tmp/lynn-baseline
git archive 3011b076e92d24e797f72f9125cc0cc431e2237a | tar -x -C /tmp/lynn-baseline
python3 benches/middleware_compare.py --prepare-baseline /tmp/lynn-baseline
CARGO_TARGET_DIR=/tmp/lynn-baseline-target cargo build --release \
  --manifest-path /tmp/lynn-baseline/Cargo.toml --example middleware_bench_server
cargo build --release --example middleware_bench_server --bench benchmark \
  --message-format=json > /tmp/lynn-current-build.jsonl
```

从 Cargo JSON 中 `target.name == "benchmark"` 的 artifact 读取 `executable`，然后运行：

```sh
python3 benches/middleware_compare.py \
  --benchmark /absolute/path/to/benchmark-executable \
  --server /absolute/path/to/current/middleware_bench_server \
  --baseline-server /tmp/lynn-baseline-target/release/examples/middleware_bench_server \
  --baseline-revision 3011b076e92d24e797f72f9125cc0cc431e2237a \
  --current-revision '<当前 commit 或基线+工作区说明>' \
  --clients 1,64 --warmup 2 --duration 5 --payload 128 --rounds 3 \
  --output docs/benchmark/middleware-local-run
```

脚本要求输出目录不存在，避免覆盖旧样本；每轮保存客户端 JSON、服务端 JSON 和日志，`manifest.json` 记录实际退出码、环境、二进制 SHA-256 与当前 `src/**/*.rs`/Cargo 文件内容摘要。手动复用 benchmark 时，`LYNN_BENCH_SERVER` 指定独立 server；`LYNN_BENCH_SERVER_STATS` 启用其 STOP/统计控制协议，不应给普通 echo server 设置此变量。

## 空链结构证据

`request_context` 的 `empty_request_never_creates_a_data_map_or_shared_snapshot` 检查空请求及冻结后的数据都为 `None`。`Route::run` 的空链分支直接调用原 Handler，绕过 `MiddlewareRequest`/`Next`；`empty_chain_freezes_without_a_middleware_allocation` 检查空链冻结保留原路由 Arc、链为 `None`。这证明没有新增请求类型表和链请求对象分配；整个网络服务仍有既有连接、帧、任务和响应分配，不能据此声称每请求零分配。


## 本次实测

环境：Apple M1 Pro、macOS 27.2 arm64、Rust 1.99.1；128 B payload，1/64 个客户端，2 秒预热、5 秒测量、3 轮旋转顺序。全部构建及功能测试在测量前结束。24 格全部 `exit 0`，每个客户端正常完成窗口；测量请求与响应各 3,892,394 条，逐条内容校验通过，无失败格。原始 [manifest](benchmark/v2.4.2-middleware-apple-m1-pro/manifest.json) 记录源码/工具/二进制摘要和逐轮文件，原始 JSON 与日志均保留。

下表为每项的三轮中位数，括号内是吞吐最小值至最大值；p99 是三个窗口 p99 的中位数，不是将全部 RTT 合并后的 p99。CPU 为全进程 user + system 秒，RSS 为全进程峰值；分配/Handler 口径见前文。

| 客户端 | 模式 | 响应/秒（范围） | p99 ms | 分配调用/Handler | 请求字节/Handler | CPU 秒 | RSS MiB |
|---:|---|---:|---:|---:|---:|---:|---:|
| 1 | baseline | 1,284.2（1,069.2–1,804.2） | 4.645 | 12.099 | 2,952.6 | 2.773 | 4.578 |
| 1 | empty | 1,823.4（1,559.6–2,110.6） | 2.978 | 12.080 | 3,125.8 | 2.956 | 5.062 |
| 1 | pass | 1,602.6（1,258.4–1,709.0） | 3.668 | 13.094 | 3,778.3 | 2.968 | 5.047 |
| 1 | composed | 1,337.4（1,196.8–1,514.6） | 4.604 | 18.119 | 5,619.2 | 2.637 | 5.141 |
| 64 | baseline | 71,281.4（54,494.6–73,608.2） | 2.586 | 12.025 | 2,849.5 | 14.778 | 5.578 |
| 64 | empty | 63,149.2（60,676.2–67,443.2） | 3.036 | 12.024 | 3,030.0 | 16.791 | 6.125 |
| 64 | pass | 65,974.2（49,130.4–74,379.6） | 2.771 | 13.024 | 3,662.3 | 17.472 | 6.156 |
| 64 | composed | 57,766.6（51,598.4–70,706.8） | 4.321 | 18.025 | 5,456.8 | 16.141 | 6.281 |

64 客户端的 empty/pass/composed 吞吐中位数相对 baseline 分别为 -11.4%/-7.4%/-19.0%。轮间区间明显重叠，逐轮变化方向也不一致；1 客户端结果同样波动，不能凭三轮中位数认定稳定吞吐退化或性能提升。这里完整披露观察到的下降，不把非空链开销包装成吞吐收益，也不作统计显著性结论。

可以重复观察到的是分配成本：64 客户端 empty 的调用数基本保持 12 次/Handler，但请求字节中位数约增加 180 B（6.3%）。空链跳过 `MiddlewareRequest`、`Next` 和 map 的运行，并不保证既有任务尺寸不变：所有请求的 `HandlerContext` 新增内联 `RequestContext`，`Route::run` 的 async 状态机同时承载非空分支。它们与分配次数不变、请求字节增加的观察一致；本次未做分配堆栈或 future 布局测量，不能精确归因这 180 B。RSS 中位数增加约 0.55 MiB，也包含二进制及启动成本。

pass 相对 empty 约增加一次分配，对应每层中间件的 boxed Future；composed 约增加六次，对应三层 Future、Principal 的 Arc、类型表桶及冻结类型表的 Arc。启动/连接/停止成本按实际 Handler 数摊销，不能把小数差值逐个解释为业务路径分配。三层链包含额外分支、时钟和类型擦除；原子分配计数器亦增加其测量成本。首版保留这一显式代价，没有零成本承诺。
