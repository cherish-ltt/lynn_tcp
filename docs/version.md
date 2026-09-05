# Version Note

### v2.0.0-rc.3

#### v2.0.0-rc.3

1.feat

- **TLS 1.3 transport encryption** (optional feature `tls`, disabled by default): rustls + ring, TLS 1.3 only. Server opts in via `LynnServerConfigBuilder::with_tls(TlsServerConfig)` / `with_tls_cert_paths(cert, key)` (startup fails fast on bad certificates; handshakes run in reactor workers with a 10s cap); client opts in via `LynnClientConfigBuilder::with_tls(TlsClientConfig)` with a CA trust anchor (verification enforced by default), optional SNI override, mutual-TLS client certificates, and an explicit `danger_accept_invalid_certs` escape hatch for development. New `LynnError::Tls` variant and `lynn_tcp::lynn_tls` public module (re-exports `rustls`).
- **Client automatic reconnection**: a connection supervisor retries the initial connect and every disconnect — 3 attempts, 1s apart by default, configurable via `with_reconnect_max_attempts` / `with_reconnect_interval_secs` / `with_connect_timeout_secs`. User-facing channels survive reconnections (stale queued frames are discarded), and `LynnClient::is_connected()` exposes the live state via a `watch` channel.
- **Global state injection** (axum-style `AppState<T>`): `LynnServer::with_state(T)` / `with_state_arc(Arc<T>)` register per-`TypeId` shared state; handlers declare `AppState<T>` parameters (deref to `&T`, several state types can coexist, resolution happens per request so registration order does not matter). Optional feature `seaorm` adds `LynnServer::with_db(DatabaseConnection)` and the `lynn_seaorm::DbConn` alias (sea-orm 2.0.2).

2.fix

- Client read/write pumps no longer spin forever on transport errors: a torn-down TLS session returns the same error on every poll, which previously caused an infinite logging loop that wedged a tokio worker and blocked runtime shutdown. Read/write loops now terminate on persistent errors.

3.refactor

- Connection pipeline de-coupled from `TcpStream`: `LynnStream` (plain/TLS enum) plus boxed read/write halves (`LynnUser` no longer depends on the concrete transport); the reactor's 9-element event tuple became a `NewSocketTask` struct; handler execution moved into a nested task so a panicking handler (e.g. an unregistered `AppState`) cannot take down reactor workers.

4.build / ci / docs

- CI gained `cargo check --all-features --all-targets` and `cargo test --all-features` steps so feature-gated code is always verified; AGENTS.md documents the new optional-feature conventions (section 10.7).
- New examples: `state_example` (global state) and `tls_example` (TLS 1.3, `required-features = ["tls"]`).
- New integration suites: `state_injection.rs` (5 cases), `tls_integration.rs` (3 cases with rcgen-generated certificates), `client_reconnect.rs` (3 cases). README/README_ZH updated in sync (feature tables, client config table, TLS/state/reconnect sections, roadmap, FAQ).

5.quality

- Test count 88 → 109+; line coverage 91.91% → **92.16%** (`cargo llvm-cov`); clippy clean for both default and `--all-features`.

### v2.0.0-rc.2

#### v2.0.0-rc.2

1.fix

- Server response frames now apply the configured custom message marks **before** encoding. Previously `check_handler_result` built the frame first and set the marks afterwards, so servers using custom header/tail marks (`with_message_header_mark` / `with_message_tail_mark`) always replied with the default marks (9177/7719), breaking custom-mark clients. Default-mark servers were unaffected.

2.test

- Line coverage raised from 19.65% to **91.91%** (`cargo llvm-cov`).
- 15 end-to-end integration tests over real TCP: echo round trip, all handler signatures (0/1/2 params, both orders), broadcast, no-reply handlers, unknown method_id / constructor_id, invalid target addrs, zero process permit, per-IP connection limit, zero max connections, client heartbeat keep-alive, client error paths.
- New unit tests: HandlerResult frame layout, InputBufVO sequential parsing & defensive reads, LynnRouter register/overwrite/concurrency, error types & `ToLynnError`, server/client config builders, BigBufReader framing (partial/sticky packets, bad header, oversized length), message format validation & SafeBuffer.

3.build / ci

- Added `AGENTS.md` as the project development constitution; `rust-ci.yml` updated (path filters, concurrency group, pinned toolchain 1.98.1, `clippy --all-targets -- -D warnings`); added `.rustfmt.toml` and `.clippy.toml`.
- `Cargo.toml`: dependencies grouped by purpose and pinned with `=`; `[profile.dev]` optimization added.
- New `release.yml`: pushing a `v*` tag publishes a GitHub Release using the matching `docs/update_logs/*.md` as release notes.
- `.gitignore` extended (IDE, macOS, local AI tool artifacts).

4.docs

- New `docs/` layout: `update_logs/`, `version.md`, `FlowChart.png`, `FlowChart-v2.png` (new v2 DDD + Onion architecture diagram) and `monitoring/` (Grafana dashboard + Prometheus config moved here).
- README.md / README_ZH.md kept in sync: project logo, dual-license badge, contribution guide referencing AGENTS.md, updated doc links and Rust badge (1.98.1+).
- Dual license: added `LICENSE-APACHE`, crate license is now `MIT OR Apache-2.0`.

### v2.0.0 - release

#### v2.0.0 - release

1.refactor

- **Architecture refactoring**: Restructured to DDD + Onion Architecture
  - `domain/` — Pure business logic (model, routing, handler abstractions)
  - `application/` — Orchestration layer (server, client)
  - `infrastructure/` — Concrete implementations (TCP reactor, metrics, validation, protocol)
  - `src/lib.rs` — Interface layer (public API unchanged)
- Rust edition upgraded to `2024`, rust-version upgraded to `1.95`

2.feat

- **7 runnable examples** covering all framework usage scenarios:
  - `basic_server` — Default server configuration with 3 handler signatures
  - `custom_config_server` — Server with custom config via Builder
  - `custom_protocol` — Custom message header/tail marks
  - `echo_server_client` — Full request-response cycle (Client ↔ Server)
  - `multi_route_service` — Multi-route distribution and verification
  - `custom_protocol_full` — Custom protocol with client-side support
  - `metrics_example` — Prometheus metrics integration demonstration

3.docs

- Rewrite README.md and README_ZH.md for v2.0
- Add complete Architecture documentation
- Add Examples section with run commands
- Add FAQ section
- Add v2.0.0 changelog

### v1.3.x - plan _(skipped — jumped directly to v2.0.0)_

#### v1.3.0-plan

1.feat

- unstable: Add optional TLS(based on rustls/tokio-rustls), enable this option to support efficient and secure communication encryption

### v1.2.x - release

#### v1.2.5 - release

1.feat

- Add Prometheus + Grafana monitoring integration
  - 17 production-grade metrics (connections, messages, network, system)
  - HTTP /metrics endpoint for Prometheus scraping
  - /health check endpoint
  - Timer helper for automatic duration tracking
  - Grafana Dashboard ready-to-use
  - Complete documentation (METRICS.md)
  - Feature flag: `metrics` (auto-enabled with `server`)

2.perf

- Low overhead monitoring: <1% CPU, ~2-3 MB memory
- Optimized metric recording with atomic operations

#### v1.2.4 - release

1.feat

- Add connection limiter (IP-based and global)
- Add connection rate limiting
- Add configurable TCP parameters
- Add server socket options configuration

2.sec

- IP-level connection limits to prevent resource abuse
- Connection rate limiting to prevent DDoS attacks

#### v1.2.3 - release

1.sec

- Add comprehensive input validation
- Add message length validation (max 10MB)
- Add message format validation
- Add connection limiter (per-IP and total)
- Add rate limiter with sliding window
- Add SafeBuffer to prevent overflow

2.fix

- Prevent memory exhaustion attacks
- Prevent buffer overflow
- Prevent protocol confusion attacks

#### v1.2.2 - release

1.fix

- Add thiserror and anyhow for error handling
- Create unified LynnError type system
- Remove 10+ unwrap() calls
- Add proper error propagation

2.refactor

- Improve error handling across router, client config, server config, and buffer reader

#### v1.2.1 - release

1.fix

- Remove unsafe raw pointer usage in router
- Replace with DashMap for thread-safe hashmap
- Fix memory leak in LynnRouter
- Remove unsafe code blocks

2.perf

- Better concurrent performance with DashMap

### v1.2.x - rc

#### v1.2.0-release

Integrate v1.2.0-rc.1,rc.2

#### v1.2.0-rc.2

Optimize network handling performance(by ai-agent GLM4.6)

Improve server throughput and reliability by:
- Using non-blocking operations for client timeout checks
- Adding adaptive buffering for socket writes
- Implementing better work-stealing algorithm
- Adding adaptive idle waiting to reduce CPU usage
- Fixing various error handling issues

We are testing and using `AI agent-GLM4.6` for the first time to optimize and develop a new version

#### v1.2.0-rc.1

1.perf

- Adding actor model on a small scale to improve the performance of high concurrency distribution

### v1.1.x - release

#### v1.1.17 - release

1.perf

- Optimize the working mode of the router

#### v1.1.16 - release

1.perf

- Replace RwLock<HashMap<K, V>> with DashMap(more convenient and high-performance thread-safe map)

#### v1.1.15 - release

1.perf

- update rust version to 1.88.0

#### v1.1.14 - release

1.perf

- Optimize memory usage

- Remove unused code

#### v1.1.13 - release

1.doc

- Add and Update doc

#### v1.1.12 - release

1.perf

- Change the original task listening of each socket to a reactor model to reduce memory usage. This modification optimizes three channels layer and 50% of the memory overhead compared to the previous version(In actual testing, the throughput per second increased by about 20% compared to version v1.1.11, and by about 50% compared to other versions earlier than v1.1.11)

- Simplify code(Abandoned and deleted the original thread_pool,removed some other redundant code)

2.doc

- Add and Update doc

#### v1.1.11 - release

1.perf

- Update thread pool load balancing method, switch from simple rotation training to job stealing algorithm (In actual testing, the throughput per second increased by about 20%)

#### v1.1.10 - release

1.perf

- update rust version to 1.87.0

#### v1.1.9 - release

1.fix

- lifecycle management(The previous lifecycle management was disrupted during the upgrade from v1.1.3 to v1.1.4, so we discontinued v1.1.4 to v1.1.8 and fixed the issue in v1.1.9. Currently, Rust still manages most of the lifecycle automatically, and we only manually closed some critical nodes)

#### v1.1.8 - release

1.perf

- logserver(Server and Clinet) Now users need to manually initialize the logs

#### v1.1.7 - release

1.feat

- Supports IPv4 and IPv6 (Server and Client)

#### v1.1.6 - release

1.perf

- Delete useless code

- Update channel_stize=>64

#### v1.1.5 - release

1.perf

- Big-Endian=>Little-Endian(Use popular architectures (x86/x64, ARM) for Little-Endian instead of using network standard Big-Endian to achieve performance improvements)

#### v1.1.4 - release

1.fix

- While=>Loop

#### v1.1.3 - release

1.fix

- Heartbeat update mechanism:Under the previous heartbeat update mechanism, msg that did not match the tag would also be treated as the correct client. Now, only standard heartbeats are received to update the heartbeat, otherwise the client will be removed in the next heartbeat detection

#### v1.1.2 - release

1.fix

- Link management(DELAYED SEND)
- Log output adjustment

2.docs

- Supplement and modify doc

#### v1.1.1 - release

1.perf

- Overall performance optimization

2.fix

- Fix `LynnConfigBuilder` failed to export correctly

3.refactor

- Structural optimization and adjustment mainly focus on code readability and maintainability

4.docs

- Improve the crate documentation

5.redundancy

- Delete abandoned code

#### v1.1.0 - release

1.feat

- Support asynchronous function tasks with different parameter routing

### v1.0.x - release

#### v1.0.3 - release

1.fix

- verified sticky package bug

#### v1.0.2 - release

1.fix

- Several known bugs

#### v1.0.1 - release

1.docs

- Improve documentation

#### v1.0.0 - release

1.feat

- Tcp server

- Tcp client

- Custom message parsing

- Automatically clean sockets

- Routing service for synchronous tasks
