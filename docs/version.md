# Version Note

### v2.5.0

#### v2.5.0

Release notes dated 2026-11-05. [Issue #91](https://github.com/cherish-ltt/lynn_tcp/issues/91) / [PR #99](https://github.com/cherish-ltt/lynn_tcp/pull/99) add background broadcast; [Issue #95](https://github.com/cherish-ltt/lynn_tcp/issues/95) / [PR #100](https://github.com/cherish-ltt/lynn_tcp/pull/100) add asynchronous request middleware. Both use the existing `server` feature and TCP/TLS/KCP paths. See the [full changelog](update_logs/v2.5.0.md), [middleware guide](middleware.md) and [performance comparison](middleware-performance.md).

1.feat — background broadcast

- Obtain a clonable `BroadcastHandle` with `LynnServer::broadcast_handle()` before `start()` consumes the server. Application-owned background work calls `broadcast(method_id, payload: Bytes)` without an inbound request.
- Snapshot connection senders and attempt one non-waiting enqueue per bounded queue, returning `BroadcastReport { enqueued, full, closed }`; full/closed targets do not prevent other enqueue attempts. Existing Handler responses retain waiting backpressure.
- Handles use instance-specific frame markers and do not keep the server, tasks or AppState alive. Calls before startup or after cancellation fail; concurrent connection changes may be absent from the snapshot. Empty payloads are valid and payloads above 10 MiB minus 5 bytes are rejected before encoding/enqueue.
- Queue acceptance is not write completion, transport acknowledgment or application delivery. Cancellation may discard accepted frames and whole-broadcast retries may duplicate delivery; scheduling, cancellation and confirmation belong to the application. See the [runnable example](../examples/background_broadcast.rs).

2.feat — asynchronous middleware

- Register global layers with `with_middleware` and route layers with `add_router_with_middleware` / `MiddlewareChain`. Global layers wrap route layers; registration order is entry order and return order is reversed. Plans freeze before startup; duplicate routes replace the Handler and local chain together.
- `Next::run(self, request)` consumes both values and can continue once. Layers may short-circuit, map errors or replace responses through existing validated `HandlerResult` constructors. No retry/rerouting, default authentication policy or generic rejection frame is introduced.
- `MiddlewareRequest` exposes read-only request metadata/payload and shared AppState access, plus isolated typed request data. Data allocates on first insert and freezes into a Handler's read-only `RequestContext`; missing values return `None`. `ResponseView` provides read-only response metadata, payload and destinations.
- Preserve request permits through the entire chain and final enqueue; normal EOF waits for that work. Layers run inside existing owned tasks. Empty chains bypass middleware request/Next/type-map construction; heartbeats, unknown routes, malformed frames and proactive broadcast bypass the inbound chain.
- Timeout covers only the wrapped inner Future, excluding queues, outer return work and network sends. It cannot preempt non-yielding synchronous code or undo side effects. Panic/cancellation does not guarantee async return cleanup. Unhandled errors are logged/dropped once without exposing internal errors or closing a healthy connection.
- Handler duration still covers terminal Handler execution and parameter extraction; short-circuits create no Handler sample. Chain tracing and application success need separate interpretation; transport counters retain their actual I/O boundary.

3.compatibility and upgrade

- Update to `lynn_tcp = "=2.5.0"`. Existing `add_router`, ordinary Handler signatures, valid wire formats and default features remain usable unchanged. Select `default-features = false, features = ["server"]` for a server-only dependency.
- Run `cargo run --example background_broadcast` or `cargo run --example middleware_example` to try the new APIs. Validate identity before insertion, reject missing identity explicitly, place error mapping outside fallible layers and design transaction/idempotency behavior around timeout.
- Retain Rust 1.99.1, Tokio `=1.52.3`, bytes `=1.11.1` and optional kcp-ovo `=0.8.0`. Follow the [KCP guide](kcp.md) for both endpoints and older upstream configuration-type migration. KCP remains unauthenticated/unencrypted; TLS+KCP and native stream mode remain rejected. Earlier upgrades still require the [v2.3.0 return-type migration](update_logs/v2.3.0.md) and [metrics migration](../METRICS.md).

4.validation and measured limits

- PR #99, PR #100 and merged main `190be310f31feadb311d3c0c74343b036ecc6fe2` each passed all 11 CI jobs; the final PR CI passed 201 default / 259 all-feature tests. Separate pre-release production line coverage was 93.52% (4462/4771). These are feature-integration results, not verification of the subsequent version bump or publication.
- After the 2.5.0 version bump, all 13 `prek run --all-files` hooks passed, including all-feature check/Clippy/tests. Old cached Node environments produced version-mismatch notices; every effective hook ran successfully. Fixing the SeaORM `DbConn` documentation link allowed all-feature rustdoc with `-D warnings` to pass. `cargo package --locked --allow-dirty --all-features` packaged 243 files and built the extracted package; new APIs, examples and release notes are included. Production logic and dependency versions are unchanged.
- Pre-release TCP loopback measurements used Apple M1 Pro/macOS arm64, Rust 1.99.1, 128-byte payloads, 1/64 clients, two-second warmup and five-second windows. All 24 cells and 780 client outcomes completed successfully, with 3,892,394 requests and equally many payload-checked responses. The harness now rejects warmup exits/incomplete clients and retains client counts when server shutdown fails.
- At 64 clients, three-run median throughput was 71,281.4 / 63,149.2 / 65,974.2 / 57,766.6 responses/s for baseline/empty/pass/composed. Ranges overlap and paired directions change; no stable speedup or regression is established. Empty-chain allocation calls stay near baseline, but requested allocation bytes rise about 180 B/handled request (6.3%) and peak RSS about 0.55 MiB; nonempty chains allocate more.
- Allocation/CPU/RSS cover the whole server lifetime, and atomic allocation counting perturbs performance. No allocation-stack or Future-layout profiling attributes the added bytes precisely. Empty-chain structural checks do not establish zero resource cost; real authentication/logging, public networks, Windows and long soaks remain outside this performance study.
- Keep the original [v2.4.2 middleware archive](benchmark/v2.4.2-middleware-apple-m1-pro/manifest.json), versions and hashes: baseline `3011b076e92d24e797f72f9125cc0cc431e2237a`, middleware source/Cargo digest `659feda7fe0293180b95158e5f976c03065bc0b11bb2ea16ae475523082e1d23`. This was measured before the version bump; no v2.5.0 performance rerun is implied. Release CI, GitHub Release and crates.io publication require separate verification; these notes do not assert publication completion.

Signature: GPT-6-Astra-Ultra

### v2.4.2

#### v2.4.2

Released on 2026-11-05. [Issue #97](https://github.com/cherish-ltt/lynn_tcp/issues/97) upgrades the optional KCP dependency from 0.6.0 to 0.8.0 and adapts the low-level transport to upstream byte API changes. See the [full changelog](update_logs/v2.4.2.md), [KCP migration guide](kcp.md) and [upgrade validation](kcp-v0.8.0-validation.md).

1.build / fix — dependency and byte API update

- Pin kcp-ovo `=0.8.0`; retain Rust 1.99.1 and this project's exact Tokio 1.52.3 / bytes 1.11.1 dependencies, although upstream now accepts compatible ranges.
- Adapt header parsing to borrowed slices and receive/output paths to `Vec<u8>`. Preserve complete datagrams for `Kcp::input`, exact declared-payload length validation, message boundaries and bounded queues.
- TCP remains default and KCP opt-in. Keep the custom fast preset, normal mode, cancellation and ACK-based flush; the upstream high-level drain API is not exposed.

2.compatibility and upgrade

- Update to `lynn_tcp = "=2.4.2"`. `KcpConfig.protocol` and `lynn_kcp::KcpOptions` expose the 0.8.0 configuration type; applications passing separately imported older upstream types must also upgrade that dependency or use the re-export.
- Ordinary builders, valid wire formats and handlers are unchanged. KCP remains unauthenticated/unencrypted; TLS+KCP and native stream mode remain rejected. Transport ACKs are not application delivery, and Drop does not drain.

3.validation and limits

- Retain allocator/RTO, input-buffer reuse and reordered/duplicate packet contracts, and add short-header/malformed HELLO length/trailing-byte coverage for the slice parser migration. The dependency-only update failed with two `E0308` errors; the adapted build and all 25 targeted KCP tests pass. Further results belong to the [0.8.0 report](kcp-v0.8.0-validation.md).
- Default/all-feature tests pass at 171/225 including doctests, as do checks/tests for ten isolated feature combinations, KCP package verification and all 13 prek hooks without warnings. Metrics return 400/400 replies with exactly 9200 bytes each direction and five closed connections; mixed 0.6.0/0.8.0 fast/normal processes exchange 3264/3264 exact responses, including 64 large payloads, on macOS arm64 loopback.
- Separate coverage passes 218 tests and measures 93.05% of production lines (4166/4477). LLVM 23.1.1 retains 663 placeholder-mapping warnings; all 44 affected symbols' real counts and the complete mapping denominators were independently checked against the original profile and empty-profile/unit baselines, as detailed in the validation report.
- No 0.8.0 performance measurement is included. Earlier performance samples and their versions, hashes, gains and declines remain unchanged; they do not establish this release's performance. GitHub Release and crates.io publication are verified separately.

Signature: GPT-6-Astra-Ultra

### v2.4.1

#### v2.4.1

Released on 2026-11-04. PR [#96](https://github.com/cherish-ltt/lynn_tcp/pull/96) upgrades the optional KCP dependency from 0.5.2 to 0.6.0 and adds downstream compatibility contracts and paired performance evidence. See the [full changelog](update_logs/v2.4.1.md), [KCP migration guide](kcp.md) and [paired validation report](kcp-v0.6.0-validation.md).

1.build / test — dependency upgrade

- Pin kcp-ovo `=0.6.0`; retain Rust 1.99.1, Tokio 1.52.3 and bytes 1.11.1. Upstream reduces parsing/receive/ACK/queue/allocation overhead; local production transport and benchmark source remain unchanged.
- Add fast/normal input-buffer reuse, single/multi-fragment reception and reordered/duplicate/full-window recovery contracts; the same four dependency tests pass on both versions, including the existing allocator/RTO checks.
- TCP remains default, KCP remains opt-in, and the custom fast preset, message framing, bounded queues, cancellation and ACK flush semantics are unchanged.

2.compatibility and upgrade

- Update to `lynn_tcp = "=2.4.1"`. Although this is a patch release, `KcpConfig.protocol` exposes an upstream type: applications passing a separately imported 0.5.2 configuration must also upgrade that dependency or use the `lynn_kcp::KcpOptions` re-export.
- Ordinary builders and valid wire formats are unchanged. KCP is still unauthenticated/unencrypted; TLS+KCP and native stream mode remain rejected. Transport ACKs are not application delivery; fast nocwnd does not guarantee fairness.

3.verification and measured limits

- Squash commit `1ab96be` has the same tree as validated `8375e18`; its PR CI passed all 11 jobs. Prior local validation passed 171 default / 225 all-feature tests including doctests, ten isolated feature combinations, process blackboxes and all prek hooks; separate line coverage was 92.97% (4165/4480), excluding upstream code.
- At 32 clients, three-run median KCP throughput changed by +18.1% for 128-byte clean loopback, -11.8% for 128-byte/20-ms/30%-loss, and -13.4% for 16-KiB clean loopback. Separately reported 10-second follow-ups gave +9.8%/-4.0%/+3.9%; TCP controls also fluctuated. There is no proven general speedup or stable regression attribution.
- All 132 invocations / 228 cells exited successfully, but the original study retained 149 finite-window reply deficits and one 0/0 cell; the follow-up retained 99 deficits. Successful exit does not establish progress or full delivery, and deficits do not prove permanent loss.
- Preserve pre-release sample names, versions, hashes and all results; no performance or coverage rerun under v2.4.1 is implied. Release packaging and publication are verified separately. Public networks, Windows, long soaks, congestion/fairness, correlated loss and CPU/memory profiles remain unverified.

### v2.4.0

#### v2.4.0

Released on 2026-11-03. PRs [#89](https://github.com/cherish-ltt/lynn_tcp/pull/89) and [#90](https://github.com/cherish-ltt/lynn_tcp/pull/90) deliver real transport metrics (#74) and optional TCP/KCP transport (#76). TCP remains the default. See the [full changelog](update_logs/v2.4.0.md) and [KCP configuration, migration and scenarios](kcp.md).

1.feat — optional low-level KCP

- Enable `kcp` and explicitly configure both builders with `with_kcp(KcpConfig::default())`; one instance selects one transport, without automatic fallback. Handlers, routes, state and framing remain shared.
- Use kcp-ovo `=0.5.2`'s low-level control block, not its high-level Stream/Listener API. Custom fast uses true/10ms/2/true and immediate data/ACK output; normal and custom protocol parameters remain available.
- Preserve bounded queues, 10 MiB frame-body limits, admission, cancellation, reconnect and timeout ownership. Native `stream_mode=true` is rejected because the HELLO/DATA/EOF envelope requires message boundaries; the framework still sees a byte stream.
- KCP is unauthenticated and unencrypted, incompatible with standard KCP and upstream high-level peers. TLS+KCP is rejected; use TCP+TLS for sensitive/untrusted paths. ACK-based flush is not application delivery, and Drop does not drain.

2.feat / fix — observability and measurement

- Automatically count server connection lifecycle, parsed/sent messages, handler durations and successful I/O bytes for TCP/TLS/KCP; metrics aggregate across servers in a process, not clients.
- Remove duplicate manual updates to built-in metrics. OS memory/thread/queue and TCP retransmit/error metrics still require separate collectors; framework byte counts exclude wire overhead and retransmissions.
- Fix tick-delayed KCP data/ACK output and independent request/reply accounting in benchmark model 1. Preserve missing replies and payload validation; historical backfilled request counts do not prove delivery.

3.build / compatibility

- Require Rust/MSRV 1.99.1; align exact Tokio/bytes dependencies to `=1.52.3` / `=1.11.1`, including default TCP builds. Downstream incompatible exact pins must be reconciled and tested.
- Upstream allocator conflict #36 and retransmission-backoff/deadline defect #38 are resolved by kcp-ovo 0.5.2 and verified through downstream contracts; no fork or upstream patch is required.
- Update to `lynn_tcp = "=2.4.0"`; KCP requires UDP reachability, coordinated endpoints and application-level delivery checks. Existing v2.3.0 Result-returning APIs remain unchanged; users of older versions must still apply the v2.3.0 migration.
- Prefer TCP for clean/LAN or bulk-throughput workloads and TCP+TLS for authenticated/encrypted transport. Evaluate fast KCP for latency-sensitive small messages on controlled lossy UDP paths; `nocwnd=true` can harm shared-link fairness. Normal is a comparison preset, not proof of production safety.

4.verification and limitations

- Both PRs were Squash merged; main `966479a` has exactly the same file tree as validated `c9abd3f`. The stacked baseline sync changed ancestry only; the two documentation conflicts preserved that tree, and the new PR head passed all 11 Linux CI jobs.
- Prior validation passed 171 default / 223 all-feature tests including doctests, ten independent feature check/test combinations, three process-blackbox modes at 800/800 each, and metrics at 400/400 replies with 9200 bytes each direction. Separate coverage passed 216 tests and measured 92.68% lines without mismatched-data warnings; upstream coverage is excluded.
- Prior isolated Linux netem measurements (128 bytes, 32 clients, median of three short runs) put KCP at 94.5% of TCP at 20 ms RTT without loss and 2.21/4.40/7.42 times TCP at 10%/20%/30% random loss. Clean loopback reached only 29.9%; some single-client low-loss cases remained slower.
- Primary runs recorded 1,706,037 requests / 1,705,957 replies, with deficits of TCP 76 / KCP 4 inside the finite drain window. Independent 30-second diagnostics received KCP 15336/15336 and TCP 3036/3036, but do not replace primary deficits or replay identical packet traces.
- Performance archives retain their original names and pre-release provenance; no v2.4.0 performance rerun is implied. Public networks, Windows, long soaks, bandwidth fairness, correlated loss and CPU/memory profiles remain unverified. Release-only verification is recorded in the changelog and PR comments.

### v2.3.0

#### v2.3.0

Released on 2026-11-03. Twelve PRs (#77-#88) address 11 bug issues (#64-#73, #75) and a TLS handshake security report. This release contains source-breaking return-type changes despite the minor version increment; valid wire formats and default features are unchanged.

1.sec / fix — bounded admission and server ownership

- Isolate TLS setup from business workers, acquire handshake permits before spawning, and reject excess handshakes immediately. The configured worker count bounds handshake concurrency; global/per-IP quotas, the existing 10-second timeout and owner cancellation remain effective (#81).
- Reserve global connection capacity immediately after accept and retain it through queueing, handshakes and sessions; `None` stays unlimited (#79).
- Cancelling server startup or dropping an unstarted server releases background tasks, connections and state. Cancellation does not drain queues, guarantee graceful TLS shutdown or preempt synchronous user code that never yields (#80).
- Return bind/TLS initialization errors from `LynnServer::start()` and reject zero workers before listening, including the legacy setter (#77, #78).
- The older receive-buffer security report was already fixed by #57 in v2.2.0 and was rechecked and closed. Both security reports remain closed and unpublished, without CVEs; the new fix is delivered through a public PR.

2.fix — client transport and protocol boundaries

- Flush every client frame, terminate on write/flush errors without replaying partial frames, and keep blocked flushing cancellable (#82).
- Apply reconnect delay after both failed attempts and established sessions ending; keep the first attempt immediate and zero-delay behavior explicit (#83).
- Keep custom protocol marks local to each server instance (#84); accept empty and short string fields without consuming bytes on invalid lengths (#85).
- Retain mTLS client identity when explicitly disabling server certificate verification, reject incomplete/invalid identities, and correct both README TLS configuration examples (#86, #87).
- Reject payloads larger than 10 MiB minus 5 bytes at the send-constructor boundary, before full-frame allocation or enqueueing, including raw senders and broadcasts; local rejection does not close healthy connections (#88).

3.compatibility and upgrade

- `LynnServer::start()` now returns `lynn_tcp::Result<()>`; propagate or handle the result and adapt supervisor task output types.
- `HandlerResult::new_with_send` and `new_with_send_to_server` now return `lynn_tcp::Result<HandlerResult>`. Use `?` where supported or explicitly handle errors in handlers; do not silently report an oversized send as successful. `new_without_send` and heartbeat behavior are unchanged.
- Update to `lynn_tcp = "2.3.0"`, configure at least one server worker, review stricter connection/handshake admission, and complete required exchanges before cancellation. Queuing does not guarantee delivery.
- Metrics integration (#74) and TCP/KCP dual transport (#76) remain out of scope. See [the full changelog](update_logs/v2.3.0.md) for migration and limitations.

4.verification evidence

- Merged main `a74be6c` has exactly the same Git tree as validated integration commit `67ae600`. Release-only changes update package metadata and documentation, not Rust source, tests, dependency versions or workflows.
- Prior macOS arm64 / Rust 1.99.1 validation passed 164 default and 192 all-feature tests including doctests, six isolated-feature check/test combinations, build/Clippy/format checks and all prek hooks. Nine main-based PRs passed all seven Linux CI jobs at delivery; three stacked PRs did not trigger standalone CI at that point.
- GitHub automatically ran [Rust CI for final merged main `a74be6c`](https://github.com/cherish-ltt/lynn_tcp/actions/runs/41604514252); all seven jobs passed. This is separate from the waived local reruns.
- A fresh all-feature coverage run passed 185 tests excluding doctests and reported 93.16% line coverage, but warned that eight functions have mismatched data; this does not reliably establish the 80% coverage gate. Host Node-cache version warnings from prek were also retained.
- Per maintainer request, merge and release preparation do not repeat local validation. Production load, public-network conditions, Windows and TLS-flush performance remain unverified; historical benchmarks are not v2.3.0 measurements.

### v2.2.0

#### v2.2.0

Released on 2026-11-02. This maintenance and security release addresses 10 issues (#46-#55) and [GHSA-4hxf-mfq8-qqxx](https://github.com/cherish-ltt/lynn_tcp/security/advisories/GHSA-4hxf-mfq8-qqxx) through eight PRs (#56-#63). Default public API usage and valid wire formats remain compatible, with stricter error and lifecycle behavior.

1.sec — bounded receive parsing

- Client and server share a bounded parser; invalid headers/trailers and invalid frame lengths terminate the transport instead of retaining further input, including when the writer is backpressured.
- Normal fragmented/coalesced frames remain supported. Retained parser data is bounded by 10 MiB + a 10-byte header + one 1 KiB read, not by a process-wide memory limit.

2.fix — lifecycle and I/O deadlines

- Return per-IP quotas across EOF, read/write errors, heartbeat eviction and failed TLS handshakes; prevent final-slot release deadlocks and stale online entries.
- Hold per-connection processing permits through queueing, handler execution and response enqueueing, including panic cleanup; drain accepted responses after normal read EOF.
- Dropping the client or cancelling startup cancels connection, read/write, retry and heartbeat work; cloned senders no longer keep a dropped owner alive.
- Apply configured server I/O deadlines to both plain TCP and TLS. Reads renew an idle deadline; each response write and flush share a deadline, with a separate shutdown deadline. Zero disables deadlines; partial writes are never retried.

3.fix / ci — independent features and transport robustness

- Fix independent client/server builds and add CI for six no-default-feature combinations.
- Accept fragmented metrics request lines within 1024 bytes and a fixed five-second whole-line deadline.
- Derive IPv6 TLS verification names from structured addresses without relaxing certificate checks.
- Saturate the reactor idle counter to prevent overflow after prolonged idle periods.

4.compatibility and verification

- Invalid frames now close their connection, configured nonzero timeouts now take effect, and previously accidental per-connection over-admission is rejected. Client Drop is cancellation, not a delivery or graceful TLS-shutdown guarantee.
- Every response is flushed, including small TLS responses; throughput and batching may change and were not benchmarked. Read timeouts are idle deadlines, not total-frame deadlines.
- Evidence predates merging and release: all eight PRs passed their Linux CI; local combined commit `d811e06` passed 149 default, 170 all-feature, 111 server-only and 122 server+TLS tests including doctests, the remaining isolated-feature checks/Clippy, default/all-feature builds and checks, and all 13 prek hooks.
- Per the maintainer's request, merging and the version bump do not rerun local validation. Coverage reports contain mismatched-function warnings and cannot reliably demonstrate the 80% gate. No production-load, public-network slow-reader, TLS-flush performance, Windows or equivalent wall-clock long-duration validation is claimed.
- Upgrade to `lynn_tcp = "2.2.0"`; middleware support remains planned and is not part of this release.
- Merged main commit `8f7a9a8` has the same Git tree as the validated combination; the release-only changes update metadata/documentation and remove the temporary stacked-PR CI branch filter, without changing Rust source or tests.

### v2.1.0

#### v2.1.0

Engineering hardening release focused on static scanning, safer protocol parsing, clearer internal type boundaries, and dependency refreshes. The public TCP server/client APIs remain compatible with `v2.0.0` default usage.

1.build — quality gates

- Added pre-commit checks for whitespace, conflict markers, oversized files, TOML/YAML/JSON syntax, private keys, secret leakage, spelling, AST rules, and local Rust formatting/check/clippy/tests.
- Kept the commit-stage test command to integration targets (`cargo test --tests --all-features`) so long-running benchmarks do not block ordinary commits.
- Added reasoned Clippy suppressions and a `.typos.toml` exemption for the `wrk` tool name.

2.refactor — protocol and internals

- Rebuilt protocol length/header parsing around fixed-size slices, added short-message boundary coverage, and removed per-byte array construction.
- Converted internal state/routing/function-system containers to tuple newtypes or private representations while preserving behavior.
- Propagated benchmark echo-server setup errors explicitly instead of panicking in fallbacks.

3.build — dependencies and release

- Updated the crate to `2.1.0`; pinned `crossbeam-deque = 0.8.8`, `rustls = 0.23.45`, and `sea-orm = 2.0.3`.
- Fixed spelling in configuration APIs, logs, README tables, and version history.

### v2.0.0

#### v2.0.0

Final release consolidating all `v2.0.0-rc.1` ~ `v2.0.0-rc.3` content: the DDD + Onion architecture refactor, the engineering/quality hardening, and the production capabilities (TLS 1.3, client reconnection, global state injection). Fully backward compatible with default usage.

1.refactor — architecture

- Reorganized the crate into domain / application / infrastructure (DDD + Onion) layers while keeping `lib.rs` (the public API) unchanged. `LynnServer`, `ServerConfig`/Builder, `LynnClient`, `ClientConfig`/Builder, `HandlerResult`, `InputBufVO`, `LynnRouter`, the handler system, `TcpReactor`, `BigBufReader`, `ConnectionLimiter`, validation, metrics and macros all moved into their new layers; internal module paths changed, public module paths (`lynn_server::*`, `lynn_client::*`, `lynn_tcp_dependents::*`) did not.
- Toolchain upgraded: edition 2021 → 2024, Rust 1.88 → 1.98.1, Tokio 1.47.1 → 1.53.1, all dependencies updated and pinned with `=`.
- 7 runnable examples added (basic_server, custom_config_server, custom_protocol, echo_server_client, multi_route_service, custom_protocol_full, metrics_example).

2.fix

- **Custom message marks on response frames** (rc.2): `check_handler_result` encoded the response before `set_marks`, so servers configured with `with_message_header_mark` / `with_message_tail_mark` always replied with the default marks (9177/7719) and custom-mark clients could not parse responses. Marks are now applied before encoding.
- **Client read/write pumps spinning forever on transport errors** (rc.3): a torn-down TLS session returns the same error on every poll, which previously caused an infinite logging loop that wedged a tokio worker and blocked runtime shutdown. Read/write loops now terminate on persistent errors.

3.feat — production capabilities (all optional, disabled by default)

- **TLS 1.3 transport encryption** (feature `tls`): rustls + ring, TLS 1.3 only. Server opts in via `LynnServerConfigBuilder::with_tls(TlsServerConfig)` / `with_tls_cert_paths(cert, key)` (startup fails fast on bad certificates; handshakes run in reactor workers with a 10s cap); client opts in via `LynnClientConfigBuilder::with_tls(TlsClientConfig)` with a CA trust anchor (verification enforced by default), optional SNI override, mutual-TLS client certificates, and an explicit `danger_accept_invalid_certs` escape hatch for development. New `LynnError::Tls` variant and `lynn_tcp::lynn_tls` public module (re-exports `rustls`).
- **Client automatic reconnection**: a connection supervisor retries the initial connect and every disconnect — 3 attempts, 1s apart by default, configurable via `with_reconnect_max_attempts` / `with_reconnect_interval_secs` / `with_connect_timeout_secs`. User-facing channels survive reconnections (stale queued frames are discarded), and `LynnClient::is_connected()` exposes the live state via a `watch` channel.
- **Global state injection** (axum-style `AppState<T>`): `LynnServer::with_state(T)` / `with_state_arc(Arc<T>)` register per-`TypeId` shared state; handlers declare `AppState<T>` parameters (deref to `&T`, several state types can coexist, resolution happens per request so registration order does not matter). Optional feature `seaorm` adds `LynnServer::with_db(DatabaseConnection)` and the `lynn_seaorm::DbConn` alias (sea-orm 2.0.2).

4.refactor — robustness

- Connection pipeline de-coupled from `TcpStream`: `LynnStream` (plain/TLS enum) plus boxed read/write halves (`LynnUser` no longer depends on the concrete transport); the reactor's 9-element event tuple became a `NewSocketTask` struct; handler execution moved into a nested task so a panicking handler (e.g. an unregistered `AppState`) cannot take down reactor workers.

5.bench

- New standardized benchmark harness (`cargo bench --bench benchmark`): two traffic models (ping-pong with RTT percentiles, pipelined send/receive), per-cell fresh echo server **as an independent process** (`bench_echo_server` binary), parametric CLI, Markdown + JSON reporting. Reference run on Apple M1 Pro: Model-1 peak **144,873 resp/s** @1024 clients, Model-2 peak **131,482 resp/s** @256 clients. Raw results archived under `docs/benchmark/`.

6.test / build / ci / docs

- 15 end-to-end integration tests over real TCP plus new unit tests (frame layout, `InputBufVO` parsing, `LynnRouter`, `BigBufReader` framing, config builders) and feature suites (`state_injection.rs`, `tls_integration.rs`, `client_reconnect.rs`).
- Added `AGENTS.md` as the project development constitution; `rust-ci.yml` updated (path filters, concurrency group, pinned toolchain 1.98.1, `clippy --all-targets -- -D warnings`, `--all-features` check/test); added `.rustfmt.toml` and `.clippy.toml`; `release.yml` publishes GitHub Releases from `docs/update_logs/`.
- New `docs/` layout (`update_logs/`, `version.md`, `FlowChart.png`, `FlowChart-v2.png`, `benchmark/`, `monitoring/`); README.md / README_ZH.md kept in sync; dual license `MIT OR Apache-2.0` with `LICENSE-APACHE`.

7.quality

- Test count 16 → 109+; line coverage 19.65% → **92.16%** (`cargo llvm-cov`); clippy clean for both default and `--all-features`.

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

5.bench

- New standardized benchmark harness (`cargo bench --bench benchmark`): two traffic models (ping-pong with RTT percentiles, pipelined send/receive), per-cell fresh echo server **as an independent process** (`bench_echo_server` binary), parametric CLI, Markdown + JSON reporting. First reference run on Apple M1 Pro: Model-1 peak **144,873 resp/s** @1024 clients, Model-2 peak **131,482 resp/s** @256 clients. README gained a new "Benchmarks" section (pre-2.0 tables retired); raw results archived under `docs/benchmark/`.

6.quality

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

- logserver(Server and Client) Now users need to manually initialize the logs

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
