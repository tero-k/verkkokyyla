# dns-tester - Work Plan

## TL;DR (For humans)
<!-- Fill this LAST, after the detailed plan below is written, so it summarizes the REAL plan. -->
<!-- Plain English for a non-engineer: NO file paths, NO todo numbers, NO wave/agent/tool names. -->

**What you'll get:** A new "DNS Tester" tab in the app with three modes: Lookup (query any DNS record type against your system resolver or public ones like Cloudflare/Google/Quad9, with live results), Benchmark (race resolvers against each other with realistic browsing, cache-behavior, and load-test profiles — with honest percentiles, timeout rates, and a reliability score), and Diagnostics (DNSSEC validation, delegation/glue checks, nameserver reachability, filtering and hijack detection, and a guarded zone-transfer exposure check). Benchmark and diagnostics runs are saved to local history like your ping sessions.

**Why this approach:** The current app can only resolve names through the OS, which makes protocol-level testing impossible — so the backend gains a real DNS protocol stack (Hickory, the mature Rust DNS library) behind the app's existing command/streaming/persistence patterns. Benchmark numbers follow the methodology of professional tools (namebench, flamethrower): percentiles from raw samples, cache-isolated measurements, and capped, consent-gated load tests so the tool can't be abused against public resolvers.

**What it will NOT do:** No global propagation checks, blacklist reputation, or geo-distributed measurements (those need external infrastructure). No changes to your existing tools or to your OS DNS settings. No scheduled monitoring — everything runs when you click a button.

**Effort:** XL
**Risk:** Medium - new crypto/TLS dependency stack (ring/quinn) on Windows, and Hickory's DNSSEC validation is experimental (mitigated: build spike first, "experimental" labeling, best-effort DoQ/DoH3).
**Decisions to sanity-check:** Rust MSRV rises to 1.88 (required by Hickory 0.26); load tests are blocked against public resolver presets and capped at 100 QPS/60 s; DNSSEC results carry an "experimental" caveat.

Your next move: approve and start work (`$start-work dns-tester`), or ask for the dual high-accuracy review first. Full execution detail follows below.

---

> TL;DR (machine): XL effort, Medium risk; 28 todos in 5 waves + 4 final verifiers; delivers tabbed DNS tester (lookup/benchmark/diagnostics) on hickory 0.26 (proto + net + resolver) with SQLite history and full test coverage.

## Scope
### Must have
- New "DNS Tester" sidebar entry (single view) with three tabs: Lookup, Benchmark, Diagnostics.
- Lookup: record queries A, AAAA, MX, TXT, NS, SOA, CNAME, SRV, CAA, PTR + arbitrary numeric type; reverse DNS with forward-confirmed check; CNAME chain walk with loop detection; wildcard detection; NXDOMAIN-vs-NODATA classification; against system resolver, preset public resolvers (Cloudflare/Google/Quad9), or a user-entered resolver.
- Transport tests: UDP, TCP, forced TCP, truncation→TCP fallback detection, EDNS0 support + buffer-size sweep (512/1232/1400/4096), DoT, DoH; DoQ and DoH3 are BEST-EFFORT (hickory quinn-based support is experimental) — reported as per-transport availability, never a pass/fail gate; same-query transport equivalence comparison.
- Diagnostics: DNSSEC validation via hickory `dnssec-ring` with EXPLICIT `ResolverOpts::validate(true)` and hickory's built-in root trust anchors — never trust a forwarded AD bit (hickory validation is documented experimental: bogus-domain SERVFAIL behavior is verified by tests, and the UI labels DNSSEC results "experimental"); AD-flag observation as a separate signal; bogus-domain SERVFAIL check against dnssec-failed.org; parent/child NS delegation consistency; glue completeness + correctness; per-nameserver IPv4/IPv6 UDP/TCP reachability (IPv6 checks report a `notAvailable` state when the machine has no IPv6 route/interface — never a failure); SOA serial consistency; open-resolver detection; answer equivalence across selected resolvers; filtering/hijack detection (NXDOMAIN rewriting, unexpected answers) using a DIRECT non-recursive query to the zone's authoritative NS as the reference ground truth (todo 4 provides this primitive); opt-in AXFR exposure check where the opt-in token must equal the exact zone name (re-prompted per target).
- Benchmark with three profiles: Everyday browsing (1000 popularity-weighted embedded domains, concurrency 1–4), Cache behavior (warm / unique-label cold "uncached approximation" / 50-50 mixed), Load (concurrency ladder 4/16/64, hard caps: ≤100 offered QPS, ≤60 s, user-entered endpoints ONLY — public-resolver presets are excluded from the Load profile — and an explicit per-run opt-in confirmation; the engine refuses to start above the caps). Metrics: min/median/mean/max + rates always computed when count>0; p90/p95/p99 are optional and computed ONLY when the cell has ≥1000 completed samples (otherwise the UI shows "insufficient samples") — always from raw per-query samples, never averaged averages; timeout rate, SERVFAIL rate, NXDOMAIN rate, completed QPS, offered-vs-completed QPS (load profile), cold-vs-reused connection latency (encrypted transports), reliability-weighted score (formula fixed in todo 13, visible in the UI).
- Live streamed progress and per-item results over Tauri Channels; benchmark progress is coalesced to aggregate snapshots at ≤500 ms cadence (NEVER per-sample streaming at up to 100 QPS); a Channel send failure switches the run to detached mode (stop sending, keep running, still persist). Cancellation for every long-running operation (cancel mid-benchmark stops in-flight queries and persists the run row with status 'cancelled').
- Persistence: migration `0006_dns.sql` (exactly two tables: `dns_runs` + `dns_run_targets`); benchmark and diagnostics runs saved with list/load/delete; one-off lookups are NOT persisted.
- Charts (uPlot, following PingGraphs pattern): latency histogram, CDF, p50/p95/p99 over time, offered-vs-completed QPS.
- Benchmark isolation: hickory's internal response cache DISABLED (`ResolverOpts::default()` then field assignment `opts.cache_size = 0` — `ResolverOpts` is `#[non_exhaustive]`, struct literals do not compile) plus a fresh resolver per measurement cell; system-resolver preset bypasses the OS cache (queries go direct) — UI labels this explicitly so numbers are not compared with OS-cached paths.
- Tests per repo convention: Rust unit tests co-located, Rust integration tests against a local hickory-server mock (fixture behaviors front-loaded in todo 3: truncation, SERVFAIL injection, slow-server timeout, NXDOMAIN, NODATA, oversized EDNS response, CNAME loop, broken PTR, delegation with glue, AXFR-enabled zone, divergent-answers second instance, per-instance query counter), vitest for hooks/lib, Playwright e2e via extended `e2e/mock-ipc.ts`; real-network tests marked `#[ignore]`.
- MSRV: `rust-version` in `src-tauri/Cargo.toml` is raised to "1.88" (hickory 0.26.1 requires it; verified via crates.io API 2026-08-22).

### Must NOT have (guardrails, anti-slop, scope boundaries)
- No distributed/external checks: global propagation, geo/CDN vantage points, DNSBL/reputation, NAT64 reachability, source-port/transaction-ID entropy measurement, QNAME-minimization observation.
- No changes to existing `src-tauri/src/engine/dns.rs` or `src-tauri/src/scan/rdns.rs` behavior; no changes to Ping/Traceroute/Speed-test/Scanner features.
- No AXFR without a per-run, per-target opt-in token that must equal the exact zone name; no NOTIFY/DNS-UPDATE probes; no cache-poisoning or forged-packet tests.
- No continuous/scheduled monitoring; all runs are user-initiated. Hard caps: concurrency ≤64, offered QPS ≤100, run duration ≤60 s (load profile), per-query timeout default 2 s; per-(resolver × profile) sample memory capped at 100 000 samples (<64 MB).
- No DNSCrypt, no hosting a DNS server in production code (hickory-server is a dev-dependency test fixture only). No `hickory-client` dependency (no stable 0.26 release — only 0.26.0-alpha.1; low-level exchanges use `hickory-net` 0.26.1 streams directly).
- No changes to OS resolver configuration; no new global state library on the frontend.
- Do not persist raw per-query benchmark samples to SQLite (in-memory only during a run); persist run metadata + per-target aggregates only.
- Do not use `hickory_resolver::Resolver::lookup` for data-collection queries — it hides rcode/flags/authority/additional sections; use the todo-4 low-level message exchange.

## Verification strategy
> Zero human intervention - all verification is agent-executed.
- Test decision: tests-after, matching repo convention — Rust: `cargo test` (unit co-located + `src-tauri/tests/` integration against a local hickory-server fixture; real-network tests `#[ignore]`), frontend: `npx vitest run`, e2e: `npx playwright test` with the in-browser Tauri mock. Build gates: `cargo clippy --all-targets -- -D warnings` and `npm run build` (tsc + vite) must exit 0.
- Evidence: .omo/evidence/dns-tester/task-<N>-<slug>.<ext> — each QA scenario writes its command output or screenshot there.

## Execution strategy
### Parallel execution waves
> Target 5-8 todos per wave. Fewer than 3 (except the final) means you under-split.

- Wave 1 (backend foundations, 7 todos): 1 crate setup + Windows build spike, 2 client layer, 3 mock-server fixture (ALL behaviors front-loaded incl. drop mode), 4 low-level query API + compile spike, 5 DB migration, 6 transport probes, 13 stats.
- Wave 2 (feature engines, 7 todos): 7 lookup orchestration, 8 name analysis, 9 DNSSEC, 10 delegation, 11 integrity, 12 benchmark scheduler, 14 query mixes + cache isolation.
- Wave 3 (profiles + commands + persistence, 6 todos): 15/16/17 profiles in parallel, then STRICTLY SEQUENCED 18 → 19 → 20 (each extends the shared manager.rs).
- Wave 4 (frontend, 6 todos): 21 types/IPC/routing, 22 Lookup tab, 23 Benchmark tab, 24 charts, 25 Diagnostics tab, 26 history UI.
- Wave 5 (e2e + final gates, 2 todos): 27 vitest + Playwright e2e, 28 full-suite gates.

### Dependency matrix
| Todo | Depends on | Blocks | Can parallelize with |
| --- | --- | --- | --- |
| 1 deps+module skeleton | — | 2,3,4,6 | 5,13 |
| 2 client/config layer | 1 | 4,6,7,8,9,10,11,12 | 3,5,13 |
| 3 mock DNS fixture (all behaviors) | 1 | 4,6,7,8,9,10,11,12,14 | 2,5,13 |
| 4 low-level query API + DTOs | 2,3 | 6,7,8,9,10,11,12,14 | 5,13 |
| 5 migration 0006 + db methods | — | 18,20 | 1,2,3,4,13 |
| 6 transport probes | 4 | 19 | 7,8,9,10,11,12,13,14 |
| 7 lookup orchestration cmd | 4 | 19,21 | 8,9,10,11,12,13,14 |
| 8 name analysis (rdns/cname/wildcard/nxdomain) | 4 | 19 | 7,9,10,11,12,13,14 |
| 9 DNSSEC diagnostics | 4 | 19 | 7,8,10,11,12,13,14 |
| 10 delegation diagnostics | 4 | 19 | 7,8,9,11,12,13,14 |
| 11 resolver integrity + AXFR | 4 | 19 | 7,8,9,10,12,13,14 |
| 12 benchmark scheduler core | 4 | 14,15,16,17 | 7,8,9,10,11,13 |
| 13 stats (percentiles/score) | — | 15,16,17 | all Wave 1-2 |
| 14 query mixes + cache isolation | 3,12 | 15,16 | 7,8,9,10,11,13 |
| 15 everyday + cache profiles | 12,13,14 | 18 | 16,17 |
| 16 load profile + pacing + guards | 12,13,14 | 18 | 15,17 |
| 17 cold-vs-reused measurement | 12,13 | 18 | 15,16 |
| 18 benchmark command + manager core | 5,15,16,17 | 20,21 | — |
| 19 diagnostics command | 5,6,7,8,9,10,11,18 | 20,21 | — |
| 20 history commands + DTOs + docs | 5,18,19 | 21 | — |
| 21 frontend types/ipc/routing | 18,19,20 | 22,23,24,25,26 | — |
| 22 Lookup tab | 21 | 27 | 23,24,25,26 |
| 23 Benchmark tab | 21 | 27 | 22,24,25,26 |
| 24 benchmark charts | 21 | 27 | 22,23,25,26 |
| 25 Diagnostics tab | 21 | 27 | 22,23,24,26 |
| 26 run history UI | 21 | 27 | 22,23,24,25 |
| 27 vitest + playwright e2e | 22,23,24,25,26 | 28 | — |
| 28 final gates (clippy/build/full suites) | 27 | — | — |

## Todos
> Implementation + Test = ONE todo. Never separate.
<!-- APPEND TASK BATCHES BELOW THIS LINE WITH edit/apply_patch - never rewrite the headers above. -->
- [x] 1. Add hickory dependencies and dns module skeleton
  What to do / Must NOT do: In `src-tauri/Cargo.toml` bump `rust-version` from "1.77.2" to "1.88" (hickory 0.26.1 declares rust_version 1.88 — verified via crates.io API 2026-08-22); add exactly these: `hickory-proto = { version = "0.26.1", features = ["dnssec-ring", "serde"] }` (message/record types ONLY — proto 0.26 has NO transport features), `hickory-net = { version = "0.26.1", default-features = false, features = ["tokio", "tls-ring", "https-ring", "quic-ring", "h3-ring", "dnssec-ring", "rustls-platform-verifier"] }` (0.26 split ALL client transports here — feature names verified via crates.io API 2026-08-22), and `hickory-resolver = { version = "0.26.1", default-features = false, features = ["tokio", "system-config", "tls-ring", "https-ring", "quic-ring", "h3-ring", "dnssec-ring", "rustls-platform-verifier"] }`; add `hickory-server = { version = "0.26.1", features = ["tls-ring"] }` to `[dev-dependencies]` (0.26.1 defaults carry no TLS support — feature verified via crates.io API 2026-08-22; the DoT fixture in todo 3 needs it). Do NOT add `hickory-client` (no stable 0.26 — only 0.26.0-alpha.1, verified crates.io 2026-08-22); low-level exchanges use hickory-net streams directly (todo 4). This todo is also the WINDOWS BUILD SPIKE: `cargo build` + `cargo test` must succeed on the dev machine (ring/quinn MSVC toolchain check) before any DNS logic is written in later todos. Create `src-tauri/src/dns/mod.rs` declaring submodules (`client`, `query`, `transport`, `names`, `dnssec`, `delegation`, `integrity`, `bench/`, `manager`) as empty `pub mod` files, and a `DnsError` enum (thiserror) with a `kind()` → `&'static str` and a manual `Serialize` impl emitting `{kind, message}` exactly like `DownloadError`. Register `pub mod dns;` in `lib.rs`. Must NOT touch existing modules or commands; must NOT add hickory-server to `[dependencies]`.
  Parallelization: Wave 1 | Blocked by: — | Blocks: 2,3,4,6
  References (executor has NO interview context - be exhaustive): src-tauri/Cargo.toml:16-42 (dep table style); src-tauri/src/download.rs:36-70 (DnsError pattern to mirror); src-tauri/src/lib.rs:1-10 (module registration).
  Acceptance criteria (agent-executable): `cargo build` exits 0; `cargo tree | Select-String hickory` shows hickory-proto 0.26.x, hickory-net 0.26.x, hickory-resolver 0.26.x and NO hickory-client; `cargo clippy --all-targets -- -D warnings` exits 0.
  QA scenarios (name the exact tool + invocation): happy — `cargo build` after edits, Evidence .omo/evidence/dns-tester/task-1-build.log; failure — temporarily set version to "0.99" and confirm `cargo build` fails with a resolution error (then revert), Evidence .omo/evidence/dns-tester/task-1-failure.log.
  Commit: Y | feat(lib): add hickory deps and dns module skeleton

- [x] 2. DNS client/config layer (dns/client.rs)
  What to do / Must NOT do: Define `DnsProtocol` enum (`Udp`, `Tcp`, `Tls`, `Https`, `Quic`, `H3`) with serde camelCase + `FromStr`; `ResolverEndpointDto { name: String, protocol: DnsProtocol, address: String }` where address is `ip:port` for UDP/TCP/TLS/QUIC (default ports 53/53/853/853) or a URL for HTTPS/H3; `System` variant resolved via `ResolverConfig::system()`. Build `make_resolver(endpoint) -> Result<TokioResolver, DnsError>` using hickory-resolver 0.26 (`Resolver::builder_with_config` / `TokioConnectionProvider`). IMPORTANT: `ResolverOpts` is `#[non_exhaustive]` — struct literals do NOT compile; ALWAYS use `let mut opts = ResolverOpts::default(); opts.cache_size = 0; opts.attempts = 1; opts.timeout = Duration::from_secs(2); opts.validate = false;` field-assignment style (validate stays false here; todo 9 controls DNSSEC). One resolver per endpoint; `cache_size = 0` for benchmark isolation. Include `detect_system_resolver() -> Vec<ResolverEndpointDto>`. Unit tests: endpoint parsing (valid/invalid), port defaults, protocol Display/FromStr round-trip. Must NOT use global caching semantics for benchmarks; must NOT silently fall back to the system resolver when an explicit endpoint fails to construct.
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 4,6,7,8,9,10,11,12
  References: src-tauri/src/http_client.rs (settings DTO → client construction pattern); hickory-resolver 0.26 docs https://docs.rs/hickory-resolver/0.26.1 (builder API, NameServerConfig protocols, non_exhaustive ResolverOpts); features verified on crates.io 2026-08-22.
  Acceptance criteria: `cargo test dns::client` exits 0 with ≥6 unit tests; constructing a UDP endpoint `1.1.1.1` yields port 53, TLS yields 853, HTTPS parses `https://cloudflare-dns.com/dns-query`.
  QA scenarios: happy — `cargo test dns::client` output, Evidence .omo/evidence/dns-tester/task-2-tests.log; failure — test that `make_resolver` on `address: "not-an-ip"` returns `DnsError` with kind `invalid-endpoint`, Evidence same file.
  Commit: Y | feat(lib): dns client and endpoint config layer

- [x] 3. Local mock DNS server fixture (hickory-server, dev-only) — ALL behaviors front-loaded
  What to do / Must NOT do: Create `src-tauri/tests/common/mock_dns.rs` spinning up in-process hickory-server instances on 127.0.0.1 with ephemeral UDP+TCP ports AND a DoT (TLS, port-853-style) listener secured by an in-process self-signed test certificate (add `rcgen = "0.13"` to `[dev-dependencies]` for cert generation; the fixture exposes the cert so tests can pin/trust it). The static test zone `mock.test` contains: A/AAAA for `www.mock.test`; MX; TXT (SPF-like); two NS records; SOA with a FIXED serial; CNAME chain `alias.mock.test → www.mock.test`; CNAME loop `loop1 → loop2 → loop1`; wildcard `*.wild.mock.test`; delegation `sub.mock.test` with in-domain NS + glue; `broken-ptr` reverse entry whose PTR target's A record does NOT contain the original IP. Behavioral knobs (query-name-triggered, no config reloads): `big.mock.test` returns an oversized response (TC=1 on UDP, full answer on TCP); `fail.mock.test` returns SERVFAIL; `slow.mock.test` delays beyond the client timeout; NODATA and NXDOMAIN occur naturally for existing/absent names; EDNS payload sizes are honored so the buffer sweep works; `drop10.mock.test` triggers a ~10% packet-drop mode (load-profile degraded test in todo 16 — owned HERE, not added later). Provide `start_mock() -> (MockHandle, SocketAddr)` and `start_mock_variant(serial, answers_override)` for a second instance with a DIFFERENT SOA serial and divergent answers (delegation-inconsistency + filtering tests); enable AXFR on the zone; expose a per-instance atomic QUERY COUNTER (proves cache isolation in todo 14). Helper returns a shutdown handle. Must NOT require root, fixed ports, or external network; must NOT sign the zone (DNSSEC tests use dedicated logic in todo 9).
  Parallelization: Wave 1 | Blocked by: 1 | Blocks: 4,6,7,8,9,10,11,12,14
  References: hickory-server 0.26.1 docs https://docs.rs/hickory-server/0.26.1 (use the 0.26 API names: `Server` and `InMemoryZoneHandler` — NOT the removed `ServerFuture`/`InMemoryAuthority`); existing integration-test layout src-tauri/tests/ (scan_foundations.rs, trace.rs); Oracle review findings (front-loading rationale; TLS listener + cert exposure).
  Acceptance criteria: `cargo test --test mock_dns_smoke` (new smoke test in src-tauri/tests/) exits 0 on Windows and exercises EVERY behavior: plain A, truncation+TCP retry, SERVFAIL, slow-timeout, NXDOMAIN, NODATA, CNAME chain, CNAME loop, wildcard, glue presence, AXFR transfer, divergent second instance, DoT round-trip against the TLS listener (pinning the fixture cert), 10%-drop mode (statistical assertion over 100 queries: 5-15 drops), query counter increments.
  QA scenarios: happy — smoke test log, Evidence .omo/evidence/dns-tester/task-3-smoke.log; failure — point the smoke client at a port 1 above the fixture and assert a timeout error surfaces within 3 s, Evidence same file.
  Commit: Y | test(lib): local mock dns server fixture

- [x] 4. Low-level query API (dns/query.rs) — hickory-proto message exchange
  What to do / Must NOT do: Do NOT use `hickory_resolver::Resolver::lookup` here — it hides rcode/flags/authority/additional sections (Oracle blocker 2). Build `query_once` on hickory-net 0.26.1 client streams directly: construct a `DnsRequest`/`Message` via `hickory_proto::op` (set RD flag, EDNS options, DO bit as requested), send it over the protocol-appropriate stream from `hickory_net` (`udp::UdpClientStream`, `tcp::TcpClientStream`, `tls::TlsClientStream`/`tls::tls_client_connect` for DoT, `h2::HttpsClientStream` for DoH, `quic::QuicClientStream`, `h3::H3ClientStream` — verify exact symbol paths against https://docs.rs/hickory-net/0.26.1 while implementing; DoQ/DoH3 remain best-effort), and await the matching `Message` response. COMPILE SPIKE GATE: `query_once` must compile and pass a fixture round-trip over UDP, TCP, and TLS before any Wave-2 todo starts. Signature: `query_once(endpoint, name, rtype, opts: QueryOpts { rd: bool, timeout, edns_size, dnssec_ok, pinned_root_cert: Option<Vec<u8>> }) -> Result<QueryResultDto, DnsError>` timed with `tokio::time::Instant` (monotonic). `pinned_root_cert` (DER bytes) is the TEST-ONLY trust path: when set, the DoT/DoH client is built with a rustls `ClientConfig` whose root store contains exactly that certificate (required because `rustls-platform-verifier` will never trust the in-memory rcgen fixture cert); production code paths NEVER set it (the frontend DTO has no such field — it exists only on the Rust-side QueryOpts). The TLS compile spike uses it against the todo-3 fixture. `RecordTypeSpec` enum: A, AAAA, MX, TXT, NS, SOA, CNAME, SRV, CAA, PTR + `Other(u16)`. `QueryResultDto` (serde camelCase): `{ queryName, recordType, rcode, answers: Vec<AnswerDto { data, ttl }>, authorityNodata, authoritySoa, adFlag, aaFlag, raFlag, truncated, latencyMs, transportUsed, responseBytes, additionalGlue: Vec<AnswerDto> }`; explicit NXDOMAIN-vs-NODATA classification (`Nodata` = NOERROR + empty answers + SOA in authority). The `rd: false` path is the DIRECT AUTHORITATIVE PRIMITIVE consumed by todos 10/11 for referrals, glue, and filtering ground truth. Map errors to DnsError kinds `timeout`/`servfail`/`refused`; NXDOMAIN is an rcode, NOT an error. Integration tests against the todo-3 fixture for every record type + NXDOMAIN + NODATA + RD=0 referral shape. Must NOT retry on timeout (single attempt); must NOT follow CNAMEs invisibly — raw answer sections only (chain walking is todo 8).
  Parallelization: Wave 1 | Blocked by: 2,3 | Blocks: 6,7,8,9,10,11,12,14
  References: src-tauri/src/download.rs:9-33 (DTO serde pattern); hickory-proto 0.26 `op::Message`, `op::Query`, `op::Edns`, `xfer::DnsRequest` docs; hickory-net 0.26.1 stream modules https://docs.rs/hickory-net/0.26.1 (udp/tcp/tls/h2/quic/h3); Oracle r2 blockers 1-2.
  Acceptance criteria: `cargo test dns::query` exits 0 covering all 10 record types + Other(u16) + NXDOMAIN + NODATA + RD=0 referral with glue visible in `additionalGlue`; DTO serializes to camelCase JSON (snapshot assertion); compile spike: UDP+TCP+TLS round-trips against the fixture pass.
  QA scenarios: happy — integration test run against fixture, Evidence .omo/evidence/dns-tester/task-4-tests.log; failure — query `absent.mock.test` asserts rcode NXDOMAIN and no error; query `www.mock.test` type `Other(65280)` asserts NODATA classification, Evidence same file.
  Commit: Y | feat(lib): low-level dns query api

- [x] 5. Migration 0006 + persistence methods for DNS runs
  What to do / Must NOT do: Add `src-tauri/migrations/0006_dns.sql`: `dns_runs(id INTEGER PRIMARY KEY, kind TEXT CHECK(kind IN ('benchmark','diagnostics')), config_json TEXT NOT NULL, started_at TEXT NOT NULL, ended_at TEXT, status TEXT NOT NULL DEFAULT 'running')`; `dns_run_targets(id INTEGER PRIMARY KEY, run_id INTEGER NOT NULL REFERENCES dns_runs(id) ON DELETE CASCADE, target TEXT NOT NULL, protocol TEXT, metrics_json TEXT NOT NULL)`; index `idx_dns_run_targets_run`. Extend `src-tauri/src/db.rs` with `NewDnsRun`, `DnsRunSummary`, `DnsRunTargetRow`, `LoadedDnsRun` structs + methods `create_dns_run`, `finish_dns_run` (tx: stamp ended_at/status + insert all target rows), `list_dns_runs` (newest first, aggregate target count in SQL), `load_dns_run`, `delete_dns_run`. Update the migration-count assertions in db.rs tests from 5 to 6. Must NOT alter existing tables; must NOT store per-query samples (aggregates JSON only).
  Parallelization: Wave 1 | Blocked by: — | Blocks: 18,20
  References: src-tauri/src/db.rs:202-215 (connect/migrate), 255-307 (create + transactional finish pattern), 740-791 (migration test to update); src-tauri/migrations/0004_scan.sql (table style).
  Acceptance criteria: `cargo test db::tests` exits 0 incl. updated `fresh_db_migrates` asserting versions [1..6] and new tables/index; new unit test inserts a run + 3 targets, lists, loads, deletes (cascade verified).
  QA scenarios: happy — `cargo test db::tests` log, Evidence .omo/evidence/dns-tester/task-5-tests.log; failure — `finish_dns_run` with a target row referencing a bogus run_id rolls back the whole transaction (mirrors `complete_trace_with_hops_rolls_back_on_bad_hop`), Evidence same file.
  Commit: Y | feat(lib): persistence for dns runs (migration 0006)

- [ ] 6. Transport probes (dns/transport.rs)
  What to do / Must NOT do: Implement (a) `edns_buffer_sweep(endpoint, name)` — query with EDNS UDP payload sizes [512, 1232, 1400, 4096] and report per-size rcode/truncated/response size; (b) `tcp_fallback_check(endpoint, name)` — detect `truncated` on UDP then re-query over TCP and report success; (c) `forced_tcp_query`; (d) `transport_equivalence(endpoint_without_protocol, name, rtype)` running the same query over all supported transports and returning a per-transport result matrix (latency, rcode, answer-set hash for equality comparison; DoQ/DoH3 entries may report `unsupported`/`unreachable` — best-effort per Scope, never a pass/fail gate); (e) `edns_support_check` — OPT present, EDNS version 0 handling, and a BADVERS probe with version 1. All built on todo-4's low-level exchange (EDNS/flags control required). Integration tests against the fixture (fixture serves UDP and TCP and honors EDNS sizes). Must NOT fragment-test real MTU (out of scope); must NOT mark a transport "broken" on a single timeout — require 2 attempts before downgrading.
  Parallelization: Wave 1 | Blocked by: 4 | Blocks: 19
  References: hickory-proto 0.26 `op::Edns`, `op::Query`, `xfer::DnsRequest` docs; hickory-net 0.26.1 stream modules; dnsdiag buffer-size methodology (research notes in draft Findings).
  Acceptance criteria: `cargo test dns::transport` exits 0; sweep returns 4 entries in ascending size order; equivalence matrix on the fixture shows UDP==TCP answer hash.
  QA scenarios: happy — integration tests vs fixture, Evidence .omo/evidence/dns-tester/task-6-tests.log; failure — against an unroutable endpoint (127.0.0.1 on a closed port) every probe returns a typed error, no panic, Evidence same file.
  Commit: Y | feat(lib): dns transport probes (edns, tcp fallback, equivalence)

- [ ] 7. Lookup orchestration command (run_dns_lookup)
  What to do / Must NOT do: `#[tauri::command] run_dns_lookup(name: String, record_types: Vec<String>, endpoint: ResolverEndpointDto, on_result: Channel<LookupEventDto>) -> Result<LookupSummaryDto, DnsError>` in `src-tauri/src/dns/manager.rs` (thin; logic in dns/query.rs + dns/names.rs). Streams one `LookupEventDto` per completed record query (query result DTO from todo 4) and returns a summary `{ name, resolver, completed, failed, elapsedMs }`. Record types come as strings parsed via `RecordTypeSpec::from_str` (unknown → per-type error event, not a whole-command failure). Frontend wrapper in ipc.ts lands in todo 21 — this todo is backend only. Must NOT persist lookups (ephemeral per plan); must NOT parallelize beyond 8 concurrent record queries for one lookup.
  Parallelization: Wave 2 | Blocked by: 4 | Blocks: 19,21
  References: src-tauri/src/lib.rs:68-78 (command + Channel pattern), src-tauri/src/download.rs:111-120 (streaming fn signature); todo 4 DTOs.
  Acceptance criteria: `cargo test dns::lookup` exits 0: against the todo-3 fixture, a 6-record-type lookup streams 6 events then resolves; an invalid type yields an error event and the command still completes.
  QA scenarios: happy — integration test log, Evidence .omo/evidence/dns-tester/task-7-tests.log; failure — empty `record_types` returns DnsError kind `invalid-input`; unreachable endpoint produces per-type timeout events, not a hang (assert wall time < 3×timeout), Evidence same file.
  Commit: Y | feat(lib): run_dns_lookup command with streamed results

- [ ] 8. Name analysis (dns/names.rs): rDNS+fcRDNS, CNAME chain, wildcard, NXDOMAIN/NODATA
  What to do / Must NOT do: (a) `reverse_lookup(endpoint, ip)` — PTR via todo-4 query, then forward-confirm by resolving the returned name and checking the original IP is in the answer set (`fcrdns: bool` in result); (b) `cname_chain(endpoint, name)` — iterative CNAME following (max 16 hops) returning the ordered chain and `loop_detected: bool`; (c) `wildcard_check(endpoint, zone)` — query a random 12-char label under the zone; wildcard iff answers present; (d) NXDOMAIN-vs-NODATA classification is already in todo-4 DTO — expose `classify_name(endpoint, name)` returning per-common-type existence matrix. Integration tests use fixture cases: `alias.mock.test` chain, `loop1/loop2` loop, `*.wild.mock.test` wildcard, `absent.mock.test` NXDOMAIN. Must NOT follow more than 16 CNAME hops; must NOT treat a wildcard NXDOMAIN-proof as authoritative absence (documented in code comment only).
  Parallelization: Wave 2 | Blocked by: 4 | Blocks: 19
  References: src-tauri/src/scan/rdns.rs (existing rDNS conventions — semantics only, do not reuse its dns-lookup path); todo-3 fixture zone contents.
  Acceptance criteria: `cargo test dns::names` exits 0 covering all four behaviors incl. loop detection and negative wildcard.
  QA scenarios: happy — test log, Evidence .omo/evidence/dns-tester/task-8-tests.log; failure — fcRDNS mismatch (fixture `broken-ptr` entry) asserts `fcrdns == false`, Evidence same file.
  Commit: Y | feat(lib): dns name analysis (rdns, cname chain, wildcard)

- [ ] 9. DNSSEC diagnostics (dns/dnssec.rs)
  What to do / Must NOT do: `dnssec_report(endpoint, domain) -> DnssecReportDto { hasDnskey, hasDs, hasRrsig, adFlag, validates, bogusDomainRejected, notes: Vec<String> }`. Validation path: construct the resolver with field-assigned opts (`let mut opts = ResolverOpts::default(); opts.validate = true;`) + `dnssec-ring` (hickory built-in root trust anchors) — never infer validation from an upstream AD bit; AD flag is reported as a separate observational field. `bogusDomainRejected`: query `dnssec-failed.org` and require SERVFAIL (real-network test, `#[ignore]`). Signed-zone positive control: `cloudflare.com` validates with AD=1 (same ignored test). Against the todo-3 mock (unsigned zone): assert report shows `validates == false`, `hasRrsig == false`, and NO bogus classification (unsigned ≠ broken). UI-facing caveat string constant: DNSSEC validation is experimental in hickory — surface it in `notes`. Also record RRSIG inception/expiration window check when signatures exist. Must NOT mark an unsigned zone as failing; must NOT enable `validate` on resolvers used by the benchmark engine (measurement isolation).
  Parallelization: Wave 2 | Blocked by: 4 | Blocks: 19
  References: hickory-resolver 0.26 `ResolverOpts::validate` (field, non_exhaustive struct), dnssec-ring docs; Metis finding B2 (draft .omo/drafts/dns-tester.md).
  Acceptance criteria: `cargo test dns::dnssec` exits 0 (mock-based: unsigned-zone report shape, window-parse unit tests); `cargo test -- --ignored dnssec_real` passes on a networked machine asserting dnssec-failed.org → SERVFAIL and cloudflare.com → AD=1.
  QA scenarios: happy — test logs, Evidence .omo/evidence/dns-tester/task-9-tests.log + task-9-real.log; failure — mock zone with an RRSIG whose expiration is in the past yields a `signature-expired` note, Evidence same file.
  Commit: Y | feat(lib): dnssec diagnostics with explicit validation

- [ ] 10. Delegation diagnostics (dns/delegation.rs)
  What to do / Must NOT do: `delegation_report(endpoint, domain) -> DelegationReportDto` covering: (a) parent/child NS set comparison — use todo-4's DIRECT primitive (`rd: false`) against the discovered authoritative servers, NOT the recursive endpoint (recursive answers hide referrals); if authoritative discovery is impossible, return a typed `no-reference` state for that check; (b) glue completeness: every in-domain NS name has A/AAAA in the additional section of the delegation response; glue correctness: parent glue == child authoritative A/AAAA; (c) per-NS reachability matrix: each NS address × {v4,v6} × {UDP,TCP}; IPv6 cells report `notAvailable` when the machine has no IPv6 route/interface (detect via local interface list) — `notAvailable` is NEVER a failure; (d) SOA serial consistency across all reachable NS. Fixture: `sub.mock.test` delegation with glue (todo 3) + second mock instance with a different serial. Must NOT query root servers directly; must NOT mark single-NS zones as inconsistent on one timeout — require 2 attempts before "unreachable".
  Parallelization: Wave 2 | Blocked by: 4 | Blocks: 19
  References: Zonemaster delegation/glue methodology (draft Findings); RFC 9471 glue-in-referral rules; todo-3 fixture; Oracle findings 3+5.
  Acceptance criteria: `cargo test dns::delegation` exits 0: consistent fixture zone → all checks pass; two-instance serial mismatch → `serialConsistent == false`; NS with TCP-only listener → matrix shows udp:false/tcp:true; simulated no-IPv6 environment → v6 cells `notAvailable`, overall check still passes.
  QA scenarios: happy — test log, Evidence .omo/evidence/dns-tester/task-10-tests.log; failure — fixture variant missing glue for in-domain NS yields `glueComplete == false` with the offending NS named in the report, Evidence same file.
  Commit: Y | feat(lib): delegation and glue diagnostics

- [ ] 11. Resolver integrity + opt-in AXFR (dns/integrity.rs)
  What to do / Must NOT do: (a) `answer_equivalence(endpoints, name, rtype)` — same query to N resolvers; report pairwise answer-set hashes, rcode agreement, TTL spread, AD agreement; (b) `filtering_check(endpoint, probes)` — reference ground truth = todo-4's direct authoritative primitive (`rd: false` against the zone's authoritative NS discovered via todo-10 logic, never a hardcoded IP); detect NXDOMAIN-rewriting (endpoint NXDOMAIN where reference has answers) and answer-injection (answers differ from reference with equal rcode); (c) `open_resolver_check(endpoint)` — RD=1 query for an unrelated external name; open iff a recursive answer comes back from a server expected to be authoritative-only (only meaningful for user-supplied authoritative targets — document); (d) `axfr_check(ns_address, zone, confirm_token)` — attempts AXFR over TCP ONLY when `confirm_token == zone` exactly (string equality, no trimming); AXFR is a MULTI-MESSAGE TCP transfer, not a todo-4 single exchange: implement it as an explicit TCP loop over `hickory_net::tcp::TcpClientStream` reading length-prefixed `Message` frames until the terminating (second) SOA record, with a 10 s overall deadline and a 10 000-record safety cap; returns exposure bool + record count on success. Tests: equivalence + filtering against the two divergent fixture instances; AXFR against the fixture covering FOUR cases: multi-message success (zone split across ≥2 frames), early EOF mid-transfer (typed `truncated-transfer` error), timeout (slow fixture), SOA-termination correctness; rejection test with wrong token. Must NOT attempt IXFR; must NOT retry AXFR; must NOT run filtering_check without a discoverable authoritative reference (return typed `no-reference` instead).
  Parallelization: Wave 2 | Blocked by: 4 | Blocks: 19
  References: Metis findings m4/M6; Oracle finding 3; hickory-server AXFR support docs (dev fixture); dnsdiag dnstraceroute methodology (draft Findings).
  Acceptance criteria: `cargo test dns::integrity` exits 0 incl.: divergent fixtures flagged inequivalent; NXDOMAIN-rewrite detected; `axfr_check(ns, "mock.test", "wrong")` returns DnsError kind `confirmation-required`; correct token returns exposure report.
  QA scenarios: happy — test log, Evidence .omo/evidence/dns-tester/task-11-tests.log; failure — filtering_check on a name whose authoritative NS is undiscoverable returns `no-reference` (not a crash/guess), Evidence same file.
  Commit: Y | feat(lib): resolver integrity checks and opt-in axfr

- [ ] 12. Benchmark scheduler core (dns/bench/scheduler.rs)
  What to do / Must NOT do: `BenchScheduler` only (mixes are todo 14): one tokio task per (resolver × cell), semaphore-bounded concurrency (hard cap 64 — requests above are clamped AND the clamp is echoed in `RunConfigDto`), per-query timeout (default 2 s, configurable), per-cell sample ring capped at 100 000 entries (drop-oldest, sets `truncated: true`), randomized resolver order per repetition, per-query `Sample { latency_ms, outcome: Ok/Timeout/Servfail/Refused/Nxdomain/NoData, cold_conn: bool }` streamed to an internal collector (NOT to a Tauri Channel — channel coalescing is todo 18). Cancellation via `tokio_util::sync::CancellationToken` (add `tokio-util = "0.7"` to Cargo.toml): cancel stops scheduling, aborts in-flight within 500 ms, returns partial samples. Queries execute through todo-4's `query_once`. Tests with the instrumented fixture: concurrency cap respected; cancellation <500 ms with partial samples; ring cap truncates + flags. Must NOT share resolver/client instances across cells; must NOT exceed the sample cap silently.
  Parallelization: Wave 2 | Blocked by: 4 | Blocks: 14,15,16,17
  References: src-tauri/src/scan/rdns.rs (JoinSet + semaphore pattern); methodology Findings in .omo/drafts/dns-tester.md; Oracle finding 7 (split rationale).
  Acceptance criteria: `cargo test dns::bench::scheduler` exits 0 for: cap clamp+echo, cancel-latency bound, ring-cap truncation, per-cell isolation (no shared state).
  QA scenarios: happy — test log, Evidence .omo/evidence/dns-tester/task-12-tests.log; failure — scheduler asked for concurrency 65 clamps to 64 and the clamp appears in the echoed config, Evidence same file.
  Commit: Y | feat(lib): benchmark scheduler core with cancellation

- [x] 13. Benchmark statistics (dns/bench/stats.rs)
  What to do / Must NOT do: Pure functions over `&[Sample]`: `percentile(sorted_samples, p)` (linear interpolation from RAW samples — never per-bucket averages; code comment cites the flamethrower averaged-averages warning). `aggregate(samples) -> MetricsDto` where `count, min, median, mean, max, timeoutRate, servfailRate, nxdomainRate, completedQps` are ALWAYS computed when count>0 (median powers the live "current median" UI), while `p90, p95, p99` are `Option<f64>` — `None` when count < 1000 (UI shows "insufficient samples"). `reliability_score(cells) -> Vec<ScoreDto>` with the EXACT formula: `score = 50*success_rate + 25*(1 - min(p95,500)/500) + 15*(1 - timeout_rate) + 10*(1 - min(1, spread))` where `spread = (p95 - median) / max(median, 1ms)` and `p95` falls back to `max` when `None`. Unit tests with hand-computed fixtures incl. the exact-score golden test, empty-samples, all-timeout, and the N<1000 → None rule. Must NOT compute p90/p95/p99 from fewer than 1000 samples; must NOT NaN on empty input.
  Parallelization: Wave 1-2 (anywhere) | Blocked by: — | Blocks: 15,16,17
  References: flamethrower metrics.cpp averaged-averages warning (draft Findings); Metis M3/m5; Oracle finding 8; src-tauri/src/stats.rs (existing stats module conventions).
  Acceptance criteria: `cargo test dns::bench::stats` exits 0 with ≥10 tests incl. golden vectors; `aggregate(&[])` returns zeroed metrics, no panic.
  QA scenarios: happy — test log, Evidence .omo/evidence/dns-tester/task-13-tests.log; failure — 999-sample cell yields `p99 == None` with `median` still `Some`, 1000-sample cell yields `Some(p99)`, Evidence same file.
  Commit: Y | feat(lib): benchmark percentile statistics and reliability score

- [ ] 14. Query mixes + cache isolation (dns/bench/mixes.rs)
  What to do / Must NOT do: (a) `QueryMix` generators: `PopularWeighted` (embedded static list of 1000 popularity-ordered domains at `src-tauri/src/dns/bench/top1000.txt` via `include_str!`, sampled by weight), `UniqueLabels { base }` (random 12-char labels under base — every DTO/UI string labels these "uncached approximation"), `Mixed { ratio }` (default 50/50 popular/unique); (b) cache isolation: each measurement cell constructs a FRESH resolver instance with `cache_size = 0` (todo-2 field-assignment style) so hickory's internal LRU cannot poison warm/cold measurements — the acceptance test uses the todo-3 fixture's QUERY COUNTER to prove two identical queries in the same cell hit the wire twice. Tests: generator cardinality/distribution (seeded RNG for determinism), top1000.txt parses to exactly 1000 valid names, isolation wire-count. Must NOT let a cold-label query leak into a warm cell; must NOT reuse a resolver instance across cells.
  Parallelization: Wave 2 | Blocked by: 3,12 | Blocks: 15,16
  References: Metis M1; namebench dataset methodology (draft Findings); todos 2,3,12.
  Acceptance criteria: `cargo test dns::bench::mixes` exits 0 incl. the wire-count isolation test and the 1000-domain list parse test.
  QA scenarios: happy — test log, Evidence .omo/evidence/dns-tester/task-14-tests.log; failure — seeded `Mixed { ratio: 0.5 }` over 200 draws yields 100±0 popular/100±0 unique with the fixed seed (exact split asserted), Evidence same file.
  Commit: Y | feat(lib): benchmark query mixes and cache isolation

- [ ] 15. Everyday + Cache-behavior profiles (dns/bench/profiles.rs)
  What to do / Must NOT do: (a) `Everyday` — 1000 weighted popular domains per resolver, concurrency 1–4, one warmup round of 20 queries per resolver EXCLUDED from metrics; (b) `CacheBehavior` — three sub-run cells per resolver: warm (repeated popular names), cold (UniqueLabels under a configurable base domain, "uncached approximation" label), mixed 50/50 — reported as SEPARATE cells in the DTO. Every run echoes its effective `RunConfigDto`. Tests with the mock fixture: warmup samples absent from aggregates; warm/cold/mixed cells separated; config echo correct. Must NOT include warmup samples in metrics; must NOT merge cache cells into one aggregate.
  Parallelization: Wave 3 | Blocked by: 12,13,14 | Blocks: 18
  References: methodology Findings (draft); Metis M3; todos 12,13,14.
  Acceptance criteria: `cargo test dns::bench::profiles` exits 0: everyday run over the fixture produces one cell per resolver with count = 1000 and no warmup contamination; cache run produces exactly 3 cells per resolver.
  QA scenarios: happy — test log, Evidence .omo/evidence/dns-tester/task-15-tests.log; failure — aggregate of a warm cell must not equal the mixed cell on the instrumented fixture (proves separation), Evidence same file.
  Commit: Y | feat(lib): everyday and cache benchmark profiles

- [ ] 16. Load profile with pacing + guards (dns/bench/load.rs)
  What to do / Must NOT do: Concurrency ladder [4, 16, 64] with offered-QPS pacing (token-bucket, measured offered QPS within ±10% of target in a fixture test); hard caps enforced here AND re-validated at the command layer (≤100 offered QPS, ≤60 s total, c≤64) — out-of-cap config returns `DnsError` kind `over-cap`; target guard: Load accepts ONLY user-entered endpoints or loopback/LAN addresses — preset resolvers (Cloudflare/Google/Quad9/system) are refused with kind `load-target-not-allowed`; stop a ladder step early when timeout rate >5% and mark the run `degraded: true`; report offered-vs-completed QPS per step. Tests: pacing accuracy vs fixture, cap rejections, preset refusal, degraded early-stop against the fixture's `drop10.mock.test` 10%-drop mode (defined in todo 3 — do NOT extend the fixture here). Must NOT run load against a preset even if the UI sends it (defense in depth); must NOT continue a step past its caps.
  Parallelization: Wave 3 | Blocked by: 12,13,14 | Blocks: 18
  References: Metis M5; resperf methodology (draft Findings); todos 12,13,14.
  Acceptance criteria: `cargo test dns::bench::load` exits 0 for: pacing ±10%, over-cap rejection, preset rejection, degraded flag on 10% loss, offered-vs-completed QPS series present.
  QA scenarios: happy — test log, Evidence .omo/evidence/dns-tester/task-16-tests.log; failure — `Load` config naming `1.1.1.1` is rejected with `load-target-not-allowed` before any packet is sent (fixture counter proves zero I/O), Evidence same file.
  Commit: Y | feat(lib): load benchmark profile with pacing guards

- [ ] 17. Cold-vs-reused connection measurement (dns/bench/conn.rs)
  What to do / Must NOT do: For encrypted transports (DoT/DoH/DoQ/DoH3): measure `cold` cells (fresh connection incl. TCP+TLS+HTTP/QUIC setup, recording `conn_setup_ms` separately) vs `reused` cells (persistent client stream, steady-state per-query cost) as SEPARATE cells — never mixed. For UDP/TCP all samples are `cold_conn: false`. Report per encrypted endpoint: connection setup time, first-query total, reused median/p95 (percentile rules from todo 13), reconnect count. Tests: cell-separation logic with a fake transport injecting a synthetic setup delay (DoT against a real TLS fixture optional — logic test suffices); reconnect counter increments on injected failure. Must NOT include handshake time in reused-cell latencies; must NOT hide reconnects.
  Parallelization: Wave 3 | Blocked by: 12,13 | Blocks: 18
  References: methodology Findings (draft: DoH/DoT handshake overhead section); todos 12,13.
  Acceptance criteria: `cargo test dns::bench::conn` exits 0: synthetic transport with 50 ms setup delay yields cold-cell median ≈ reused-cell median + 50 ms (±20%); reconnect counter test passes.
  QA scenarios: happy — test log, Evidence .omo/evidence/dns-tester/task-17-tests.log; failure — injected mid-run connection failure increments `reconnects` and does not abort the run, Evidence same file.
  Commit: Y | feat(lib): cold vs reused connection measurement

- [ ] 18. Benchmark command + DnsManager core (bench path only)
  What to do / Must NOT do: EXTEND the existing `src-tauri/src/dns/manager.rs` created in todo 7 (which already owns `run_dns_lookup` — preserve and keep it registered) into `DnsManager { db: Database, cancel: CancellationToken registry }` mirroring TraceManager; register in `lib.rs` `generate_handler!` and `app.manage(DnsManager::new(dns_db))` in setup (new `Database::connect` like existing managers). Commands in THIS todo: `run_dns_benchmark(config, on_progress: Channel<BenchProgressDto>, on_status: Channel<BenchStatusDto>) -> Result<DnsRunSummaryDto, DnsError>` and `stop_dns_benchmark()`. CHANNEL POLICY (Oracle r1 finding 6): benchmark progress is coalesced to per-cell AGGREGATE snapshots at ≤500 ms cadence — never per-sample events; a `Channel::send` error switches the run to DETACHED mode (stop sending, keep running, still persist). Persistence: create run row at start (kind='benchmark'), finish with status completed|cancelled|error + per-target aggregates JSON (metrics from todo 13, NOT raw samples). Cancellation acceptance: cancel mid-benchmark → in-flight stopped <500 ms AND run row persisted 'cancelled' (integration test with slow fixture). Command layer re-validates todo-16 caps (defense in depth). Must NOT implement diagnostics or history commands here; must NOT block the Tauri runtime thread.
  Parallelization: Wave 3 | Blocked by: 5,15,16,17 | Blocks: 20,21
  References: src-tauri/src/lib.rs:106-141,208-253 (manager + registration pattern); src-tauri/src/db.rs todo-5 methods; src-tauri/src/trace.rs (manager shape); todos 15,16,17.
  Acceptance criteria: `cargo test dns::manager` exits 0 incl. cancellation-persistence test and a coalescing test (≥100 mock samples produce ≤ elapsed/500ms + 1 progress events); `cargo build` exits 0 with both commands registered.
  QA scenarios: happy — integration test: full benchmark vs mock persists run + targets, Evidence .omo/evidence/dns-tester/task-18-tests.log; failure — detached-mode test: close the channel receiver and assert the run still completes and persists, Evidence same file.
  Commit: Y | feat(lib): dns benchmark command and manager core

- [ ] 19. Diagnostics command (run_dns_diagnostics)
  What to do / Must NOT do: Add `run_dns_diagnostics(domain, checks: Vec<String>, axfr_token: Option<String>, on_item: Channel<DiagItemDto>) -> Result<DiagReportDto, DnsError>` to the manager from todo 18, orchestrating the check suites from todos 6,8,9,10,11 (transport probes on the domain's NS set, name analysis, DNSSEC report, delegation report, integrity checks); unknown check names are skipped with a warning item, not an error. Stream one `DiagItemDto` per finished check `{ check, status: pass|warn|fail|notAvailable, summary, detailJson }`. Persist kind='diagnostics' runs via todo-5 db methods. The AXFR check requires `axfr_token == domain` before ANY network I/O for that check (fixture counter proves zero I/O otherwise). Must NOT reorder check execution nondeterministically (fixed order so reports are stable); must NOT persist lookups.
  Parallelization: Wave 3 | Blocked by: 5,6,7,8,9,10,11,18 | Blocks: 20,21
  References: todos 6,8,9,10,11 (check functions + DTOs); src-tauri/src/lib.rs command pattern; todo 18 manager.
  Acceptance criteria: `cargo test dns::diagnostics` exits 0: full check suite vs fixture emits one item per requested check in stable order; AXFR without token returns `confirmation-required` with zero network I/O.
  QA scenarios: happy — integration test log, Evidence .omo/evidence/dns-tester/task-19-tests.log; failure — unknown check name yields a warning item and the command still completes, Evidence same file.
  Commit: Y | feat(lib): dns diagnostics command orchestration

- [ ] 20. History commands + DTO polish + docs
  What to do / Must NOT do: Add `list_dns_runs`, `load_dns_run`, `delete_dns_run` commands (thin over todo-5 db methods). Finalize `DnsRunSummaryDto { id, kind, configEcho, startedAt, endedAt, status, targetCount }` and `LoadedDnsRunDto { run, targets }` serde-camelCase shapes consumed by the frontend; add rustdoc on every public dns module item (repo style is doc comments on DTOs — see db.rs); add the DNS section to README.md tools list. Must NOT change any command signature after this todo (frontend builds against them).
  Parallelization: Wave 3 | Blocked by: 5,18,19 | Blocks: 21
  References: src-tauri/src/session.rs (LoadedSessionDto shape); src-tauri/src/lib.rs:143-161 (list/load/delete pattern); README.md (tools list).
  Acceptance criteria: `cargo doc --no-deps` exits 0 without missing-doc warnings on dns items; list/load/delete round-trip test passes (create via todo-18/19 paths, list, load, delete, verify cascade); README renders the new row; DTO JSON snapshot test passes.
  QA scenarios: happy — `cargo test dns::manager` + snapshot test log, Evidence .omo/evidence/dns-tester/task-20-tests.log; failure — rename a DTO field, watch the snapshot test fail (then revert), Evidence same file.
  Commit: Y | docs(lib): dns history commands, dtos, and readme entry

- [ ] 21. Frontend types, IPC wrappers, routing entry
  What to do / Must NOT do: Add DNS DTOs + event unions to `src/lib/types.ts` mirroring the backend camelCase DTOs from todos 4-20 (ResolverEndpointDto, DnsProtocol, QueryResultDto, LookupEventDto, BenchConfigDto, BenchProgressDto, BenchStatusDto, MetricsDto, ScoreDto, DiagItemDto, DiagReportDto, DnsRunSummaryDto, LoadedDnsRunDto); add wrappers to `src/lib/ipc.ts`: `runDnsLookup`, `runDnsBenchmark(onProgress, onStatus)`, `stopDnsBenchmark`, `runDnsDiagnostics(onItem)`, `listDnsRuns`, `loadDnsRun`, `deleteDnsRun` — Channel pattern identical to startTrace; routing: add `"dns"` to Route union, NAV_ITEMS entry `{ route: "dns", label: "DNS Tester", hash: "#/dns" }`, resolveRoute case; render the view in `src/App.tsx`; update `src/routing.test.ts`. Create `src/views/DnsTesterView.tsx` + `DnsTesterView.module.css` shell with the three-tab switcher (Lookup | Benchmark | Diagnostics) using local state (no routing changes per tab). Must NOT introduce a state library; must NOT break existing routes (routing tests stay green).
  Parallelization: Wave 4 | Blocked by: 18,19,20 | Blocks: 22,23,24,25,26
  References: src/routing.ts:1-23; src/App.tsx; src/lib/ipc.ts:75-89 (Channel wrapper pattern); src/lib/types.ts (DTO conventions); src/routing.test.ts.
  Acceptance criteria: `npx vitest run src/routing.test.ts` exits 0 incl. new `#/dns` case; `npm run build` exits 0; the app renders the DNS Tester nav item (verified by Playwright in todo 27).
  QA scenarios: happy — vitest + build logs, Evidence .omo/evidence/dns-tester/task-21-tests.log; failure — unknown hash still falls back to ping (existing assertion kept green), Evidence same file.
  Commit: Y | feat(ui): dns tester types, ipc wrappers, and routing

- [ ] 22. Lookup tab (useDnsLookup + components)
  What to do / Must NOT do: `src/hooks/useDnsLookup.ts` (follow useDownloadSpeedTest shape: inputs, isRunning, error, results, reset) calling `runDnsLookup` and collecting streamed per-record events; `src/components/dns/LookupTab.tsx`: domain input with validation (trim, IDN left as-is, reject empty/schemes), record-type multi-select checkboxes (A, AAAA, MX, TXT, NS, SOA, CNAME, SRV, CAA, PTR), resolver picker (presets: System, Cloudflare, Google, Quad9 + custom endpoint input with protocol select), results table (type, rcode, answers w/ TTL, latency ms, flags AD/AA/TC), inline per-type error rows. Include the "system preset bypasses OS cache" label under the resolver picker. data-testid attributes on all interactive elements (repo convention). Must NOT persist lookups; must NOT fire queries on every keystroke (explicit Run button only).
  Parallelization: Wave 4 | Blocked by: 21 | Blocks: 27
  References: src/hooks/useDownloadSpeedTest.ts:39-92; src/views/DownloadSpeedView.tsx (form + results layout); src/views/*.module.css (CSS module conventions); Metis M2 label.
  Acceptance criteria: `npx vitest run src/hooks/useDnsLookup.test.ts` exits 0 (mocked ipc: streams 3 events → 3 rows; invoke rejection → error state); `npm run build` exits 0.
  QA scenarios: happy — vitest log, Evidence .omo/evidence/dns-tester/task-22-tests.log; failure — empty domain disables Run (assert in test via isValid false), Evidence same file.
  Commit: Y | feat(ui): dns lookup tab

- [ ] 23. Benchmark tab (useDnsBenchmark + results table)
  What to do / Must NOT do: `src/hooks/useDnsBenchmark.ts` — config state (profile, resolvers multi-select, custom endpoints, duration/sample knobs within caps), start/stop (stop → `stopDnsBenchmark`), progress state from BenchProgressDto, final DnsRunSummaryDto; `src/components/dns/BenchmarkTab.tsx`: profile cards (Everyday / Cache behavior / Load), Load profile shows the opt-in confirmation dialog (explicit checkbox: "I am authorized to load-test these resolvers") and DISABLES preset resolvers with an explanatory hint; live progress per resolver (completed/total, current median — median is always present per todo 13); results table per resolver × cell: count, median, p90/p95/p99 (or "insufficient samples" when null), timeout %, SERVFAIL %, NXDOMAIN %, completed QPS, reliability score with a tooltip/popover showing the exact formula; "uncached approximation" label on cold cells. Must NOT let the UI send out-of-cap values (clamp + disable, backend still rejects); must NOT render p90/p95/p99 when the DTO fields are null.
  Parallelization: Wave 4 | Blocked by: 21 | Blocks: 27
  References: src/hooks/useLanScan.ts (buffered stream + cancel pattern); src/hooks/useDownloadSpeedTest.ts; todo 13 formula; Metis M3/M5; Oracle finding 8.
  Acceptance criteria: `npx vitest run src/hooks/useDnsBenchmark.test.ts` exits 0 (start/progress/stop/final flow with mocked channels; load+preset combination blocked client-side); `npm run build` exits 0.
  QA scenarios: happy — vitest log, Evidence .omo/evidence/dns-tester/task-23-tests.log; failure — mocked invoke rejection mid-run sets error and clears isRunning, Evidence same file.
  Commit: Y | feat(ui): dns benchmark tab

- [ ] 24. Benchmark charts (src/components/dns/BenchCharts.tsx)
  What to do / Must NOT do: uPlot charts following PingGraphs exactly (refs + useEffect lifecycle + ResizeObserver width + setData updates): (a) latency histogram per selected resolver; (b) CDF overlay (one series per resolver); (c) p50/p95/p99-over-time from streamed progress; (d) offered-vs-completed QPS (load profile). Data shaping helpers in `src/lib/dnsGraphData.ts` with vitest coverage; reuse `src/lib/downsample.ts` for long runs. Must NOT create uPlot instances outside useEffect; must NOT leak instances on unmount (destroy in cleanup — mirror PingGraphs).
  Parallelization: Wave 4 | Blocked by: 21 | Blocks: 27
  References: src/components/PingGraphs.tsx; src/lib/graphData.ts; src/lib/downsample.ts + downsample.test.ts; src/uplot.d.ts.
  Acceptance criteria: `npx vitest run src/lib/dnsGraphData.test.ts` exits 0 (histogram binning, CDF math, percentile series shaping with golden inputs); `npm run build` exits 0.
  QA scenarios: happy — vitest log, Evidence .omo/evidence/dns-tester/task-24-tests.log; failure — empty samples produce empty chart data (no crash, no NaN series — asserted), Evidence same file.
  Commit: Y | feat(ui): dns benchmark charts

- [ ] 25. Diagnostics tab (useDnsDiagnostics + checklist UI)
  What to do / Must NOT do: `src/hooks/useDnsDiagnostics.ts` streaming DiagItemDto per check; `src/components/dns/DiagnosticsTab.tsx`: domain input, check group toggles (DNSSEC / Delegation & glue / NS reachability / Answer equivalence / Filtering / Open resolver / AXFR), AXFR toggle reveals a confirmation field where the user must RETYPE the exact zone name (sent as axfr_token; backend enforces equality); results as a checklist with pass/warn/fail/`notAvailable` states + expandable detail JSON; DNSSEC section shows the "experimental validation" caveat note from the report; run persists to history (kind='diagnostics'). Must NOT enable the AXFR Run state until the typed token matches the domain exactly (UI) — backend re-checks regardless.
  Parallelization: Wave 4 | Blocked by: 21 | Blocks: 27
  References: src/hooks/useTraceroute.ts (item+status channel model); src/views/TracerouteView.tsx (live list rendering); Metis M6.
  Acceptance criteria: `npx vitest run src/hooks/useDnsDiagnostics.test.ts` exits 0 (streamed items accumulate; token mismatch keeps AXFR disabled; error path sets error); `npm run build` exits 0.
  QA scenarios: happy — vitest log, Evidence .omo/evidence/dns-tester/task-25-tests.log; failure — token "Example.com" vs domain "example.com" mismatch (case-sensitive) asserted disabled, Evidence same file.
  Commit: Y | feat(ui): dns diagnostics tab with axfr guard

- [ ] 26. Run history UI (benchmark + diagnostics)
  What to do / Must NOT do: History panel inside the DNS Tester view (collapsible section below the tabs, following the existing history UX in the other views): list runs (kind badge, startedAt, status, targetCount), load a run back into the corresponding tab (rehydrate results table + charts), delete with confirm. Wire via listDnsRuns/loadDnsRun/deleteDnsRun. Must NOT add a separate route; must NOT auto-refresh while a run is active (refresh on run finish + manual refresh button).
  Parallelization: Wave 4 | Blocked by: 21 | Blocks: 27
  References: history patterns in src/views/LanScanView.tsx + useLanScan.ts (list/load/delete flow); todo 20 DTOs.
  Acceptance criteria: `npx vitest run src/hooks/useDnsHistory.test.ts` exits 0 (list on mount, load rehydrates, delete removes); `npm run build` exits 0.
  QA scenarios: happy — vitest log, Evidence .omo/evidence/dns-tester/task-26-tests.log; failure — load of a deleted id surfaces the backend error message in the panel, Evidence same file.
  Commit: Y | feat(ui): dns run history panel

- [ ] 27. Vitest coverage completion + Playwright e2e
  What to do / Must NOT do: Extend `e2e/mock-ipc.ts` with all DNS commands (run_dns_lookup/run_dns_benchmark/stop_dns_benchmark/run_dns_diagnostics/list_dns_runs/load_dns_run/delete_dns_run) simulating channels with timers exactly like existing mocks; add `e2e/dns-tester.spec.ts`: nav shows DNS Tester; lookup tab runs against mock and renders rows; benchmark tab everyday profile completes and shows score column; load profile disables presets without opt-in; diagnostics AXFR field gates on token; history lists a finished mock run. Fill any vitest gaps so every new hook/lib file has a test file. Add `src-tauri/tests/dns_real.rs` with `#[ignore]` real-network tests (presets reachable, dnssec-failed SERVFAIL) mirroring the README's ignored-e2e convention. Must NOT make CI e2e depend on real network; must NOT weaken existing specs.
  Parallelization: Wave 5 | Blocked by: 22,23,24,25,26 | Blocks: 28
  References: e2e/mock-ipc.ts; e2e/lan-scan.spec.ts + download-speed.spec.ts (spec conventions); playwright.config.ts; README.md ignored-e2e note.
  Acceptance criteria: `npx playwright test` exits 0 (all specs, old + new); `npx vitest run` exits 0; `cargo test` exits 0; `cargo test -- --ignored dns_real` passes on a networked machine (record output; if the network blocks it, note and skip with justification).
  QA scenarios: happy — full suite logs, Evidence .omo/evidence/dns-tester/task-27-*.log; failure — temporarily break a mock channel and confirm the new spec fails (then revert), Evidence .omo/evidence/dns-tester/task-27-negative.log.
  Commit: Y | test(e2e): dns tester playwright specs and mock ipc

- [ ] 28. Final gates: clippy, build, full test suites, plan compliance self-check
  What to do / Must NOT do: Run `cargo clippy --all-targets -- -D warnings`, `cargo test`, `npm run build`, `npx vitest run`, `npx playwright test` — all must exit 0; fix only what this feature broke; re-read the plan Must-NOT list and diff against `git status`/`git diff --stat` proving no out-of-scope files changed. Must NOT suppress lints, skip tests, or touch unrelated files.
  Parallelization: Wave 5 | Blocked by: 27 | Blocks: final verification wave
  References: package.json scripts; README.md test commands; plan Scope/Must-NOT sections.
  Acceptance criteria: every command above exits 0; `git diff --stat` touches only dns/plan/test/doc files.
  QA scenarios: happy — gate logs, Evidence .omo/evidence/dns-tester/task-28-gates.log; failure — n/a (gate failure = todo not done).
  Commit: Y | chore(final): dns tester gates green

## Final verification wave
> Runs in parallel after ALL todos. ALL must APPROVE. Surface results and wait for the user's explicit okay before declaring complete.
- [ ] F1. Plan compliance audit
  Steps (agent-executed): (a) read `.omo/plans/dns-tester.md` in full; (b) run `git log --oneline -30` and `git status` — verify one commit per todo 1-28 with the declared commit messages; (c) for EVERY todo, open its evidence file under `.omo/evidence/dns-tester/` and confirm the acceptance command output shows success — missing or empty evidence = REJECT; (d) verify every "Must NOT have" bullet against the diff: `git diff --stat` plus targeted greps (no modifications in `src-tauri/src/engine/dns.rs` or `src-tauri/src/scan/rdns.rs`, no per-query samples table in `src-tauri/migrations/`, no hickory-client in `src-tauri/Cargo.toml`, no DNSCrypt references).
  Expected result: APPROVE only if every todo has passing evidence and every Must-NOT grep is clean. Evidence: .omo/evidence/dns-tester/f1-compliance.md (full findings table).
- [ ] F2. Code quality review
  Steps (agent-executed): (a) `cargo clippy --all-targets -- -D warnings` exits 0; (b) `Select-String -Path src-tauri\src\dns\*.rs -Pattern "unwrap\(\)|expect\(|#\[allow"` — hits allowed only inside `#[cfg(test)]` modules, every other hit = finding; (c) `npm run build` exits 0 (tsc strict, no `@ts-ignore`/`as any` in new frontend files — grep `src/` for both); (d) review each new file for dead code and oversized modules (>600 lines = finding to justify).
  Expected result: APPROVE only with zero unaddressed findings. Evidence: .omo/evidence/dns-tester/f2-clippy.log + .omo/evidence/dns-tester/f2-quality.md.
- [ ] F3. Real manual QA
  Steps (agent-executed, no human in the loop): (a) `npx playwright test e2e/dns-tester.spec.ts` exits 0 against the IPC mock — covers nav, lookup rows, benchmark completion, load-guard, AXFR token gate, history; (b) `cargo test -- --ignored dns_real` on the dev machine against real resolvers — record full output; if the network blocks it, record the exact failure and mark CONDITIONAL with justification; (c) `cargo tauri build` exits 0 producing an installer binary (proves the real app surface compiles and bundles end-to-end).
  Expected result: APPROVE when (a) and (c) pass and (b) passes or is justified-conditional. Evidence: .omo/evidence/dns-tester/f3-playwright.log, f3-real-network.log, f3-build.log.
- [ ] F4. Scope fidelity
  Steps (agent-executed): (a) build a table mapping every Scope "Must have" bullet to the named test(s) or Playwright assertion(s) proving it — any bullet without proof = REJECT; (b) map every "Must NOT have" bullet to its proving grep/test; (c) `git diff --stat <base>..HEAD` — list every changed file, confirm each is declared in some todo; any undeclared file must be reverted or justified in the evidence file.
  Expected result: APPROVE only when both mappings are complete and the diff is fully accounted for. Evidence: .omo/evidence/dns-tester/f4-scope.md.

## Commit strategy
One conventional commit per todo (repo style: `feat(lib):`, `feat(ui):`, `test(e2e):`, `docs(…):`, `chore(…)` — see `git log`: c2dd256, 4b36fe6). Commit only after that todo's acceptance criteria pass; never batch multiple todos into one commit; never commit plan/evidence files with feature code (`.omo/` stays out of feature commits unless the repo already tracks it — check `.gitignore` first).

## Success criteria
- `cargo clippy --all-targets -- -D warnings`, `cargo test`, `npm run build`, `npx vitest run`, `npx playwright test` all exit 0 on the Windows dev machine.
- A "DNS Tester" nav entry renders three tabs; a lookup against a mock resolver streams per-record results live; an Everyday benchmark across two preset resolvers produces median/p95/timeout-rate columns and a reliability score; a Diagnostics run renders the checklist and persists to history; cancel mid-benchmark leaves a 'cancelled' run row.
- Load profile refuses preset resolvers and out-of-cap values; AXFR refuses to run without the exact zone-name token.
- p90/p95/p99 are never shown for cells with <1000 samples; no averaged-averages anywhere in the metrics path; benchmark progress never streams per-sample over Tauri Channels.
- No changes to existing Ping/Traceroute/Speed/Scanner behavior; no files outside the feature scope modified.
