# netdebug-ping - Work Plan

## TL;DR (For humans)
**What you'll get:** A desktop network-testing app (Windows-first) whose first tool is Ping: enter a hostname or IPv4/IPv6 address, watch a live stats table (kept to 500 rows) and live graphs of latency, loss, and jitter covering the entire session, with every session saved locally so you can reopen past runs.

**Why this approach:** Tauri 2 + Rust pings natively without a server (browsers can't send ICMP), and SQLite gives durable full history for the graphs while the visible table stays bounded.

**What it will NOT do:** No other network tools yet (traceroute/DNS/port scan), no simultaneous multi-target sessions, no data export, no cloud anything.

**Effort:** Large
**Risk:** Medium - raw ICMP may require admin on Windows/macOS; mitigated by a Day-1 spike plus locale-independent fallbacks (Windows IcmpSendEcho API, POSIX ping with forced English locale).
**Decisions to sanity-check:** React+Vite+TS frontend (defaulted), continuous-only ping (no count mode), single active session, table cap is a code constant (500), prefer-IPv4 DNS with manual family override, per-todo git commits in a freshly initialized repo.

Your next move: approve and start work, or run a high-accuracy review first. Full execution detail follows below.

---

> TL;DR (machine): Large effort, Medium risk (ICMP privileges) - Tauri 2 ping tool: bounded table, full-history SQLite-backed graphs, 14 todos + 4 final verifiers.

## Scope
### Must have
- Git-initialized Tauri 2 desktop app (Windows-first, cross-platform code), Rust backend + Vite/React/TypeScript frontend.
- Ping tool: input accepts DNS hostname, IPv4, or IPv6 (incl. zone-id, parsed to `(Ipv6Addr, scope_id: u32)`: `%<index>` numeric on all platforms, POSIX `%<ifname>` via `if_nametoindex`; Rust `IpAddr` carries no scope, so the pair is threaded to the engine); frontend validation with specific error messages.
- DNS resolution in Rust (`tokio::net::lookup_host`): family selector Auto/IPv4/IPv6; Auto prefers first IPv4 answer, falls back to IPv6; all answers + the selected IP displayed in UI; re-resolve at every session start.
- Ping engine: enum-dispatched `PingEngine { Surge(SurgePinger), WinIcmp(WinIcmpPinger), OsPosix(OsPinger), #[cfg(test)] Mock(MockPinger) }` with inherent `async fn probe` — no async-trait crate, no trait objects. surge-ping (ICMPv4/v6) is the preferred engine. Fallbacks: Windows = IPHlpAPI via direct `windows`-crate FFI — IPv4 via `IcmpCreateFile`/`IcmpSendEcho`/`IcmpParseReplies`, IPv6 via `Icmp6CreateFile`/`Icmp6SendEcho2`/`Icmp6ParseReplies` with `sockaddr_in6.sin6_scope_id` carrying the zone-id (unprivileged, locale-independent, no process spawn); POSIX = OS `ping` binary spawned via `tokio::process::Command` (absolute path, fixed argv array, child env `LC_ALL=C` forcing English output) with an English-fixture-pinned parser. Runtime privilege probe at session start selects engine; chosen engine recorded on the session; mid-session permission failure raises an error event + UI banner with fallback offer.
- Continuous ping mode only (Start/Stop). Named constants: `PING_INTERVAL_MS=1000`, `PING_TIMEOUT_MS=1000`, `PING_PAYLOAD_BYTES=32`, `TABLE_ROW_CAP=500`.
- Statistics: per-probe seq/RTT/loss/timestamp; O(1) incremental aggregates over the FULL session (count, min, avg, max, population stddev via Welford, jitter via RFC 3550 estimator `J += (|D| - J)/16`, loss%) — never recomputed from the capped table.
- Bounded stats table: chronological append with autoscroll; rows beyond `TABLE_ROW_CAP` drop from the TABLE ONLY; aggregates unaffected.
- uPlot graphs over the WHOLE session history: RTT line (loss shown as gaps + red markers), loss% and jitter series, aggregates displayed numerically. Render-time downsampling via min-max buckets per pixel column — preserves global extrema and loss markers; naive decimation forbidden.
- SQLite persistence (sqlx + `sqlx::migrate!()`, schema v1: `sessions`, `probes`): live session written incrementally in batches (flush every 50 probes or 500 ms, and on clean stop). Durability contract: a hard crash may lose at most the current unflushed batch window; all previously flushed probes survive.
- Past-session browser: list, reopen (read-only table+graphs), delete. Single active session at a time.
### Must NOT have (guardrails, anti-slop, scope boundaries)
- No other network tools (traceroute, DNS lookup, port scan); app shell is a STATIC sidebar with one "Ping" entry — no plugin registry, tool manifest, or routing framework.
- No settings persistence store; `TABLE_ROW_CAP` and timing knobs are code constants.
- No concurrent multi-target sessions; no count mode (`-n N`); continuous only.
- No CSV/JSON export (rejected with the SQLite choice); no alerting, accounts, cloud sync.
- No installer/signing work beyond default `tauri build`.
- No naive-recompute aggregates, no naive-decimation downsampling, no locale-dependent parsing (POSIX fallback forces `LC_ALL=C`; Windows fallback uses the locale-independent IcmpSendEcho API), no Tauri shell-plugin capability granted to the frontend.

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: tests-after. Backend: `cargo test` (unit + integration). Frontend: `vitest` (validator/stats formatting) + Playwright against the Vite dev server with mocked Tauri IPC for UI QA.
- Engine integration tests run against `127.0.0.1` / `::1` (happy) and `192.0.2.1` TEST-NET-1 (timeout/loss path); OS-ping adapter tested with a fake ping binary emitting pinned English-locale fixtures; privilege-denial simulated via injected failure in the `Pinger` factory.
- Evidence: .omo/evidence/task-<N>-netdebug-ping.<ext> (outside ulw-loop).

## Execution strategy
### Parallel execution waves
> Target 5-8 todos per wave. Fewer than 3 (except the final) means you under-split.
- Wave 1: 1 (scaffold — blocks everything)
- Wave 2: 2 (privilege spike), 3 (DB layer)
- Wave 3: 4 (Pinger trait + surge engine + DNS), 5 (OS-ping adapter), 6 (stats engine)
- Wave 4: 7 (session lifecycle + event pipeline + batched SQLite writer)
- Wave 5: 8 (app shell), 9 (target input), 10 (stats table)
- Wave 6: 11 (graphs), 12 (past-session browser)
- Wave 7: 13 (E2E wiring + integration tests), 14 (error hardening + README)

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 | - | all | - |
| 2 | 1 | 4 (engine default) | 3 |
| 3 | 1 | 7, 12 | 2 |
| 4 | 1, 2 | 7 | 5, 6 |
| 5 | 1 | 7 | 4, 6 |
| 6 | 1 | 7, 10, 11 | 4, 5 |
| 7 | 3, 4, 5, 6 | 9, 10, 11, 12, 13 | - |
| 8 | 1 | - | 9, 10 |
| 9 | 7 | 13 | 8, 10 |
| 10 | 6, 7 | 13 | 8, 9 |
| 11 | 6, 7 | 13 | 12 |
| 12 | 3, 7 | 13 | 11 |
| 13 | 9, 10, 11, 12 | 14 | - |
| 14 | 13 | - | - |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [x] 1. Scaffold Tauri 2 + Vite/React/TS project with git init
  What to do / Must NOT do: Initialize git repo; create Tauri 2 app structure: `src-tauri/` (Rust, tokio, tauri v2 deps) + `src/` (Vite + React 18 + TypeScript strict); add vitest + Playwright dev deps; `.gitignore` (node_modules, target, dist); placeholder window showing app name "verkkokyyla". Must NOT add any UI framework beyond React, no state library yet, no tailwind/CSS framework (plain CSS modules).
  Parallelization: Wave 1 | Blocked by: - | Blocks: all
  References (executor has NO interview context - be exhaustive): https://v2.tauri.app/start/create-project/ ; surge-ping 0.9.0 (github.com/kolapapa/surge-ping); sqlx sqlite feature (docs.rs/sqlx); uPlot (github.com/leeoniya/uplot)
  Acceptance criteria (agent-executable): `cargo tauri dev` launches a window rendering "verkkokyyla"; `cargo test` exits 0 (empty suite); `npx vitest run` exits 0; `git log --oneline` shows the scaffold commit.
  QA scenarios (name the exact tool + invocation): happy: `cargo tauri dev` stays up 30s, screenshot evidence. failure: `cargo tauri build` on clean checkout succeeds, proving deps pinned. Evidence .omo/evidence/task-1-netdebug-ping.png/.txt
  Commit: Y | chore(scaffold): Tauri 2 + React/TS app skeleton

- [x] 2. Privilege spike: surge-ping raw/datagram socket on Windows (and macOS if available)
  What to do / Must NOT do: Add surge-ping 0.9 to src-tauri; write `src-tauri/src/bin/ping_spike.rs` attempting ICMPv4 echo to 127.0.0.1 and ICMPv6 to ::1 via surge-ping as the CURRENT unprivileged user, AND (Windows only) IPv4 via IPHlpAPI `IcmpSendEcho` and IPv6 via `Icmp6SendEcho2` to ::1; record exact error codes on failure; write spike report recommending ENGINE default per platform. Must NOT build the production engine yet; spike code lives in a bin target, not the app.
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 4
  References: github.com/kolapapa/surge-ping README (Linux ping_group_range, CAP_NET_RAW); docs.rs/surge-ping/0.9.0/src/surge_ping/client.rs.html#L24-L111; https://learn.microsoft.com/windows/win32/api/icmpapi/nf-icmpapi-icmpsendecho ; https://learn.microsoft.com/windows/win32/api/icmpapi/nf-icmpapi-icmp6sendecho2
  Acceptance criteria: `cargo run --bin ping_spike` prints PASS/FAIL per family per method (surge + IcmpSendEcho + Icmp6SendEcho2) with OS error; report at .omo/evidence/task-2-netdebug-ping.md states the per-platform engine default decision explicitly.
  QA scenarios: happy: run unprivileged, capture output. failure: run and force raw-socket failure (documented env/flag), verify error is captured verbatim. Evidence .omo/evidence/task-2-netdebug-ping.md
  Commit: Y | test(spike): surge-ping privilege probe on desktop platforms

- [x] 3. SQLite persistence layer (sqlx + migrations)
  What to do / Must NOT do: sqlx with `sqlite` + `runtime-tokio` + `migrate` features; `sqlx::migrate!()` embedded migrations plus `build.rs` emitting `cargo:rerun-if-changed=migrations`; migration 0001: `sessions(id INTEGER PK, target_input TEXT, resolved_ip TEXT, family TEXT, engine TEXT, interval_ms INT, timeout_ms INT, started_at TEXT, ended_at TEXT NULL)` and `probes(id INTEGER PK, session_id INTEGER FK, seq INT, rtt_ms REAL NULL, loss INTEGER NOT NULL, at TEXT)`; DB file under Tauri app data dir; CRUD: create/finish session, batch-insert probes, list sessions, load probes by session, delete session. Must NOT use rusqlite, no runtime SQL string building outside the repository module, no schema changes without a new migration file.
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 7, 12
  References: docs.rs/sqlx (sqlite, migrate macro); https://v2.tauri.app/develop/ (app data dir via path resolver)
  Acceptance criteria: `cargo test db` passes: fresh DB migrates to v1; second open is a no-op; batch insert of 1000 probes < 500 ms; `SELECT COUNT(*)` helpers correct.
  QA scenarios: happy: cargo test creating session + 1000 probes, reopen, counts match. failure: corrupt migration (tampered file in test tempdir) errors cleanly, no panic. Evidence .omo/evidence/task-3-netdebug-ping.txt
  Commit: Y | feat(db): sqlx SQLite layer with sessions/probes schema v1

- [x] 4. Pinger trait + surge-ping engine + DNS resolution
  What to do / Must NOT do: Define `enum PingEngine { Surge(SurgePinger), WinIcmp(WinIcmpPinger), OsPosix(OsPinger), #[cfg(test)] Mock(MockPinger) }` with inherent `async fn probe(seq) -> ProbeResult` (`ProbeResult = Rtt(Duration) | Timeout | Error(String)`) — enum dispatch only, NO async-trait crate, NO trait objects; `SurgePinger` impl for IPv4/IPv6; zone-id parser producing `(Ipv6Addr, scope_id: u32)`: `%<index>` numeric on all platforms, POSIX `%<ifname>` via `if_nametoindex`, scope passed to surge client config or fallback target string; DNS via `tokio::net::lookup_host` with family rule Auto->prefer IPv4 else IPv6, explicit V4/V6 override; return all answers + selected. Engine loop: continuous until stop token, constants `PING_INTERVAL_MS=1000`, `PING_TIMEOUT_MS=1000`, `PING_PAYLOAD_BYTES=32`; applies todo-2 spike decision for default engine. Must NOT add count mode, no sub-100ms intervals, no hardcoded family preference violating the selector.
  Parallelization: Wave 3 | Blocked by: 1, 2 | Blocks: 7
  References: .omo/evidence/task-2-netdebug-ping.md (spike outcome); docs.rs/surge-ping/0.9.0; docs.rs/tokio (lookup_host); libc `if_nametoindex` via `nix` or `libc` crate
  Acceptance criteria: `cargo test pinger` passes via the `Mock` variant; zone-id parser unit tests pass for `fe80::1%3` and `fe80::1%eth0` (if_nametoindex mocked) WITHOUT network probing; `cargo test -- --ignored integration_ping` against 127.0.0.1/::1: 5 probes, >=4 replies, RTT >= 0 (skipped gracefully when unprivileged).
  QA scenarios: happy: loopback v4+v6 replies recorded. failure: 192.0.2.1 yields Timeout per probe; bad hostname `exa mple.com` rejected by lookup with error event. Evidence .omo/evidence/task-4-netdebug-ping.txt
  Commit: Y | feat(engine): Pinger trait, surge-ping impl, DNS resolution

- [x] 5. Platform fallback adapters (Windows IcmpSendEcho + POSIX OS-ping)
  What to do / Must NOT do: `WinIcmpPinger` (Windows only, `#[cfg(windows)]`): IPHlpAPI via direct `windows`-crate FFI — IPv4 via `IcmpCreateFile`/`IcmpSendEcho`/`IcmpParseReplies`; IPv6 via `Icmp6CreateFile`/`Icmp6SendEcho2`/`Icmp6ParseReplies` using `sockaddr_in6` with `sin6_scope_id` set from the parsed zone-id — unprivileged, locale-independent, NO process spawn; `OsPinger` (POSIX only): spawn the ping binary via `tokio::process::Command` with absolute resolved path (discovery order `/bin/ping`, `/sbin/ping`, `/usr/bin/ping`), fixed argv array `-c 1 -W <sec>` (Linux) / `-c 1 -W <ms>` (macOS), and child env `LC_ALL=C` to force English output; English-fixture-pinned parser; parse failure => record probe as loss + log raw line + emit warning event. Backend-only: NO Tauri shell-plugin capability is granted anywhere. Must NOT parse localized output heuristically, no shell:true/string interpolation (argv array only), no arbitrary binary execution, no POSIX adapter on Windows, no IcmpSendEcho (v4 API) for IPv6 targets.
  Parallelization: Wave 3 | Blocked by: 1 | Blocks: 7
  References: https://learn.microsoft.com/windows/win32/api/icmpapi/nf-icmpapi-icmpsendecho (+ IcmpCreateFile/IcmpParseReplies); https://learn.microsoft.com/windows/win32/api/icmpapi/nf-icmpapi-icmp6sendecho2 (+ Icmp6CreateFile/Icmp6ParseReplies, sockaddr_in6.sin6_scope_id); `windows` crate `Win32_NetworkManagement_IpHelper`; man7.org/linux/man-pages/man8/ping.8.html ; man page macOS ping (-W waittime ms)
  Acceptance criteria: `cargo test osping` passes using a fake POSIX ping binary emitting pinned English fixtures (RTT parsed; garbled output => loss + warning; missing binary => Error); `#[cfg(windows)] cargo test winicmp` passes unprivileged IPHlpAPI echoes to BOTH 127.0.0.1 (IcmpSendEcho) and ::1 (Icmp6SendEcho2).
  QA scenarios: happy: fake binary prints `time=12.3 ms` fixture, RTT=12.3 parsed; Windows loopback v4+v6 both succeed without admin. failure: fixture with localized/garbled text does NOT silently parse — probe counted as loss with warning, raw line logged. Evidence .omo/evidence/task-5-netdebug-ping.txt
  Commit: Y | feat(engine): Windows IcmpSendEcho + POSIX OS-ping fallback adapters

- [x] 6. Statistics engine (O(1) incremental aggregates)
  What to do / Must NOT do: Pure Rust `StatsEngine` (no Tauri deps): feed(ProbeResult) updates count, loss_count, min, max, Welford mean/M2 (population stddev), RFC 3550 jitter `J += (|D| - J)/16`, loss%; full per-probe history retained in memory for graphing; snapshot() returns aggregates. Must NOT recompute aggregates from any capped/bounded collection; no floating-point sum-of-squares naive stddev (Welford only).
  Parallelization: Wave 3 | Blocked by: 1 | Blocks: 7, 10, 11
  References: RFC 3550 section 6.3.1/appendix A.8 (jitter estimator); Welford algorithm
  Acceptance criteria: `cargo test stats` passes: canned series with known min/avg/max/stddev/jitter within 1e-9; 2-of-10 loss => 20%; correctness test feeding 2000 probes proves aggregates independent of any cap.
  QA scenarios: happy: known-series assertions. failure: all-lost session => avg/min/max None, loss 100%, no NaN. Evidence .omo/evidence/task-6-netdebug-ping.txt
  Commit: Y | feat(stats): incremental aggregates engine with Welford + RFC3550 jitter

- [x] 7. Session lifecycle, Tauri commands/events, batched DB writer
  What to do / Must NOT do: Tauri commands: `start_session(target, family, on_probe: Channel<ProbeEvent>, on_status: Channel<StatusEvent>)` — Tauri 2 IPC channels passed as command arguments (NOT named global events); plus `stop_session`, `get_snapshot`, `list_sessions`, `load_session(id)`, `delete_session(id)`; single-active-session guard (second start => error); privilege probe at session start (surge attempt -> on permission error auto-select the platform fallback, status event names chosen engine); batched writer: flush every 50 probes or 500 ms and on clean stop. Durability contract: a hard crash may lose at most the current unflushed batch window; flushed probes survive. Session row records engine/interval/timeout. Must NOT allow concurrent sessions, no per-probe individual INSERT in the hot path, no frontend polling (channels only), no named `emit` event strings for probe traffic.
  Parallelization: Wave 4 | Blocked by: 3, 4, 5, 6 | Blocks: 9, 10, 11, 12, 13
  References: https://v2.tauri.app/develop/calling-frontend/ (Channel as command argument); todos 3-6 modules
  Acceptance criteria: `cargo test session` passes: start->5 probes->clean stop persists session with 5 probes and correct engine; second concurrent start errors; simulated permission denial selects the platform fallback and emits a status event naming it; batch-window test: simulated crash between flushes loses at most the unflushed window (asserted via DB count bounds) and a clean stop loses nothing.
  QA scenarios: happy: full loopback session persisted, verified via sqlx query. failure: stop without start / double start => typed errors, no panic; crash-simulation between flushes => DB contains all flushed probes, loss bounded by the window. Evidence .omo/evidence/task-7-netdebug-ping.txt
  Commit: Y | feat(session): lifecycle commands, IPC channel pipeline, batched persistence

- [x] 8. App shell with static sidebar
  What to do / Must NOT do: React layout: static sidebar with single "Ping" nav entry (active state), main content area, window title "verkkokyyla"; plain CSS modules, dark-neutral theme; unknown route => redirect to Ping. Must NOT build plugin registry, tool manifests, dynamic routing for future tools, or settings page.
  Parallelization: Wave 5 | Blocked by: 1 | Blocks: -
  References: todo 1 scaffold; https://v2.tauri.app/reference/config/ (window title)
  Acceptance criteria: `npx vitest run` shell test passes; Playwright: launch dev server with mocked IPC, sidebar shows exactly one "Ping" item, unknown path redirects.
  QA scenarios: happy: screenshot of shell with Ping active. failure: navigate to `/nope` => Ping view, no crash. Evidence .omo/evidence/task-8-netdebug-ping.png
  Commit: Y | feat(shell): static sidebar layout with Ping entry

- [x] 9. Target input + validation UI
  What to do / Must NOT do: Input field accepting DNS/IPv4/IPv6 (incl. `%zone`); family selector Auto/IPv4/IPv6; Start/Stop buttons wired to todo-7 commands; resolved-IP display (all answers + selected); validator util (shared logic unit-tested in vitest): accepts `example.com`, `192.168.1.1`, `::1`, `fe80::1%3`; rejects `999.1.1.1`, `exa mple.com`, empty — each with specific message; engine/error banner surface (incl. fallback notice). Must NOT do DNS in the frontend (backend resolves), no silent fallback to a different target than displayed.
  Parallelization: Wave 5 | Blocked by: 7 | Blocks: 13
  References: todo 7 command/event contract; todo 4 DNS semantics
  Acceptance criteria: `npx vitest run` validator tests pass (cases above); Playwright: type `localhost`, Start (mocked IPC) => resolved label shows 127.0.0.1 or ::1; invalid input shows inline error, Start disabled.
  QA scenarios: happy: localhost flow. failure: `exa mple.com` rejected with message; mocked permission-denied status => banner naming fallback engine. Evidence .omo/evidence/task-9-netdebug-ping.png
  Commit: Y | feat(ui): target input, validation, family selector, status banner

- [x] 10. Bounded live stats table
  What to do / Must NOT do: Table of per-probe rows (seq, time, RTT ms or "lost") chronological with autoscroll; rows beyond `TABLE_ROW_CAP=500` dropped from the table ONLY; aggregates bar above table (count, loss%, min/avg/max, stddev, jitter) fed from snapshot events, unaffected by row dropping. Must NOT derive aggregates from table rows; no unbounded DOM growth; no pagination.
  Parallelization: Wave 5 | Blocked by: 6, 7 | Blocks: 13
  References: todo 6 aggregates contract; todo 7 event channel
  Acceptance criteria: `npx vitest run` buffer test passes (600 in => 500 rows, first visible seq=101); Playwright: seed 600 mocked probes => DOM rows == 500 and aggregates show full-session values.
  QA scenarios: happy: 600-probe seed assertions. failure: loss probes render "lost" (not 0 ms); snapshot with None aggregates renders dashes, no NaN text. Evidence .omo/evidence/task-10-netdebug-ping.png
  Commit: Y | feat(ui): bounded stats table with aggregates bar

- [x] 11. uPlot graphs with full-history downsampling
  What to do / Must NOT do: uPlot charts: RTT-over-time (loss = gap + red marker series), loss% and jitter panels; aggregates shown numerically beside; min-max bucket downsampling per pixel column preserving global max/min and every loss marker; frontend coalesces probe events via requestAnimationFrame; reopening a past session renders the same graphs from DB data. Must NOT use naive every-Nth decimation; no chart library swap (no ECharts/recharts); no per-probe setData without batching.
  Parallelization: Wave 6 | Blocked by: 6, 7 | Blocks: 13
  References: github.com/leeoniya/uplot (setData streaming demo); todo 6 series contract
  Acceptance criteria: `npx vitest run` downsampler test passes: 2000-point series downsampled to <= 2x canvas px buckets, global max RTT point preserved, loss count preserved, RTT y-values are `null` at lost probes, and adjacent successful probes across a loss do NOT render as a continuous line; Playwright: seeded session renders canvases (screenshot).
  QA scenarios: happy: 2000-probe render, extrema preserved (asserted in test). failure: 50%-loss session shows gaps/markers, no fabricated zero RTTs. Evidence .omo/evidence/task-11-netdebug-ping.png
  Commit: Y | feat(ui): uPlot full-history graphs with min-max downsampling

- [x] 12. Past-session browser (list / reopen / delete)
  What to do / Must NOT do: Sessions panel listing past sessions (target, resolved IP, started_at, probe count, loss%); reopen => read-only table + graphs loaded via `load_session`; delete with confirm. Disabled while a live session runs (single-session rule). Must NOT add editing, tagging, export, search/filter, or multi-select.
  Parallelization: Wave 6 | Blocked by: 3, 7 | Blocks: 13
  References: todo 3 repository API; todo 7 commands
  Acceptance criteria: Playwright: run mocked 5-probe session, stop, list shows it with count 5; reopen renders same probe count (mocked load_session); delete removes it.
  QA scenarios: happy: list->reopen->delete flow. failure: `load_session` for deleted id => typed error shown, no crash. Evidence .omo/evidence/task-12-netdebug-ping.png
  Commit: Y | feat(ui): past-session browser with reopen and delete

- [x] 13. End-to-end integration wiring + integration tests
  What to do / Must NOT do: Replace mocked IPC seams with real Tauri backend in E2E config (tauri-driver + WebDriver where supported; otherwise cargo-level integration tests as the real-backend gate); assert full loop: input -> live events -> table/graphs -> persisted session -> reopen matches DB (`SELECT COUNT(*) FROM probes WHERE session_id=?`); verify loss path against 192.0.2.1. Must NOT weaken the mocked Playwright suite (keep both), no network-dependent assertions beyond loopback/TEST-NET.
  Parallelization: Wave 7 | Blocked by: 9, 10, 11, 12 | Blocks: 14
  References: https://v2.tauri.app/develop/tests/webdriver/ ; todos 7-12
  Acceptance criteria: `cargo test -- --ignored e2e` passes against real backend: loopback session of 5 probes persisted and re-opened identically; TEST-NET target => all-loss session recorded.
  QA scenarios: happy: loopback E2E. failure: TEST-NET loss E2E + restart-app persistence check. Evidence .omo/evidence/task-13-netdebug-ping.txt
  Commit: Y | test(e2e): real-backend integration and persistence verification

- [x] 14. Error hardening + README
  What to do / Must NOT do: Typed error surface end-to-end (engine, DB, permission) -> user-readable banner states; mid-session privilege revocation => pause + banner + one-click fallback retry; README: build/run/test commands, privilege notes per OS (from spike report), engine selection logic, known limitations (locale-pinned parser, single session). Must NOT add features, no marketing docs, no CHANGELOG.
  Parallelization: Wave 7 | Blocked by: 13 | Blocks: -
  References: .omo/evidence/task-2-netdebug-ping.md; todo 7 status events
  Acceptance criteria: `cargo test` + `npx vitest run` all pass; README commands verified by executing them; simulated revocation test emits pause+banner path.
  QA scenarios: happy: fresh-clone README build succeeds. failure: revocation simulation => UI banner state asserted. Evidence .omo/evidence/task-14-netdebug-ping.txt
  Commit: Y | docs+fix: error hardening and README

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [x] F1. Plan compliance audit
- [x] F2. Code quality review
- [x] F3. Real manual QA
- [x] F4. Scope fidelity

## Commit strategy
One conventional commit per todo (message on each todo's Commit line), committed only after that todo's acceptance criteria + QA evidence exist. Final verification wave findings land as `fix:` commits if any. No history rewriting; greenfield repo initialized in todo 1.

## Success criteria
- `cargo tauri dev` launches "verkkokyyla"; user can enter a DNS name / IPv4 / IPv6 target, pick family, Start/Stop a continuous ping.
- Table stays bounded at 500 rows while aggregates remain correct past 500 probes (proven by test).
- Graphs show the whole session history with extrema and loss preserved under downsampling (proven by test).
- Sessions persist to SQLite incrementally; past sessions list/reopen/delete works and reopened data matches the DB (proven by E2E query).
- Unprivileged run never dead-ends: on Windows the IPHlpAPI fallback (IcmpSendEcho for v4, Icmp6SendEcho2 for v6) is locale-independent and unprivileged; on POSIX the ping fallback forces `LC_ALL=C`; selection is automatic with a visible notice (proven by simulated-denial test).
- `cargo test`, `npx vitest run`, Playwright suite, and `cargo tauri build` all pass; final verification wave F1-F4 all APPROVE.
