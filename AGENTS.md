# AGENTS.md

本文件定义了本项目的 Rust 开发规范与自动化流程。所有贡献者必须严格遵守，并在每次修改代码后及时更新本文件（如有新增规范或调整）。

---

## 1. Git 提交规范

- **范围**：每次提交应独立且完整地对应一个逻辑变更（如单一功能点、缺陷修复或配置调整），禁止混合多个不相关改动；按功能批次顺序组织，单次提交代码变动量建议控制在 300 行以内(仅建议，非强制，可适当突破)，避免大批量改动挤在同一条提交信息中。
- **格式**：`<type>: <中文描述>`
- **常用 type**：
  - `feat` – 新功能
  - `fix` – 修复 bug
  - `docs` – 文档更新
  - `style` – 代码格式（不影响逻辑）
  - `refactor` – 重构
  - `perf` – 性能优化
  - `test` – 测试相关
  - `build` – 构建系统或外部依赖变更
  - `ci` – CI 配置变更
  - `chore` – 杂项（如工具、配置等）
  - `revert` – 回退提交

示例：`feat: 添加用户登录接口`

---

## 2. Rust CI 标准（GitHub Actions）

确保 `.github/workflows/rust-ci.yml` 存在，内容如下：

```yaml
name: Rust CI

on:
  push:
    branches: [ "main", "master" ]
    paths:
      - "**.rs"
      - "**.proto"
      - "**/Cargo.toml"
      - "**/Cargo.lock"
      - ".rustfmt.toml"
      - ".clippy.toml"
      - "rust-toolchain.toml"
      - ".github/workflows/rust-ci.yml"
  pull_request:
    branches: [ "main", "master" ]
    paths:
      - "**.rs"
      - "**.proto"
      - "**/Cargo.toml"
      - "**/Cargo.lock"
      - ".rustfmt.toml"
      - ".clippy.toml"
      - "rust-toolchain.toml"
      - ".github/workflows/rust-ci.yml"

env:
  CARGO_TERM_COLOR: always

concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

jobs:
  check:
    name: Check & Test
    runs-on: ubuntu-latest
    steps:
      - name: Checkout repository
        uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@master
        with:
          toolchain: "1.99.1"
          components: rustfmt, clippy

      - name: Show rustup info
        run: rustup show

      - name: Cache Cargo dependencies
        uses: Swatinem/rust-cache@v2

      - name: Check formatting
        run: cargo fmt --all -- --check

      - name: Run Clippy (lints)
        run: cargo clippy --all-targets -- -D warnings

      - name: Build the project
        run: cargo build --verbose

      - name: Run tests
        run: cargo test --verbose

      - name: Check all features
        run: cargo check --all-features --all-targets

      - name: Run tests (all features)
        run: cargo test --all-features

```

---

## 3. Cargo.toml 配置

- 必须包含完整的包元数据（满足可发布到 [crates.io](https://crates.io) 的要求），例如：
  - `name`、`version`、`edition`、`authors`、`description`、`license`、`repository` 等。
- 依赖项必须**归类**，使用 `#` 注释说明每组依赖的用途。
- 每个依赖必须使用 `version = "x.y.z"` **锁定具体版本**（使用 `=` 号），不得使用范围限定符。
- 使用 `edition = "2024"` 以及环境中的 Rust 版本，例如:`rust-version = "1.95"`

示例结构：

```toml
[package]
name = "my_crate"
version = "0.1.0"
edition = "2024"
rust-version = "1.95"
authors = ["Your Name <email@example.com>"]
description = "A short description"
license = "MIT OR Apache-2.0"
repository = "https://github.com/your/repo"

# 核心依赖
[dependencies]
# 序列化
serde = { version = "=1.0.210", features = ["derive"] }

# 异步并发
tokio = { version = "=1.42.0", features = ["full"] }

# 开发依赖
[dev-dependencies]
# 基准测试
criterion = { version = "=0.5.1" }

# 编译优化配置
[profile.dev]
opt-level = 1
[profile.dev.package."*"]
opt-level = 3
```

---

## 4. 代码格式化（.rustfmt.toml）

项目根目录必须包含 `.rustfmt.toml`，内容如下：

```toml
edition = "2024"
max_width = 100
tab_spaces = 4
reorder_imports = true
reorder_modules = true
newline_style = "Auto"
match_block_trailing_comma = true
```

所有代码必须通过 `cargo fmt --all -- --check` 检查。

---

## 5. Clippy 配置（.clippy.toml）

项目根目录必须包含 `.clippy.toml`，内容如下：

```toml
# ── Clippy Configuration ──
cognitive-complexity-threshold = 15
too-many-arguments-threshold = 5
too-many-lines-threshold = 30
allow-unwrap-in-tests = true
msrv = "1.99.1"
```

所有代码必须通过 `cargo clippy --all-targets -- -D warnings` 检查，无警告。

---

## 6. README.md

- 每次完成任务后需及时更新 `README.md`，至少包含：
  - 项目简介
  - 构建与运行说明
  - 主要功能或使用示例
  - 贡献指南（引用本 AGENTS.md）

---

## 7. .gitignore

必须排除以下内容（示例）：

```
# Rust
/target/
**/*.rs.bk
*.pdb

# macOS
.DS_Store

# IDE
.vscode/
.idea/
*.swp
```

---

## 8. 项目结构

- **使用DDD+洋葱结构，严格遵循此结构开发**
- **禁止单`mod.rs`文件写入大量代码，代码较多时候将代码拆分到更小的带具体名称的代码文件中(如 utils.rs)**
- **保持单代码文件简洁和更细致的 crate 划分以加速增量编译**

---

## 9. 通用原则

- **保持本文件（AGENTS.md）更新**：每次修正代码或引入新规范后，请同步更新此文档。
- **所有变更**必须通过 CI 检查（格式、lint、构建、测试）。
- **版本锁定**：工具链版本统一使用环境中的版本，但需>=1.99.1（如 CI 和 clippy 配置所示）。
- **遵循设计**：改动必须遵循原有结构设计，不得私自添加和修改，除非用户发出明确重构指令。
- **后续开发追加 AGENTS.md 内容**：写入第 10 章节。
- **测试**：编写单元测试，如果已经安装`cargo-llvm-cov`则检测测试覆盖率>=80%。
- **隐私**：任何文件/代码/图片/视频，应注意避免隐私泄露。(只关注项目文件本身，不关注 git 等外部工具)

---

**本文件是项目的“开发宪法”，所有 pull request 和代码审查均应参照其内容。**

## 10. 其他追加内容

### 10.1 版本更新与 update_logs（版本发布必做）

每次发布新版本（含 rc 预发布版本）时，**必须**在 `docs/update_logs/` 目录下新增对应版本的更新日志，并同步更新 `docs/version.md`（英文版历史）。

- **文件命名**：`docs/update_logs/v<主>.<次>.<修订>[-rc.N].md`，例如 `v2.0.0.md`、`v2.0.0-rc.2.md`、`v1.2.5.md`。
  - 文件名与 crate 版本号一致（不带 `-release` 后缀）；tag 命名规则见 10.4。
- **单文件结构**（参考 `docs/update_logs/v2.0.0.md`）：

  ```markdown
  # vX.Y.Z 变更日志

  发布日期：YYYY-MM-DD

  ## 概述
  （本版本的核心主题，1~3 段）

  ## 主要变更
  ### 1. feat / fix / perf / refactor / sec / docs …（按变更类型分节）
  （说明动机、改动点、涉及文件）

  ## 向后兼容性
  （公共 API、feature flags、行为是否有变化）

  ## 升级指南
  （用户从上一版本升级需要做什么）
  ```

- **同时更新**：`docs/version.md` 按既有格式追加该版本的英文摘要（置顶新增小节）。
- **tag 触发发布**：推送形如 `vX.Y.Z` 或 `vX.Y.Z-release` 的 tag 时，GitHub Actions 会自动读取该文件内容发布到 GitHub Release（见 10.4），因此日志文件必须先于（或随同）tag 合入 `main`。

### 10.2 README.md 与 README_ZH.md 同步更新

本项目维护英文 `README.md` 与中文 `README_ZH.md` 两份说明文档，**内容必须保持同步**：

- 任一文件发生结构性变更（新增/删除章节、配置表、示例、徽章、链接、许可证等）时，另一个文件必须同步同等变更，仅语言不同。
- 两份文档顶部的语言互跳链接必须保留且指向正确。
- 版本号、Rust 版本、feature 表、配置表等事实性内容不允许两份文档出现不一致。
- TLS 客户端示例须通过启用 `tls` 的编译与双语片段一致性回归，不得为错误示例改变 builder 返回语义。
- 纯文档提交使用 `docs:` 类型，例如：`docs: 同步更新 README 与 README_ZH 的贡献指南`。

### 10.3 文档与资源目录结构（docs/）

所有非源码文档与静态资源统一收敛到 `docs/` 目录：

```
docs/
├── update_logs/        # 每个版本的中文更新日志（vX.Y.Z.md）
├── version.md          # 全版本英文历史记录
├── kcp.md              # KCP 配置、迁移、建议场景与验证契约
├── kcp-tls.md          # TLS-over-KCP 配置、迁移、认证和资源边界
├── kcp-tls-performance.md # KCP/TCP+TLS/KCP+TLS 独立进程性能对照
├── kcp-performance.md  # KCP 参数及同条件性能研究
├── kcp-v0.6.0-validation.md # kcp-ovo 0.5.2/0.6.0 配对复测
├── kcp-v0.8.0-validation.md # kcp-ovo 0.8.0 API 迁移与升级验证
├── middleware.md       # 请求中间件 API、顺序、错误与生命周期契约
├── graceful-shutdown.md # 限时排空、准入边界、结果与取消契约
├── middleware-performance.md # 中间件独立进程性能对照与复测方法
├── session-handles.md  # 原会话定向发送、关闭观察及授权/清理边界
├── benchmark/          # 基准测试结果存档（JSON，按版本与机型命名）
├── FlowChart.png       # v1.x 架构流程图
├── FlowChart-v2.png    # v2.x（DDD + 洋葱架构）流程图
└── monitoring/         # 监控配置（metrics feature 配套）
    ├── grafana/        #   Grafana Dashboard JSON
    └── prometheus/     #   Prometheus 抓取配置 prometheus.yml
```

- README / METRICS / 工作流中引用这些资源时，必须使用上述 `docs/` 路径。
- 新增文档或图片资源时放入 `docs/` 并在本节登记，不得散落在仓库根目录。

**基准测试（benches/）**：

- 标准化基准由 `benches/benchmark.rs`（压测客户端，harness=false）与 `src/bin/bench_echo_server.rs`（独立进程 echo server）组成，`cargo bench --bench benchmark` 运行。
- server 必须以**独立进程**运行（与真实部署一致），每格（cell）使用全新 server 进程；流量模型、并发梯度、预热/测量窗口见 README「Benchmarks」章节。
- 基准运行时默认禁止初始化日志订阅者（日志会经 stdout 全局锁串行化并污染测量），调试用 `LYNN_BENCH_LOGS=1`。
- 结果 JSON 存档至 `docs/benchmark/`，命名 `v<版本>-<机型>.json`；README 结果表更新时必须与 JSON 同步提交。

### 10.4 GitHub Release 自动发布流程

- **tag 命名**：`v<主>.<次>.<修订>[-rc.N]`（如 `v2.0.0-rc.2`），正式发布可追加 `-release` 后缀（如 `v2.0.0-release`）。
- 推送 tag 到远端后，两个工作流自动触发：
  1. `cargo-publish.yml`：将 crate 发布到 crates.io。
  2. `release.yml`：从 `docs/update_logs/` 查找与 tag 对应的 md 文件（自动尝试 `v<tag>`、去掉 `-release` 后缀、去掉 `v` 前缀等候选名），以其内容作为 Release Notes 创建 GitHub Release；找不到对应日志时使用默认说明并告警。
- 因此**发布前必须确保**：`Cargo.toml` 版本号、tag 名、`docs/update_logs/` 日志文件三者一致。
- 两个发布工作流独立运行，必须分别确认 GitHub Release 和 crates.io 发布成功；不能以其中一个成功替代另一个结果。

### 10.5 静态扫描规范

- 提交前执行 `prek run --all-files`，处理 warning 与 error，不得仅处理错误级诊断。
- 分隔注释使用普通章节标题，不使用装饰线；Clippy 抑制必须通过 `reason` 说明保留原因。
- 扫描修复按逻辑分类提交，公共 API 变化必须同步两份 README 与测试。

### 10.6 追加内容xxx

### 10.7 可选功能（feature flags）规范（v2.0.0-rc.3 起）

新增重量级能力必须以**默认关闭**的可选 feature 提供，保持核心库轻量：

| feature | 依赖 | 内容 | 默认 |
|---------|------|------|------|
| `tls` | `tokio-rustls` / `rustls` / `rustls-pemfile`（ring provider，仅 TLS 1.3） | 服务端与客户端 TLS 传输加密，需配置证书并手动开启（`with_tls` / `with_tls_cert_paths`） | ❌ |
| `seaorm` | `sea-orm`（`runtime-tokio-rustls`） | 数据库句柄内置支持：`LynnServer::with_db(...)` + `lynn_seaorm::DbConn`（即 `AppState<DatabaseConnection>`） | ❌ |

- **编译验证**：feature 门控代码不参与默认构建，CI 与本地提交前必须通过 `cargo check --all-features --all-targets` 与 `cargo test --all-features`（已加入 rust-ci.yml，见第 2 节）。
- **禁止静默降级**：某能力依赖 feature 时，其配置 API（builder 方法、配置结构体）必须同门控，不允许"配置了但被忽略"。
- **TLS 安全基线**：仅启用 TLS 1.3 协议版本；客户端默认必须校验服务端证书（CA 信任锚），跳过校验仅能通过显式的 `danger_accept_invalid_certs` 且必须打日志告警。
- **mTLS 身份**：服务端证书校验策略不得丢弃显式配置的客户端身份；证书与私钥必须成对配置，两种校验模式均须验证真实 mTLS 握手与残缺身份报错。
- **TLS 地址名称**：默认校验名称须从结构化 SocketAddr 提取 IP，支持 IPv4/IPv6；显式 server_name 保持优先，回归测试须验证真实 IP SAN 握手且不能关闭证书校验。
- **AppState 注入**：`LynnServer::with_state(T)` 按 `TypeId` 注册，一个服务可共存多个状态类型；`AppState<T>` 在请求时解析（注册顺序与 `add_router` 无关），未注册时 panic（会被 reactor 的 handler panic 隔离捕获，不影响服务存活），因此状态必须在 `start()` 前注册。
- **传输抽象**：连接管道统一走 `LynnStream`（明文/TLS 枚举），新增传输类型时在枚举上扩展变体并实现 `AsyncRead + AsyncWrite` 委派。

### 10.8 独立 feature 构建

- 角色专用模块须与 `server` / `client` 一致门控，共享类型不得隐式引入另一角色。
- CI 除默认与全 feature 外，还检查并测试关闭默认功能后的无 feature、`client`、`server`、`metrics`、`client,tls`、`server,tls` 六种组合。
- 示例、集成测试和基准目标使用 `required-features` 声明真实依赖；文档中的独立角色配置必须设置 `default-features = false`。

### 10.12 metrics 请求边界

- `/metrics` 与 `/health` 的请求行解析不得依赖 TCP 分片边界；读取须有固定长度上限和整行总期限，并覆盖分片、EOF、超长和超时回归测试。

### 10.15 客户端生命周期

- `LynnClient` 是连接和心跳任务的所有者；释放 owner 或取消 `start()` 必须结束其任务，克隆 sender 不得延长连接生命周期。
- 读写操作属于监督任务的子 future，不得 detach；任一传输方向结束时释放另一方向，阻塞 I/O 与重连等待必须可取消。
- Drop 是异步取消，不保证排空发送队列或 TLS 优雅关闭；需要确认交付的业务必须在释放前完成协议交互。
- TLS Drop 回归保留关闭总期限及 sender 释放断言；未发送 close_notify 的传输可表现为 EOF 或 ConnectionReset，不限定单一系统错误。
- 客户端逐帧执行写入与刷新；刷新失败终止当前连接，阻塞刷新必须可取消，不重试可能已部分发送的帧。

- 重连间隔须覆盖连接失败及已建立连接结束两条路径，首次连接立即尝试；零值禁用等待，等待仍随 owner 取消。

### 10.14 Reactor 长空闲回归

- worker 空闲退避计数必须有界，不得依赖整数溢出；使用测试专用 Tokio 虚拟时钟覆盖长空闲后的真实服务响应。

...

### 10.9 接收解析边界

- 帧头/帧尾标记必须按服务端实例传递，响应编码不得读取跨实例全局配置；回归覆盖同进程同时及先后启动不同协议标记的服务端。

- 字符串字段长度校验不得复用完整帧最小长度；空字段合法，越界及超长读取不得推进游标。

- 发送消息必须在公共构造边界校验 payload 加 5 字节元数据/帧尾的总长度，并返回 `Result`；不得让原始 sender 或广播绕过上限，不得因本地超限输入关闭健康连接。

- 客户端与服务端使用同一有界解析器；错误帧必须显式终止连接，不得当作未收齐继续缓存。
- 实际保留数据限制为最大帧长度、帧头与一次读取的总和；回归须覆盖错误帧持续输入、长度边界、分片/粘包及明文/TLS 双端关闭。

### 10.10 连接生命周期与处理许可

- 连接配额必须随接受事件、TLS 握手及读写会话转移所有权，所有终止路径恰好归还一次；清理旧连接不得移除相同地址的新连接。
- 每连接处理许可覆盖排队、handler 执行及响应入队；panic 也必须释放许可，超限和零许可保留拒绝并告警语义。
- 正常读取 EOF 排空已接收请求及其响应；错误和心跳驱逐终止读写两端，不得持有 DashMap guard 跨异步等待。

### 10.11 服务端 I/O 超时

- 读写超时配置必须传入明文与 TLS 共用的实际异步 I/O；零值禁用，读取有进展即续期，不作为完整帧总期限。
- 写入期限覆盖单次响应及刷新，不得遗留 TLS 小响应缓冲；最终关闭另计期限，超时必须显式报错并回收会话，不得重试可能已部分写出的响应。
- 回归覆盖空闲读取、半帧停滞、读取续期、部分写入、刷新和关闭阻塞，以及零值兼容性。

### 10.16 全局连接准入

- 全局连接上限必须在 accept 后立即预留，并随排队、握手和会话转移；不能以在线表长度代替准入计数。
- 全局和每 IP 许可在失败、EOF、超时及取消后均由连接所有权恰好释放一次。

### 10.17 服务端任务所有权

- 服务端 owner 是取消信号的唯一持有者；worker、handler、分派、读写、心跳和限流清理任务不得延长 owner 生命周期。
- 取消入口、释放未启动实例或初始化失败必须释放后台任务和 AppState；强制取消不承诺队列排空或 TLS 优雅关闭。

### 10.18 TLS 握手隔离

- TLS 初始化须在业务 worker 外执行，以 worker 数为独立并发上限，先取得许可再创建任务；超限立即拒绝，不无限 detach。
- 初始化须保留全局/每 IP 配额和既有握手超时，成功、失败、超时和 owner 取消均回收资源；回归覆盖已有 TLS 请求持续响应。

### 10.19 服务端启动错误

- `LynnServer::start()` 返回 `lynn_tcp::Result<()>`；监听绑定和 TLS 初始化错误必须保留类型并返回调用方，不得只记录日志后正常完成。
- 示例和下游监督任务必须处理启动结果；回归覆盖端口占用的 `AddrInUse` 与 TLS 初始化失败。
- 事件任务池大小为零必须在监听前返回配置错误；新旧 setter 共用此约束，回归覆盖零值拒绝与正值正常请求。

### 10.20 服务端传输指标

- 自动指标仅采集服务端，连接观测所有权随准入许可转移，所有终止路径恰好关闭一次；TCP/TLS 初始化失败不得泄漏 active。
- 网络字节按成功读写的实际长度统计，消息发送计数须在完整写入及刷新后增加；TLS 统计框架字节，不冒充线路流量或确认交付。
- 内置自动采集和手工业务指标必须区分；禁止在示例中伪造内存/连接数据，取消时未排空队列的丢弃统计限制须明确。
- 指标回归须隔离全局指标污染，覆盖真实 TCP/TLS、异常关闭、并发连接和精确字节差值。

### 10.21 可选 KCP 传输

- `kcp` 默认关闭，双端使用 `with_kcp` 显式选择；单实例不自动回退或同时监听。未启用 `kcp-tls` 时 TLS+KCP 必须显式拒绝。
- 使用 kcp-ovo 0.8.0 底层控制块及有界适配，保持现有帧/业务层不变；明文 KCP 会话没有加密认证，不兼容标准 KCP 或上游高层 API。
- 会话先取得总数/每 IP 配额再分派，地址与 conv 同时区分报文；活跃旧会话不被不同 conv 抢占，失效后清理；owner 持有 driver，Drop 必须取消。
- KCP flush 等待已写字节的传输 ACK，shutdown 等待有序 EOF 确认；均不代表业务交付，强制释放不承诺排空。
- KCP 保持 10 MiB 帧 body 上限；适配发送分片、接收队列及逐会话报文队列均有界。
- CI 包含 `kcp`、`client,kcp`、`server,kcp`、`client,server,kcp`；回归覆盖 UDP 仿真、背压、重连、取消及独立进程双传输。
- 依赖升级须保留应用自选全局分配器、fast/normal 连续超时退避及新 RTO 重传期限回归；0.5.2 起已解除上游 #36/#38，不能仅凭发布状态认定本项目组合通过。0.6.0 回归另覆盖输入缓冲复用、单段/多段接收与乱序重复报文后的窗口恢复。
- KCP 默认采用自定义 fast（nodelay/10ms/fastresend=2/nocwnd），新数据及 ACK 立即驱动；normal 保留周期输出。底层参数必须验证，不得启用破坏 HELLO/DATA/EOF 边界的原生 stream_mode。
- 性能测试须独立统计请求和响应、校验内容；不得用响应数反填请求数证明无丢失。TCP/KCP 丢包比较须使用同条件包级网络仿真，不能以 TCP 应用代理替代。
- 高丢包研究保留每轮退出码、请求/响应差值及 qdisc 统计，不删除失败轮次；有限接收期限内的缺失回复不能直接认定为协议永久丢失。
- 依赖复测说明存放于 `docs/kcp-v0.5.2-validation.md`、`docs/kcp-v0.6.0-validation.md` 和 `docs/kcp-v0.8.0-validation.md`，原始数据存放于 `docs/benchmark/`；新旧依赖版本结果分开存档，不覆盖历史数据。性能升级须同源码配对复跑，使用独立 target 并记录二进制哈希；所有构建完成后串行测量，不能把上游微基准收益直接当作本项目端到端收益。
- KCP 发布说明须链接 `docs/kcp.md` 的双端配置、迁移和建议场景，区分发布前实测与发布验证；版本更新不得重命名旧性能样本或承诺普遍快于 TCP。
- v2.4.2 起锁定 kcp-ovo 0.8.0；即使是补丁发布，也须说明公开的 `KcpConfig.protocol` 存在上游版本类型边界，直接传入旧版上游配置的应用需同步升级或使用 `lynn_kcp::KcpOptions` 重导出。
- 上游切片头部解析仅推进头部游标，不保证剩余切片等于声明 payload；必须保留声明长度相等检查及完整报文输入，回归覆盖 HELLO 声明长度偏短/偏长、尾随字节和合法握手。

### 10.22 后台广播

- 广播句柄不持有 server owner 或强引用在线表；启动前及停止后显式报错，不得保活 I/O、后台任务或 AppState。
- 每次记录目标连接的 sender 快照，对有界写队列非等待入队并报告 enqueued/full/closed；不持 DashMap guard 跨等待，不重定向至同地址的新连接，不改变 Handler 的等待背压。
- 广播复用长度校验、实例帧标记和共用 writer；入队不代表交付。回归覆盖满队列隔离、关闭/取消、双实例、空/超限 payload、实际 TCP/TLS/KCP 和发送指标。

### 10.23 请求中间件

- `server` 提供全局及路由级异步链，注册顺序即进入顺序，全局始终包住路由；启动前冻结，重复路由覆盖 Handler 与本地链。
- `Next` 与请求单次消费，不允许重试/重新分派；请求身份与 payload 只读，请求类型表首次插入才分配，`RequestContext` 冻结只读且缺失返回 None，与全局 AppState 分开。
- 空链保留原 Handler 快路径；非空链复用冻结计划，不每层 spawn，不提前释放处理许可；正常 EOF 等待整链与最终响应入队。
- 未处理错误在根部记录一次并计一次 dropped，不回发内部错误、不关闭健康连接；响应替换须走现有长度校验，主动广播不经过入站链。
- Handler 耗时只包含终端调用/参数解析，短路无样本；timeout 不回滚副作用，panic/取消不保证 async 返回阶段运行。
- 示例不得记录凭据/payload 或默认放行身份；性能比较保留基线/空链/透传/组合链的独立进程原始数据与失败轮次，不将全进程分配当作纯 Handler 分配。
- 广播和请求中间件自 v2.5.0 发布；发布前的 `v2.4.2-middleware-apple-m1-pro` 性能存档须保留原名、版本与哈希，不得将版本更新描述为重新测量。

### 10.24 启动就绪

- `startup_handle()` 只观察一次初始化结果；真实绑定地址必须取自 TCP listener 或 KCP UDP socket，配置校验、TLS acceptor 与 worker 准备完成后才报告成功。
- 失败观察保留类型化状态，`start()` 原错误不降级；成功前取消或 owner Drop 必须唤醒等待，晚等待保留结果，观察者不保活资源。
- 就绪不代表实时健康或客户端已连接；动态端口示例直接绑定 `:0`，不得预选端口后释放再绑定或依赖探活。

### 10.25 客户端连接状态订阅

- `client` 的连接状态由监督任务单点发布，`is_connected()` 从同一快照与通道存活状态派生；首次传输初始化成功为第 1 代，每次重连成功递增。
- 订阅仅保留最新快照，允许合并中间变化，不承诺逐帧代次或跨通道排序；慢订阅者不能阻塞监督任务，订阅不能保活 owner。
- 未启动及初次失败返回 None；重试耗尽发布 Stopped 后关闭，Drop/启动取消/再次启动以旧通道关闭为结束信号，缓存 Connected 不代表在线。再次启动须重新订阅，代次仅在同一监督生命周期内比较。
- 正常终止须先关闭发送准入与接收生产端，再发布 Stopped，不能让观察者收到终态后仍成功入队。
- 文档先复制并读取初始快照，再等待 changed 并处理关闭；回归覆盖多订阅、慢读取、接收并发、重试结束及 TCP/TLS/KCP 初始化与取消边界。

### 10.26 原会话能力

- 请求入队时绑定原 `SessionHandle`，延迟执行不得按地址重查；`SessionId` 在实例及连接代次间隔离，应用清理须比较身份。
- 定向推送使用弱队列引用、公共长度校验、实例帧标记和共用 writer；非等待反馈 Enqueued/Full/Closed，不保活 owner、I/O 或 AppState，不改变 Handler 响应背压。
- 观察器独立于发送权限，保留可重复等待的终态；终态须在读写 transport 两端及 KCP driver 实际释放后通知，正常 EOF 先排空请求链及响应并完成 writer shutdown，不能以读 EOF 或发出 abort 代替。
- Handler/中间件持有处理许可时不得等待原会话最终关闭；有界示例须先注册返回再后台推送/等待，说明授权、重连、入队非交付和副作用边界。

### 10.27 服务端限时排空

- 同一服务端的取消/排空状态由轻量共享句柄引用，不在每个请求内逐字段克隆或为事件额外装箱；取消发送端仍只由 server owner 持有。

- `start_with_shutdown` 只接收应用信号，初始化失败保持原错误；总期限从观察到信号计算，零期限强制取消，结果区分正常、失败及超时。
- 停止准入与连接注册、请求入队、后台广播及 SessionHandle 定向推送共享同步边界；发送须持有准入许可直到队列预留及入队完成。新会话及待决 TLS 握手不得越过边界，已入队请求包括中间件返回阶段及最终响应入队。
- 所有 draining writer 等待全实例请求屏障，保留跨连接响应；队列关闭后仍写完已成功入队的消息。Drop/abort 与错误帧/驱逐保持既有立即取消语义。
- KCP 禁止新会话但保留既有 DATA/ACK/EOF 分派；写入、flush、关闭、ACK 和任务释放共用一个期限，必须等待真实 writer、driver 及其持有的状态/配额释放后返回。
- 任务完成计数必须在 future 资源析构后通知；取消前未轮询、等待中取消、正常返回与 panic 均须以析构探针覆盖，不能依赖独立 async 捕获的隐式释放顺序。
- 连接排空计数须由共享 transport 持有并在两半实际析构后释放；KCP driver 另持计数直到任务退出及错误记录完成，不能在 Session 清理或发出 abort 时提前停掉 listener。回归使用跨线程析构门闩并保留真实 listener 失败报告。
- KCP+TLS 排空须取消未认证握手，认证完成后的注册仍持准入许可；真实 mTLS 回归覆盖已入队推送及响应、close_notify/EOF、ACK 丢失超时与边界后发送拒绝，返回前观察器和 driver 均已关闭。
- 超时不重试部分写出的帧、不回滚副作用、不代表业务交付；文档说明协作式取消和非让出同步代码不能抢占，示例须临时端口、自带客户端、校验完整响应并有界退出。

### 10.28 TLS-over-KCP

- `kcp-tls` 默认关闭，复用现有 `kcp`、`tls` 依赖；双端必须同时配置 KCP/TLS，使用独立 `\0lynn-kcp/2` HELLO，不回退明文或 TCP，不更改旧明文线路。
- 只启用 TLS 1.3，强制 CA 与名称校验，保留可选 mTLS 的成对身份校验；拒绝 `danger_accept_invalid_certs`，禁用 0-RTT、恢复、tickets 和 half-RTT 应用数据。
- 客户端总期限覆盖 KCP、TLS 及加密的服务端认证确认，确认前不得报告 Connected。服务端按 worker 数限制握手并发，先取得许可再创建任务，10 秒总期限从准入开始；认证前不得进入 Handler 或广播在线表。
- 逐帧刷新 TLS 记录及底层 KCP ACK；正常关闭先 close_notify 再有序 EOF，异常截断报错，Drop/abort 仍为强制取消。握手、背压与 owner 取消须回收 driver、AppState、任务和连接许可。
- 原会话观察必须同时等待 TLS 读写两端及底层 KCP driver 释放；定向发送复用已认证会话，不跳过认证确认或向重连后的新会话转发。
- TLS 只保护完整 Lynn 业务字节流，KCP 控制字段及 ACK 仍不认证，不承诺抗伪造/放大/DoS 或业务交付；指标只计框架明文字节，不计 TLS 确认、握手、记录开销或重传。
- 保留 1400 字节 UDP 上限、10 MiB body、KCP 既有有界队列并记录 TLS 额外缓冲。回归覆盖证书负例、IPv4/IPv6、模式不匹配、密文修改/重放、弱网、慢握手隔离、关闭与取消。
- CI 包含 `kcp-tls`、`client,kcp-tls`、`server,kcp-tls`、`client,server,kcp-tls`，并保留 `client,server,kcp,tls` 验证未启用组合的拒绝行为。双语配置须编译并做内容一致性回归；独立进程比较保留失败轮次，不将回环结果推广为公网保证。
- KCP+TLS 性能存档保留采样时的 v2.5.0 开发版本与哈希，首次密钥编码失败和 named-curve 完整复测分别留存；建连时间按整批含重试计量，分配/CPU/RSS 按服务端全生命周期计量，不冒充纯握手或 Handler 内部成本。

### 10.29 v2.6.0 发布记录

- 启动就绪、客户端状态、原会话推送/关闭观察、限时排空和 TLS-over-KCP 归入 v2.6.0；版本更新同步 Cargo、双语 README、当前配置示例、中文日志及英文版本历史。
- 最终集成的功能验证与版本/package 预检分别记录；v2.5.0 KCP+TLS 开发性能样本保持原名与哈希，不得将版本更新描述为重新测量。GitHub Release 和 crates.io 必须分别核实。
