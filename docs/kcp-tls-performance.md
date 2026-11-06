# KCP + TLS 独立进程性能对照

本对照使用同一组预构建二进制，比较明文 KCP、KCP+TLS 1.3 和 TCP+TLS 1.3。它衡量指定回环负载下的成本，不承诺加密没有开销，也不能代替目标网络复测。配置和安全边界见 [KCP+TLS](kcp-tls.md)。

## 方法与计量范围

- 每格启动全新服务端进程，使用相同 echo Handler、空中间件链和 4 个 Tokio worker；没有日志订阅者。
- `benches/benchmark.rs` model 1，每客户端一个在途请求，128 B payload；1/32 客户端、1 秒预热、5 秒测量、3 轮。每轮旋转三种传输的顺序，全部构建完成后串行测量。
- KCP 为既有自定义 fast 默认参数；两种 TLS 均为 TLS 1.3。KCP+TLS 禁用恢复和 0-RTT；TCP+TLS 每客户端新建 TLS 配置。本次使用服务端证书认证，没有启用 mTLS。
- 请求入队数与收到的响应数独立累计，逐条校验内容；每客户端必须完成窗口且有非零测量请求。提前断线、内容不符、超时、计数不等或服务端停止失败均使该格失败。测量窗口结束后保留有限响应排空时间。
- RTT 包含请求入队至回复的端到端等待。`connection_setup_ms` 是整个 `connect_clients` 批次的耗时，包含可能的重试，在流量窗口之外；**不是单次或纯 TLS 握手延迟**。
- 同一批中的客户端并发连接。计时前，KCP/KCP+TLS 完成并释放一次真实连接作为探测，TCP+TLS 只进行裸 TCP 探测；探测不计入建连毫秒数，但计入服务端资源总量，因此结果不是冷启动成本。
- 分配、CPU 和峰值 RSS 覆盖服务端整个生命周期，包括启动、连接、预热、测量及停止。分配调用统计成功的 `alloc`、`alloc_zeroed`、`realloc`；请求字节不是净存活内存。全进程分配除以实际 Handler 次数只是摊销口径，不是 Handler 内部精确分配。
- 复用 System 分配计数器，原子计数本身会扰动吞吐；分配更多的模式也承担更多计数成本。CPU 为 user + system 秒，多线程值可以超过墙钟时间；macOS 的峰值 RSS 按字节转为 MiB。

## 2026-11-06 实测

环境为 Apple M1 Pro（8 核、16 GiB）、macOS 27.2 arm64、Rust 1.99.1，UDP/TCP loopback，无注入丢包、延迟或重排。本任务没有并行构建或测试；主机仍有其他桌面负载，逐格记录的 1 分钟 load average 为 6.03–14.77，存在明显轮间波动。

采样时包版本仍为 **2.5.0**，源码基于 `ecfdac20b36cb905a99f1bc14effe5039627efb0` 加尚未提交的 #94 实现。以下存档属于该开发快照；后续功能集成、版本或文档更新不代表重新测量，不得把目录改名为发布版本。

| 证据 | SHA-256 |
|---|---|
| `src/**/*.rs` 与 Cargo 文件摘要 | `5dee7b95ae47fa62f23cdcece6226cee658ba38aa0d7eac9850bfd40d0bb57e5` |
| 构建前 260 文件内容映射摘要 | `4acb446f534348c0496fa8e7d51574a8e0f6f32d6b4eb325937067fa065bce5f` |
| benchmark 二进制 | `b979014e0411da066149d375dbe03472954d7cd788f373b861c85b4b78ff28f8` |
| 独立服务端二进制 | `75362613b6897449bc4fbf0aadb7abcf015324d2f6d57198945b8bf4c38d7971` |

修正下述临时证书生成参数后，完整重跑的 **18/18 格退出码为 0**，请求与响应各 **1,622,486** 条，全部客户端完成窗口且内容校验通过。原始 [manifest](benchmark/v2.5.0-kcp-tls-apple-m1-pro-named-curve/manifest.json) 保存命令、工具/源码/二进制哈希、逐轮计数、资源和对应 JSON/日志。

下表每项为三轮中位数，括号内为最小值至最大值。p99 是三个窗口 p99 的中位数，不是合并所有 RTT 后的 p99。

| 客户端 | 传输 | 响应/秒（范围） | p99 ms（范围） | 整批建连 ms（范围） |
|---:|---|---:|---:|---:|
| 1 | KCP | 794.6（701.2–822.4） | 6.246（5.545–7.868） | 0.557（0.533–1.188） |
| 1 | KCP+TLS | 731.2（698.2–817.2） | 6.819（4.044–7.720） | 0.829（0.799–0.993） |
| 1 | TCP+TLS | 1,314.4（1,258.4–1,317.2） | 4.900（4.542–7.999） | 1.098（0.853–3.121） |
| 32 | KCP | 21,385.2（20,781.4–21,678.4） | 3.229（3.068–3.593） | 4.273（4.250–5.192） |
| 32 | KCP+TLS | 21,204.6（20,800.4–22,894.8） | 3.261（2.884–4.009） | 11.654（11.443–14.786） |
| 32 | TCP+TLS | 65,537.8（50,409.4–71,350.4） | 1.593（1.441–3.148） | 8.785（8.022–24.792） |

| 客户端 | 传输 | 分配调用/Handler（范围） | 请求字节/Handler（范围） | CPU 秒（范围） | 峰值 RSS MiB（范围） |
|---:|---|---:|---:|---:|---:|
| 1 | KCP | 25.245（25.241–25.268） | 7,001.1（6,994.2–7,078.0） | 2.330（2.302–2.808） | 6.578（6.547–6.594） |
| 1 | KCP+TLS | 27.315（27.295–27.334） | 7,495.8（7,445.8–7,545.2） | 2.377（2.321–2.668） | 7.453（7.453–7.469） |
| 1 | TCP+TLS | 14.159（14.154–14.160） | 4,420.8（4,407.9–4,426.1） | 2.495（1.609–2.699） | 7.234（7.188–7.266） |
| 32 | KCP | 25.044（25.041–25.045） | 6,388.2（6,386.1–6,388.7） | 15.830（15.552–15.926） | 7.750（7.734–7.812） |
| 32 | KCP+TLS | 27.068（27.067–27.069） | 6,814.8（6,811.9–6,817.4） | 15.587（13.201–15.666） | 9.125（9.000–9.219） |
| 32 | TCP+TLS | 14.031（14.030–14.035） | 4,089.9（4,076.7–4,091.9） | 15.210（9.887–16.426） | 8.234（8.156–8.281） |

KCP+TLS 相对明文 KCP 的吞吐中位数在 1/32 客户端下分别为 -8.0%/-0.8%，但区间重叠，不能据三轮结果判定稳定差异。32 客户端的分配摊销由约 25.04 增至 27.07 次/Handler，请求字节由约 6,388 增至 6,815，RSS 中位数增加 1.375 MiB。它们包含连接、TLS、队列和计数器成本，本次没有做分配堆栈归因。

本次 TCP+TLS 吞吐更高；这是该回环模型的实际结果。三种模式没有相同流量量级，CPU 总秒数不能直接比较单位请求效率。结果不能推广到弱网、其他 payload、流水线并发、Windows、长时间运行或生产业务，也没有证明外层 KCP 控制报文的安全性。

## 首轮失败与复测材料

首次完整运行的 [manifest](benchmark/v2.5.0-kcp-tls-apple-m1-pro/manifest.json) 和全部日志保留：6 格明文 KCP 成功，12 格 KCP+TLS/TCP+TLS 在服务端初始化阶段失败，错误为 `failed to parse private key as RSA, ECDSA, or EdDSA`。这些格没有业务测量结果，不与成功样本合并，也不填充零吞吐或推算响应数。

提交前的空白检查清除了这 12 个失败文本日志末尾各一个空格；错误、退出码与计数未改动。[原始日志压缩包](benchmark/v2.5.0-kcp-tls-apple-m1-pro/original-failed-logs.tar.gz) 另存逐字节原件，SHA-256 为 `aa3748d4565dc77957facd718468fa0286b740b5606567d9a2f081421501c1a4`。

原因是本机 OpenSSL 4.1.0 生成的临时 EC 私钥使用显式曲线参数，ring 不接受该编码。重新生成时指定 `ec_param_enc:named_curve`，先验证真实安全服务端启动，再用**同一组二进制**完整重跑 18 格。没有为适配测试材料修改产品密码实现或依赖；临时私钥不入库。

## 复测命令

使用自己的有效服务器证书/私钥；本地演示可按以下命令生成短期材料（需要支持这些参数的 OpenSSL）：

```sh
mkdir -m 700 /tmp/lynn-kcp-tls-materials
openssl req -x509 -newkey EC -pkeyopt ec_paramgen_curve:P-256 \
  -pkeyopt ec_param_enc:named_curve -noenc -days 2 \
  -subj '/CN=localhost' -addext 'subjectAltName=DNS:localhost,IP:127.0.0.1' \
  -addext 'basicConstraints=critical,CA:FALSE' \
  -addext 'keyUsage=critical,digitalSignature' -addext 'extendedKeyUsage=serverAuth' \
  -keyout /tmp/lynn-kcp-tls-materials/key.pem \
  -out /tmp/lynn-kcp-tls-materials/cert.pem
chmod 600 /tmp/lynn-kcp-tls-materials/key.pem
CARGO_TARGET_DIR=/tmp/lynn-kcp-tls-target cargo build --locked --release \
  --features kcp-tls --bench benchmark --example middleware_bench_server \
  --message-format=json > /tmp/lynn-kcp-tls-build.jsonl
```

从 Cargo JSON 中 `target.name == "benchmark"` 的 artifact 读取 `executable`，完成所有构建后执行：

```sh
python3 benches/kcp_tls_compare.py \
  --benchmark /absolute/path/from/cargo-json/benchmark-executable \
  --server /tmp/lynn-kcp-tls-target/release/examples/middleware_bench_server \
  --cert /tmp/lynn-kcp-tls-materials/cert.pem \
  --key /tmp/lynn-kcp-tls-materials/key.pem \
  --revision '<实际 commit 与工作区变更说明>' \
  --rounds 3 --clients 1,32 --payload 128 --warmup 1 --duration 5 --timeout 60 \
  --output docs/benchmark/kcp-tls-local-run
```

脚本要求输出目录不存在，不覆盖历史数据；每格有总期限，超时终止该格进程组并继续记录其他格。任一失败使对照退出非零。自签证书仅用于回环复测，服务端身份通过显式信任该证书和 `localhost` 名称验证；生产应使用自己的 PKI 和密钥管理。
