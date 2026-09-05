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
          toolchain: "1.98.1"
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
msrv = "1.98.1"
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
- **版本锁定**：工具链版本统一使用环境中的版本，但需>=1.98.1（如 CI 和 clippy 配置所示）。
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
- 纯文档提交使用 `docs:` 类型，例如：`docs: 同步更新 README 与 README_ZH 的贡献指南`。

### 10.3 文档与资源目录结构（docs/）

所有非源码文档与静态资源统一收敛到 `docs/` 目录：

```
docs/
├── update_logs/        # 每个版本的中文更新日志（vX.Y.Z.md）
├── version.md          # 全版本英文历史记录
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

### 10.5 追加内容xxx

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
- **AppState 注入**：`LynnServer::with_state(T)` 按 `TypeId` 注册，一个服务可共存多个状态类型；`AppState<T>` 在请求时解析（注册顺序与 `add_router` 无关），未注册时 panic（会被 reactor 的 handler panic 隔离捕获，不影响服务存活），因此状态必须在 `start()` 前注册。
- **传输抽象**：连接管道统一走 `LynnStream`（明文/TLS 枚举），新增传输类型时在枚举上扩展变体并实现 `AsyncRead + AsyncWrite` 委派。

...
