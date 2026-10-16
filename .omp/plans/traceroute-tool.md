# traceroute-tool - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** A third tool in the app: Traceroute. Enter a target, watch the network path build hop-by-hop in real time — each router's IP, its reverse-DNS name (great for spotting which ISP/operator owns a problem hop), and per-probe response times. Traces are saved to local history and can be reopened or deleted like ping sessions.

**Why this approach:** The app shells out to the operating system's own traceroute (`tracert.exe` / `traceroute`) and parses its output structurally — zero new dependencies for the trace engine itself, works without admin rights, and survives localized Windows output because the parser only trusts numbers, never words. The one added dependency is a tiny reverse-DNS helper (`dns-lookup`), needed because Rust's standard tooling has no PTR lookup. Reusing the existing ping library was considered and ruled out with evidence: it cannot see the "time exceeded" replies that traceroute is built on.

**What it will NOT do:** No raw-socket/custom-packet traceroute, no map or GeoIP visualization, no probe-mode knobs (fixed sensible defaults: 30 hops, 1 s timeout), no concurrent traces, and it leaves your uncommitted HTTP-settings work untouched.

**Effort:** Large
**Risk:** Medium - output parsing across OSes/locales is the fragile point, mitigated by structural parsing, localized-fixture tests, and an accepted residual on exotic Windows locales.
**Decisions to sanity-check:** (1) OS-binary approach over a native Rust crate, (2) traces persisted like ping sessions, (3) reverse DNS resolved asynchronously so names trickle in without slowing the trace, (4) precondition: commit or stash your uncommitted HTTP-settings work before execution starts, since traceroute must additively edit some of the same files.

Your next move: start execution (`$start-work traceroute-tool`) or request a high-accuracy review first. Full execution detail follows below.

---

> TL;DR (machine): Large / Medium risk — 11 todos in 4 waves + 4 final verifiers: OS-binary traceroute (spawn+parse+stream), rDNS enrichment, SQLite migration 0003, new #/traceroute view, TDD + Playwright e2e.

## Scope
### Must have
- New `#/traceroute` route: `Route` union + `NAV_ITEMS` entry + `resolveRoute` branch in `src/routing.ts`; `App.tsx` renders `TracerouteView`; `src/routing.test.ts` updated.
- Trace engine (Rust): spawn `tracert.exe /d /h 30 /w 1000 <ip>` on Windows, `traceroute -n -m 30 -w 1 -q 1 <ip>` on POSIX (binary resolution: PATH lookup first, then `/usr/sbin/traceroute`, `/usr/bin/traceroute`, `/sbin/traceroute`, `/bin/traceroute`); target resolved first via existing `resolve_target` so the binary receives an IP literal (no family flags needed); `kill_on_drop` + per-line inactivity watchdog (5 s) + absolute ceiling (120 s); `with_program(program, prefix_args)` test seam — POSIX fixtures are shell scripts as in osping, Windows fixtures are `.cmd` files via `program=cmd.exe, prefix_args=["/C", script]` (sh fixtures don't run on Windows), and the hung-fixture is a direct `powershell.exe Start-Sleep` process so `kill_on_drop` is deterministic.
- Pure parser module (Rust): structural line parsing for both output formats — hop number, `*` lost probes, `<1 ms`, `N ms` / `N.NN ms` RTT tokens, trailing IP, POSIX `!H`/`!N`/`!P`/`!X` annotations; never matches localized prose.
- Live streaming: incremental stdout line reading; one event per completed hop line on a Tauri Channel; async reverse-DNS (PTR via `dns_lookup::lookup_addr` inside `tokio::task::spawn_blocking` — tokio has NO reverse lookup, Oracle finding), bounded 2 s timeout, per-trace cache, concurrency cap 4) streamed as follow-up hostname events; rDNS failure → `null` hostname; late hostnames persisted via `update_trace_hop_hostname`.
- TraceManager (Rust): single active trace enforced; start/stop; hops + hostnames persisted in one transaction at completion/cancel.
- Migration `0003_trace.sql`: `traces` + `trace_hops` tables with `ON DELETE CASCADE`; db functions + tests — including atomic `complete_trace_with_hops(...)` (finish + insert in ONE transaction) and `update_trace_hop_hostname(trace_id, hop, hostname)` for hostnames that resolve after completion (Oracle blocker fix).
- Tauri commands: `start_trace`, `stop_trace`, `list_traces`, `load_trace`, `delete_trace`; registered in `src-tauri/src/lib.rs`; `TraceManager` managed as Tauri state.
- Frontend: `src/lib/types.ts` mirror types, `src/lib/ipc.ts` wrappers (two-Channel pattern), `src/hooks/useTraceroute.ts`, `src/views/TracerouteView.tsx` + hop table + past-traces panel + CSS modules.
- E2E: `e2e/mock-ipc.ts` traceroute cases with deterministic scripted hops (incl. hostname events) + `e2e/traceroute.spec.ts`.
- README update (new tool, tests) + fix stale "exactly one tool" comment in `src/routing.ts`.

### Must NOT have (guardrails, anti-slop, scope boundaries)
- No surge-ping TTL-based tracing (verified non-viable: SurgeError has no TimeExceeded variant).
- No new crate dependencies for the trace engine (no trippy-core, tracert, pnet). ONE pinned exception (Oracle blocker fix): `dns-lookup = "2"` for reverse DNS — tokio/std offer no PTR lookup; `dns-lookup` is the minimal safe `getnameinfo` wrapper.
- No reverse DNS inside the OS binary invocation — keep `/d` and `-n`; enrichment is backend-side only.
- No UDP/TCP/ICMP probe-mode selection, no probes-per-hop / max-hops / timeout UI controls (fixed defaults: 30 hops, 1 s timeout, `-q 1` POSIX; tracert's fixed 3 probes/hop).
- No multiple concurrent traces; no continuous per-hop re-probing (single pass).
- No changes to the ping or download-speed tools' behavior.
- Dirty-worktree policy (Metis 1.1 + Oracle 5 — HARD precondition, not optional): the feature must additively edit files that currently carry uncommitted HTTP-settings changes — `src/routing.ts`, `src/routing.test.ts`, `src/lib/ipc.ts`, `src/lib/types.ts`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`, `e2e/mock-ipc.ts`. **Execution REQUIRES the user to commit or stash the HTTP-settings work before `$start-work`.** If the tree is still dirty at execution start, the worker stops and asks — it must not improvise partial staging. After the precondition is met, normal per-todo commits apply.
- No GeoIP, maps, or hop-location features.
- No rDNS retries, backoff, or a global DNS cache (Metis 3.1); no router/framework extraction in `App.tsx` (Metis 3.2).

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: **TDD** — failing tests first inside every implementation todo (impl + tests = ONE todo). Frameworks: `cargo test` (Rust unit + db), `npx vitest run` (frontend pure logic), `npx playwright test` (E2E vs mock IPC). Real-network trace verified via ignored integration test (`cargo test -- --ignored e2e_trace`) runnable by the agent.
- Evidence: .omo/evidence/task-<N>-traceroute-tool.<ext> (test output captures per todo)

## Execution strategy
### Parallel execution waves
> Target 5-8 todos per wave. Fewer than 3 (except the final) means you under-split.

- Wave 1 (foundations, parallel): 1 (Rust parsers), 2 (migration 0003 + db layer), 6 (routing + types + ipc wrappers). Pinned DTO/event shapes in this plan make frontend foundation independent of backend impl.
- Wave 2: 3 (OS trace engines + fake-binary tests), 4 (TraceManager + rDNS enrichment), 7 (useTraceroute + pure reducers).
- Wave 3: 5 (Tauri commands + registration), 8 (TracerouteView + components), 9 (mock-ipc trace cases).
- Wave 4: 10 (Playwright e2e spec), 11 (README + comment fixes + full gate run).

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 parsers | — | 3 | 2, 6 |
| 2 db layer | — | 4 | 1, 6 |
| 3 OS engines | 1 | 4 | 4 (manager uses scripted seam), 7 |
| 4 TraceManager | 1, 2, 3 | 5, 9 | 7 |
| 5 Tauri commands | 4 | 10 | 8, 9 |
| 6 routing/types/ipc | — | 7, 8, 9 | 1, 2 |
| 7 useTraceroute | 6 | 8 | 3, 4 |
| 8 TracerouteView | 6, 7 | 10 | 5, 9 |
| 9 mock-ipc | 4 (event shapes), 6 | 10 | 5, 8 |
| 10 e2e spec | 5, 8, 9 | 11 | — |
| 11 docs + gate | 10 | — | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->

Pinned wire shapes (normative for ALL todos — executor has no interview context):

```rust
// src-tauri/src/trace.rs — hop/status channels
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum TraceEvent {
    Hop { hop: u32, address: Option<String>, rtt1_ms: Option<f64>, rtt2_ms: Option<f64>, rtt3_ms: Option<f64>, annotation: Option<String>, at: String },
    Hostname { hop: u32, address: String, hostname: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum TraceStatusEvent {
    Completed { trace_id: i64, hop_count: u64, reached_target: bool },
    Cancelled { trace_id: i64, hop_count: u64 },
    Error { message: String },
}

// All command-return DTOs derive Clone+Debug+PartialEq+Serialize with #[serde(rename_all = "camelCase")]
// — mirroring session.rs:105-197 exactly; snake_case must never leak onto the wire.
#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct StartTraceDto { pub trace_id: i64, pub engine: String, pub resolved_ip: String, pub answers: Vec<String> }
#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct StoppedTraceDto { pub trace_id: i64, pub hop_count: u64, pub ended_at: String }
#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct TraceSummaryDto { pub id: i64, pub target_input: String, pub resolved_ip: String, pub family: String, pub engine: String, pub max_hops: i64, pub started_at: String, pub ended_at: Option<String>, pub status: String, pub reached_target: bool, pub hop_count: i64 }
#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct TraceHopDto { pub hop: i64, pub address: Option<String>, pub hostname: Option<String>, pub rtt1_ms: Option<f64>, pub rtt2_ms: Option<f64>, pub rtt3_ms: Option<f64>, pub annotation: Option<String>, pub at: String }
#[derive(Clone, Debug, PartialEq, Serialize)] #[serde(rename_all = "camelCase")]
pub struct LoadedTraceDto { pub trace: TraceSummaryDto, pub hops: Vec<TraceHopDto> }
```

```sql
-- src-tauri/migrations/0003_trace.sql
CREATE TABLE traces (
    id INTEGER PRIMARY KEY,
    target_input TEXT NOT NULL,
    resolved_ip TEXT NOT NULL,
    family TEXT NOT NULL,
    engine TEXT NOT NULL,
    max_hops INTEGER NOT NULL,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    status TEXT NOT NULL DEFAULT 'running' CHECK(status IN ('running','completed','cancelled','error')),
    reached_target INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE trace_hops (
    id INTEGER PRIMARY KEY,
    trace_id INTEGER NOT NULL REFERENCES traces(id) ON DELETE CASCADE,
    hop INTEGER NOT NULL,
    address TEXT,
    hostname TEXT,
    rtt1_ms REAL, rtt2_ms REAL, rtt3_ms REAL,
    annotation TEXT,
    at TEXT NOT NULL
);
CREATE INDEX idx_trace_hops_trace ON trace_hops(trace_id, hop);
```

Fixed defaults everywhere: MAX_HOPS = 30, PROBE_TIMEOUT_MS = 1000 (Windows `/w 1000`, POSIX `-w 1`), POSIX `-q 1`; per-line inactivity watchdog 5000 ms; absolute trace ceiling 120_000 ms; rDNS timeout 2 s; rDNS concurrency cap 4. `<1 ms` / `< 1 ms` maps to 0.5 ms.

- [ ] 1. Pure trace-output parser module with fixture tests
  What to do / Must NOT do: TDD — write failing tests FIRST, then create `src-tauri/src/engine/trace_parse.rs` with `RawHop { hop: u32, address: Option<String>, rtts: Vec<Option<f64>>, annotation: Option<String> }`, `parse_tracert_line(line: &str) -> Option<RawHop>` and `parse_traceroute_line(line: &str) -> Option<RawHop>`. Parser anchors ONLY on lines matching `^\s*(\d+)\s` (hop number); RTT tokens are `*` (→ None), `<1 ms` / `< 1 ms` (→ Some(0.5)), or numeric `N[.NN] ms`; the trailing IP/token after RTTs is the address (Windows `/d` and POSIX `-n` guarantee numeric). POSIX annotations `!H`/`!N`/`!P`/`!X` captured into `annotation`. Header/garbage/localized-prose lines return None. Must NOT match on any English/localized words ("Request timed out", "Reitin jäljitys", etc.); must NOT use regex crate if not already a dependency — hand-roll tokenization like `extract_time_token` does (check `Cargo.toml` first; regex IS acceptable only if already present). Export from `engine/mod.rs` as `pub mod trace_parse;`.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 3
  References (executor has NO interview context - be exhaustive): `src-tauri/src/engine/osping.rs:100-172` (interpret/extract_time_token hand-rolled tokenization to imitate); `src-tauri/src/engine/osping.rs:174-280` (fixture test style); `src-tauri/src/engine/mod.rs:9-39` (export pattern); librarian findings in `.omo/drafts/traceroute-tool.md` (tracert/traceroute output shapes).
  Acceptance criteria (agent-executable): `cargo test trace_parse` passes with fixtures covering: (a) Windows happy path `  1    <1 ms    <1 ms    <1 ms  192.168.1.1` → hop 1, rtts [0.5,0.5,0.5], addr 192.168.1.1; (b) all-timeout hop `  2     *        *        *     Request timed out.` → rtts [None,None,None], addr None; (c) mixed `  3    12 ms    *      11 ms  10.0.0.1` → [Some(12),None,Some(11)]; (d) `< 1 ms` spaced variant → 0.5; (e) synthetic localized header lines (Finnish + German) → None; (f) non-ASCII garbage bytes → None, no panic; (g) POSIX ` 1  192.168.1.1  0.537 ms` → hop 1, [Some(0.537)]; (h) POSIX ` 2  *` → [None], addr None; (i) POSIX annotation ` 4  10.0.0.1  5 ms !H` → annotation Some("!H"); (j) POSIX multi-address hop line keeps FIRST address.
  QA scenarios (name the exact tool + invocation): happy = `cargo test trace_parse` all fixtures green; failure = fixtures (e),(f) prove localized/garbage input yields None without panic. Evidence .omo/evidence/task-1-traceroute-tool.txt (cargo test output).
  Commit: Y | feat(engine): tracert/traceroute output parsers with fixture tests

- [ ] 2. Migration 0003 + trace persistence layer with db tests
  What to do / Must NOT do: TDD — failing db tests first, then add `src-tauri/migrations/0003_trace.sql` (exact schema pinned above) and extend `src-tauri/src/db.rs` with row types `NewTrace`, `TraceHopRow`, `TraceSummary` and functions: `create_trace(&NewTrace) -> Result<i64, DbError>`, `complete_trace_with_hops(id, ended_at, status, reached_target, hops: &[TraceHopRow])` — finish + hop insert in ONE transaction (Oracle blocker fix: no separate finish/insert at completion), `update_trace_hop_hostname(trace_id, hop, hostname)` for late rDNS results, `list_traces() -> Vec<TraceSummary>` (newest first, hop_count via subquery like list_sessions does), `load_trace_hops(trace_id) -> Vec<TraceHopRow>` (ordered by hop), `delete_trace(id)`. Must NOT alter migrations 0001/0002 or existing functions; must NOT use `query!` macros (repo uses runtime `sqlx::query` only).
  Parallelization: Wave 1 | Blocked by: — | Blocks: 4
  References: `src-tauri/src/db.rs:132-145` (connect + migrate!), `db.rs:148-166` (create_session), `db.rs:179-199` (insert_probes_batch transaction), `db.rs:202-236` (list_sessions SQL-derived counts), `db.rs:259-265` (delete_session cascade); `src-tauri/migrations/0001_init.sql`, `0002_add_payload_and_df.sql` (naming/style).
  Acceptance criteria: `cargo test db` passes including new tests: migration applies on fresh db and `Database::connect` twice on same file is a no-op (idempotency); `list_traces` returns hop_count + reached_target correctly; `delete_trace` cascades (`SELECT COUNT(*) FROM trace_hops WHERE trace_id=?` → 0); `load_trace_hops` ordered by hop; `complete_trace_with_hops` is atomic (a bad hop row rolls back BOTH the hops and the status update — status stays 'running'); `update_trace_hop_hostname` sets hostname on the right hop row only.
  QA scenarios: happy = full round-trip create→complete_with_hops→list→load→delete via `cargo test db`; failure = complete_trace_with_hops with one invalid row rolls back everything (status + count unchanged). Evidence .omo/evidence/task-2-traceroute-tool.txt.
  Commit: Y | feat(db): traces and trace_hops persistence (migration 0003)

- [ ] 3. OS trace engines (spawn + streaming parse) with trickling fake-binary tests
  What to do / Must NOT do: TDD — failing tests first. Create `src-tauri/src/engine/tracert_win.rs` (`#[cfg(windows)]`) and `src-tauri/src/engine/trace_posix.rs` (`#[cfg(unix)]`) plus a shared `src-tauri/src/engine/trace_os.rs` core: `TraceEngineError { Unavailable(String), Spawn(String) }`.
  `OsTracer::new(target: IpAddr)` with binary resolution — PATH lookup first, then absolute fallbacks: Windows `tracert.exe` (PATH/System32); POSIX `/usr/sbin/traceroute`, `/usr/bin/traceroute`, `/sbin/traceroute`, `/bin/traceroute` (Oracle 6: this PATH-then-absolute order is normative).
  Test seam: `with_program(program: PathBuf, prefix_args: Vec<String>, target: IpAddr)` — POSIX fixtures are executable `.sh` scripts with empty prefix (osping style); WINDOWS trickle fixtures are `.cmd` files run as `program=cmd.exe, prefix_args=["/C", <script path>]` (Oracle 4: sh fixtures cannot run on this Windows machine; `.cmd` trickles lines via `echo` + `ping 127.0.0.1 -n 2 >nul` ≈1 s delays — short-lived grandchildren are fine because trickle fakes are never used in kill tests);
  the WINDOWS HUNG fixture is `program=powershell.exe, prefix_args=["-NoProfile", "-Command", "Start-Sleep -Seconds 999"]` — blocks in the DIRECT process with no grandchildren, so `kill_on_drop` is deterministic (Oracle round-2: production spawns `tracert.exe` directly so kill_on_drop is exact there; tree-kill logic is explicitly OUT of scope).
  argv: Windows `[/d, /h, 30, /w, 1000, <ip>]`; POSIX `[-n, -m, 30, -w, 1, -q, 1, <ip>]` with `env("LC_ALL", "C")`.
  Streaming is NEW (osping uses `.output()` — do NOT copy that): spawn with `stdout(Stdio::piped())`, `kill_on_drop(true)`, read with `BufReader::lines()`, feed each line to `trace_parse`, yield `RawHop` events via an `mpsc` channel.
  Watchdogs: kill child when no new line for 5000 ms (inactivity) or after 120_000 ms absolute ceiling. Decode with `String::from_utf8_lossy`. Must NOT add crate dependencies; must NOT reuse ping `EngineFactory`/`ProbeResult`.
  Parallelization: Wave 2 | Blocked by: 1 | Blocks: 4
  References: `src-tauri/src/engine/osping.rs:32-97` (discovery/spawn+kill_on_drop/LC_ALL/lossy patterns — but replace `.output()` with piped streaming); osping.rs:183-191 (POSIX fixture style — Windows needs the cmd.exe/.cmd variant above); todo 1 parser API; `.omo/drafts/traceroute-tool.md` Metis findings 1.2/1.3/2.2/2.3.
  Acceptance criteria: `cargo test trace_os` passes ON THIS WINDOWS MACHINE (and cfg-gated POSIX tests compile): (a) fake program trickling hop lines with delays yields hop events BEFORE process exit (assert first event arrives while child still running); (b) hung fake program (no output) is killed by the 5 s inactivity watchdog and surfaces a typed error; (c) resolution with no traceroute anywhere → `Unavailable` (injectable search-path list pointing at an empty temp dir); (d) argv builder unit tests per platform cfg; (e) absolute ceiling test uses a shortened test constant (inject ceiling) so the test takes seconds, not 120 s.
  QA scenarios: happy = trickling-fixture test green via `cargo test trace_os`; failure = hung-binary watchdog kill + missing-binary Unavailable (same invocation). Evidence .omo/evidence/task-3-traceroute-tool.txt.
  Commit: Y | feat(engine): OS traceroute engines with streaming parser

- [ ] 4. TraceManager: lifecycle, rDNS enrichment, DTOs, commands-facing API
  What to do / Must NOT do: TDD — failing tests first with scripted seams. FIRST add `dns-lookup = "2"` to `src-tauri/Cargo.toml` (additive line only — file is dirty; Oracle blocker 1: tokio/std have no reverse-DNS API, `dns_lookup::lookup_addr(&IpAddr)` is the pinned mechanism, always wrapped in `tokio::task::spawn_blocking`).
  Create `src-tauri/src/trace.rs`: `TraceManager::new(db: Database, factory: TraceFactory, resolver: Arc<dyn Fn(IpAddr) -> BoxFuture<Option<String>> + Send + Sync>)`; `TraceFactory = Arc<dyn Fn(IpAddr) -> Future<Output=Result<OsTracer stream, TraceEngineError>> + Send + Sync>` with a scripted mock trace seam for tests (trace-shaped analog of session.rs:729-760).
  API: `start(target, family, on_event: impl Fn(TraceEvent), on_status: StatusSink) -> Result<StartTraceDto, TraceError>`; `stop() -> Result<StoppedTraceDto, TraceError>` (NO id — single active trace; `TraceError::NoActiveTrace` when idle; `TraceError::AlreadyRunning` on second start).
  Flow: resolve via existing `resolve_target`, construct the engine (on `Unavailable`/spawn failure: emit typed Error status, persist NO trace row — history never contains traces that never started), insert traces row, spawn engine stream, emit Hop events as they arrive;
  for each unique hop address run PTR enrichment via the injected resolver with 2 s timeout, per-trace `HashMap<IpAddr, Option<String>>` cache, concurrency cap 4 via a `tokio::sync::Semaphore` whose permit is MOVED INTO the spawned enrichment task and held until the blocking lookup's `JoinHandle` completes — the 2 s `tokio::time::timeout` stops event emission only, it does NOT release the permit early (Oracle round-2 fix: a timed-out lookup still occupies its slot until the OS resolver returns, so concurrent blocking calls can never exceed 4),
  then emit `TraceEvent::Hostname` — MAY arrive after Completed; on stream end finalize via `db.complete_trace_with_hops(...)` (ONE transaction) with whatever hostnames have resolved so far, then emit Completed/Cancelled/Error;
  each late Hostname event ALSO calls `db.update_trace_hop_hostname(trace_id, hop, hostname)` so reopened traces show names (Oracle blocker 2).
  `list_traces`, `load_trace(id)`, `delete_trace(id)` delegate to db. All-* traces are valid completions, NOT errors.
  Must NOT retry rDNS; no global DNS cache; rDNS lookups must not block hop events; no tokio::net::lookup_addr (does not exist).
  Parallelization: Wave 2 | Blocked by: 1, 2, 3 | Blocks: 5, 9
  References: `src-tauri/src/session.rs:375-760` (manager structure, StatusSink, test seams — adapt, don't copy); `src-tauri/src/engine/dns.rs:37` (`resolve_target` reuse); todo 2 db functions; serde patterns `session.rs:49-74,105-197`; dns-lookup crate docs (docs.rs/dns-lookup/2).
  Acceptance criteria: `cargo test trace::` passes: (a) scripted trace of 3 hops streams 3 Hop events then Completed, rows persisted atomically with status 'completed'; (b) second `start` while active → `AlreadyRunning`; (c) `stop` mid-trace persists received hops with status 'cancelled' and emits Cancelled; (d) `stop` when idle → `NoActiveTrace`; (e) scripted resolver returning Some/None produces Hostname events with hostname/null, hop rows persist resolved names; (f) resolver slowness does not delay Hop events (assert Hop emitted before resolver future completes); (g) engine Unavailable → typed Error status event and NO trace row persisted (`SELECT COUNT(*) FROM traces` unchanged); (h) LATE hostname (resolver completes after trace end) still persists via update_trace_hop_hostname — `load_trace` shows the name; (i) resolver concurrency bound: scripted resolver with 6 lookups that each block 3 s — assert peak concurrent resolver invocations ≤ 4 even after the 2 s timeouts fire (permit held until JoinHandle completes).
  QA scenarios: happy = (a)+(e) via `cargo test trace::`; failure = (b)(c)(d)(g)(h). Evidence .omo/evidence/task-4-traceroute-tool.txt.
  Commit: Y | feat(trace): trace manager with rDNS enrichment and persistence

- [ ] 5. Tauri commands + state registration
  What to do / Must NOT do: Add to `src-tauri/src/lib.rs`: `pub mod trace;`; commands `start_trace(manager: State<TraceManager>, target: String, family: String, on_event: Channel<TraceEvent>, on_status: Channel<TraceStatusEvent>) -> Result<StartTraceDto, TraceError>`, `stop_trace(manager) -> Result<StoppedTraceDto, TraceError>`, `list_traces(manager) -> Result<Vec<TraceSummaryDto>, TraceError>`, `load_trace(manager, id) -> Result<LoadedTraceDto, TraceError>`, `delete_trace(manager, id) -> Result<(), TraceError>`; register all five in `generate_handler!`; construct `TraceManager` in `setup` next to `SessionManager` with production `TraceFactory` (OsTracer) and production resolver wrapping `dns_lookup::lookup_addr(&ip)` inside `tokio::task::spawn_blocking` (map Ok→Some(name), Err→None). `TraceError` must implement `Serialize` for the command boundary like `SessionError` does. Must NOT modify the existing 9 commands or download parts of this dirty file — additive only (Metis 1.1).
  Parallelization: Wave 3 | Blocked by: 4 | Blocks: 10
  References: `src-tauri/src/lib.rs:18-110` (command + Channel adaptation pattern), `lib.rs:112-136` (setup + generate_handler); `session.rs` `SessionError` serde impl.
  Acceptance criteria: `cargo check` and `cargo clippy` clean; `cargo test` still fully green; `git diff --numstat src-tauri/src/lib.rs` shows 0 deletions (additive-only proof).
  QA scenarios: happy = `cargo clippy` + `cargo test` green; failure = PowerShell-native additivity assertion: `if ((git diff --numstat -- src-tauri/src/lib.rs | ForEach-Object { [int](($_ -split "\s+")[1]) } | Measure-Object -Sum).Sum -ne 0) { throw "deletions in shared dirty file" }` — any deleted line in this dirty shared file fails the todo. Command-registration coverage is exercised end-to-end by todo 10 (mock invokes the same command names). Evidence .omo/evidence/task-5-traceroute-tool.txt.
  Commit: Y | feat(backend): trace Tauri commands and state

- [ ] 6. Frontend foundation: route, types, IPC wrappers (TDD red→green on routing)
  What to do / Must NOT do: TDD — first update `src/routing.test.ts` to assert THREE nav items and `#/traceroute` resolution (red), then edit `src/routing.ts`: `Route` gains `"traceroute"`, `NAV_ITEMS` gains `{ route: "traceroute", label: "Traceroute", hash: "#/traceroute" }` (position: after Ping, before download-speed), `resolveRoute` gains an explicit branch, stale "exactly one tool" comment corrected. Add to `src/lib/types.ts` (additive, dirty file — preserve existing lines): `TraceEvent`, `TraceStatusEvent`, `StartTraceDto`, `TraceSummaryDto`, `TraceHopDto`, `LoadedTraceDto`, plus UI `TraceHopRow` mirroring the pinned Rust shapes. Add to `src/lib/ipc.ts` (additive): `startTrace(target, family, onEvent, onStatus)`, `stopTrace()`, `listTraces()`, `loadTrace(id)`, `deleteTrace(id)` cloning the two-Channel `startSession` shape. Must NOT alter download/ping exports.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 7, 8, 9
  References: `src/routing.ts` (whole file, 19 lines); `src/routing.test.ts:5-31`; `src/lib/ipc.ts:18-36` (startSession Channel pattern); `src/lib/types.ts:24-109` (ping DTO mirrors to imitate); pinned wire shapes above.
  Acceptance criteria: `npx vitest run` green with updated routing.test.ts (3 routes, traceroute branch, unknown-hash fallback to ping); `npx tsc --noEmit` clean.
  QA scenarios: happy = vitest+tsc green; failure = revert the resolveRoute branch → routing.test.ts fails (proves test sensitivity; run once, then restore). Evidence .omo/evidence/task-6-traceroute-tool.txt.
  Commit: Y | feat(ui): traceroute route, types, and IPC wrappers

- [ ] 7. useTraceroute hook + pure hop reducers (node-env vitest)
  What to do / Must NOT do: TDD — failing reducer tests first. Create `src/lib/traceHops.ts` with pure functions `applyHopEvent(rows: readonly TraceHopRow[], e: TraceEvent & {event:"hop"}) -> TraceHopRow[]` (upsert by hop, sorted) and `applyHostnameEvent(rows, e) -> TraceHopRow[]` (set hostname on matching hop; MUST work after trace completion and ignore unknown hops) — pure because vitest runs in node env (no jsdom). Then `src/hooks/useTraceroute.ts` cloning `usePingSession` lifecycle at reduced scope: state target/family/isRunning/error/status/hops/pastTraces/viewMode/pastTrace; `start()` validates non-empty target (reuse pattern from PingView validation), wires Channels, batches via requestAnimationFrame like usePingSession:92-115; `stop()` calls `stopTrace()`; `openTrace(id)`/`deleteTrace(id)` mirror openSession/deleteSession incl. `window.confirm`. UNLIKE usePingSession: add an unmount effect that calls `stopTrace()` when running (Metis 2.1 — navigating away must not leak a ~90 s child process).
  Parallelization: Wave 2 | Blocked by: 6 | Blocks: 8
  References: `src/hooks/usePingSession.ts:36-271` (whole lifecycle to adapt); `src/lib/tableBuffer.test.ts` + `src/lib/validate.test.ts` (vitest style); `src/hooks/__tests__/useHttpSettings.test.ts` (hook-adjacent test conventions); Metis 2.1/4.3 in draft.
  Acceptance criteria: `npx vitest run` green with new `src/lib/traceHops.test.ts`: (a) hop upsert keeps sort order; (b) hostname event updates existing row; (c) hostname event for unknown hop is a no-op; (d) hostname event applied after a "completed" marker still updates rows; (e) all-`*` hop row renders address null. `npx tsc --noEmit` clean.
  QA scenarios: happy = reducer suite green; failure = (c)/(d) prove out-of-order/late events can't corrupt state. Evidence .omo/evidence/task-7-traceroute-tool.txt.
  Commit: Y | feat(ui): useTraceroute hook and hop reducers

- [ ] 8. TracerouteView + hop table + history panel
  What to do / Must NOT do: Create `src/views/TracerouteView.tsx` + `TracerouteView.module.css`, `src/components/TracerouteTable.tsx` + `.module.css`, `src/components/TraceSessionPanel.tsx` + `.module.css`. View: target text input + family select (auto/v4/v6, reuse PingView control markup), Start/Stop buttons, error/status banner, running progress line `Hop n/30` (updates as Hop events arrive — silent spinner is a FAIL per Metis 5.2), live `TracerouteTable` (columns: Hop, Address, Hostname (dash while null), RTT1..3 ms, Note(annotation)), `TraceSessionPanel` listing past traces (target, resolvedIp, startedAt, hop count, reached-target yes/no, status; Reopen/Delete buttons — mirror PingSessionPanel structure). Edit `src/App.tsx` minimally: add `route === "traceroute"` branch rendering TracerouteView (Metis 3.2: NO router extraction, other branches untouched). data-testids: `trace-target`, `trace-start`, `trace-stop`, `trace-table`, `trace-row`, `trace-status`, `trace-error`, `trace-progress`, `trace-session-panel`, `trace-open`, `trace-delete`.
  Parallelization: Wave 3 | Blocked by: 6, 7 | Blocks: 10
  References: `src/views/PingView.tsx:45-249` (controls/banner/panel composition), `src/views/DownloadSpeedView.tsx` (single-view structure), `src/components/PingSessionPanel.tsx` (panel to mirror), `src/App.tsx:22-39`; CSS module convention `import styles from "./X.module.css"`.
  Acceptance criteria: `npx tsc --noEmit` clean; `npm run build` (tsc && vite build) exits 0; `git diff --numstat src/App.tsx` shows 0 deletions (branch added, nothing removed).
  QA scenarios: happy = `npm run build` green; failure = PowerShell-native assertions: `if ((git diff --numstat -- src/App.tsx | ForEach-Object { [int](($_ -split "\s+")[1]) } | Measure-Object -Sum).Sum -ne 0) { throw "App.tsx deletions" }` and `if ((Select-String -Path src/App.tsx -Pattern 'route === "traceroute"').Count -lt 1) { throw "traceroute branch missing" }`. Rendering verified by todo 10 e2e. Evidence .omo/evidence/task-8-traceroute-tool.txt.
  Commit: Y | feat(ui): traceroute view, hop table, and history panel

- [ ] 9. E2E mock: traceroute command cases
  What to do / Must NOT do: Extend `e2e/mock-ipc.ts` (additive — dirty file, preserve all existing cases) with mock state `activeTrace`/`endedTraces` and cases: `start_trace` (returns StartTraceDto `{traceId, engine:"tracert-mock", resolvedIp, answers}`; then asynchronously emits scripted Hop events over `args.onEvent` — for target `example.com`: hop 1 `192.0.2.1` [0.5,0.5,0.5], hop 2 `198.51.100.1` [12,null,11], hop 3 = resolvedIp → reached; followed by `TraceEvent Hostname` for hops 1-2 where the LAST hostname arrives AFTER the Completed status event — Metis 4.3 ordering case; for `error.test`: throw `{kind:"unavailable", message:"traceroute binary not found"}`), `stop_trace` (persists partial hops, emits Cancelled), `list_traces`, `load_trace`, `delete_trace`. Support `window.__TAURI_MOCK_ENDED_TRACES__` preload like ended sessions. Must NOT alter existing ping/download cases.
  Parallelization: Wave 3 | Blocked by: 4 (event shapes), 6 | Blocks: 10
  References: `e2e/mock-ipc.ts:39-86` (install + sendChannel), `:162-290` (session case patterns), `:292-438` (download case patterns); pinned wire shapes above.
  Acceptance criteria: `npx playwright test e2e/ping.spec.ts e2e/download-speed.spec.ts` still green (no regressions in shared mock); new cases exercised green by todo 10.
  QA scenarios: happy = existing suites green + trace case flows verified in todo 10; failure = `error.test` target surfaces typed error to the view (todo 10 asserts). Evidence .omo/evidence/task-9-traceroute-tool.txt.
  Commit: Y | test(e2e): mock IPC traceroute cases

- [ ] 10. Playwright E2E spec for traceroute
  What to do / Must NOT do: Create `e2e/traceroute.spec.ts` in the style of `e2e/download-speed.spec.ts`: `installMockTauri(page)` then navigate `/#/traceroute`. Tests: (a) nav sidebar shows Traceroute and route renders; (b) empty target disables Start; (c) happy path: start `example.com` → rows appear INCREMENTALLY (assert hop-1 row visible before completion), progress shows `Hop 1/30`, hostnames fill in, late hostname after Completed still updates the row; (d) stop mid-trace → Cancelled status, partial rows kept; (e) `error.test` → error banner shows unavailable message; (f) unmount safety: start trace, navigate to `#/ping`, assert `stop_trace` was invoked (expose mock call log, e.g. `window.__TAURI_MOCK_TRACE_STOPPED__`); (g) history: completed trace appears in panel, Reopen renders stored hops with hostnames, Delete removes it. Must NOT modify other specs.
  Parallelization: Wave 4 | Blocked by: 5, 8, 9 | Blocks: 11
  References: `e2e/download-speed.spec.ts:1-149` (structure/assertion style), `e2e/ping.spec.ts`; mock API from todo 9; testids from todo 8.
  Acceptance criteria: `npx playwright test` — entire suite green including the 7 new scenarios.
  QA scenarios: happy = `npx playwright test` full suite green; failure = `npx playwright test e2e/traceroute.spec.ts` MUST fail if run with the mock's `start_trace` case removed (verify once by commenting the case, running, observing red, restoring — capture both runs in evidence). Evidence .omo/evidence/task-10-traceroute-tool.txt.
  Commit: Y | test(e2e): traceroute Playwright spec

- [ ] 11. Real-network ignored test, README, and full gate
  What to do / Must NOT do: Add `src-tauri/tests/e2e_trace.rs` with `#[ignore]`d `e2e_trace_loopback` (trace `127.0.0.1`: completes, ≤2 hops, persisted rows consistent) modeled on `src-tauri/tests/e2e_ping.rs:122-174`; on typed `Unavailable` (OS lacks traceroute) the test LOGS the skip and returns `Ok(())` so the command exits 0 on such machines (Oracle 7 — no conflict with the exit-0 gate). Run it once via `cargo test -- --ignored e2e_trace`. Update `README.md`: new tool paragraph (Traceroute: OS tracert/traceroute under the hood, live hops + reverse DNS, history), test commands unchanged. Final gate run: `cargo test`, `cargo clippy`, `npx vitest run`, `npm run build`, `npx playwright test` — all green; capture combined output.
  Parallelization: Wave 4 | Blocked by: 10 | Blocks: —
  References: `src-tauri/tests/e2e_ping.rs:122-174`; `README.md` (whole file).
  Acceptance criteria: all five commands exit 0; e2e_trace executed with captured output (or captured Unavailable residual).
  QA scenarios: happy = full gate green; failure = any red command blocks completion and is fixed at root cause, never by deleting tests. Evidence .omo/evidence/task-11-traceroute-tool.txt.
  Commit: Y | test(e2e): real-network trace test and README traceroute docs

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit — agent reads this plan, then runs `git log --oneline -15` and `git status --short`; asserts: exactly one commit per todo 1-11 with the pinned messages, no stray modified files, and every todo's acceptance command re-run green (`cargo test`, `npx vitest run`). Output a per-todo verdict table. FAIL if any commit is missing or any command red.
- [ ] F2. Code quality review — run `cargo clippy --all-targets -- -D warnings` and `npx tsc --noEmit`; then read every file created by todos 1-11 and reject on: `as any`/`@ts-ignore`, empty catch blocks, panics/unwraps in non-test Rust paths, functions over ~60 lines without justification, deviation from pinned wire shapes. FAIL on any hit.
- [ ] F3. Real QA execution — run `cargo test -- --ignored e2e_trace` (real loopback trace on this machine; Unavailable-skip returns 0 by design) and `npx playwright test` full suite; assert the trace test reports ≥1 persisted hop OR the documented Unavailable skip. Record raw outputs as evidence.
- [ ] F4. Scope fidelity — record the pre-plan revision FIRST: `git rev-parse HEAD` into `.omo/evidence/f4-preplan-head.txt` before todo 1 starts; at verification time diff the Must-have list against delivered code via `git diff --stat <prePlanHead>`; assert: `git diff <prePlanHead> -- src-tauri/Cargo.toml` adds ONLY the `dns-lookup` line; ping/download-speed files show no behavioral edits (numstat deletions = 0 via the PowerShell assertion pattern from todo 5); spawned argv contains no reverse-DNS flags — `if (Select-String -Path src-tauri/src/engine/trace_os.rs,src-tauri/src/engine/tracert_win.rs,src-tauri/src/engine/trace_posix.rs -Pattern 'gethostby') { throw }` and the pinned argv lists match todo 3 verbatim; nothing from Must-NOT-have present. FAIL on any unrequested addition.

## Commit strategy

One commit per todo, conventional-commit style matching repo history (`feat(engine):`, `feat(db):`, `feat(ui):`, `test(e2e):`, `docs:`). Commit only after the todo's acceptance command passes. Never amend; if a hook rejects, fix and re-commit. HARD precondition (Oracle 5): the HTTP-settings work must be committed or stashed by the user before todo 1 starts; from then on the tree only contains traceroute changes, so per-todo commits stay clean.

## Success criteria

- `cargo test` passes (all new parser/db/manager/engine tests + existing suite).
- `npx vitest run` passes (routing + trace reducer tests + existing).
- `npx playwright test` passes (new traceroute spec + existing specs unharmed).
- `cargo test -- --ignored e2e_trace` executes a real loopback trace (agent-run; skip-with-log allowed only where the OS lacks the binary — evidence captured either way).
- `cargo clippy` clean on touched crates; `tsc && vite build` exits 0.
- Nav shows three tools; `#/traceroute` runs a live trace against the mock in E2E, showing hops then hostnames; past traces reopen and delete.
