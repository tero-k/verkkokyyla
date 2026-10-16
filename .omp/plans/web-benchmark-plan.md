# Web Benchmark — work plan

## Goal

Rename the "Web page speed test" feature to **Web Benchmark** and expand the existing
HTTP download benchmark into a network-performance diagnostics tool: per-phase timings,
repeated runs with percentile statistics, cold/warm connection modes, protocol
comparison (HTTP/1.1 vs HTTP/2), optional concurrency test, structured errors, and a
comparison UI. Existing "Single file" and "Full page" modes keep working unchanged.

## Locked decisions

- **Name:** "Web Benchmark" (nav label + view heading).
- **Phase timings:** optional companion **connection probe** (manual DNS + TCP +
  tokio-rustls TLS handshake against the same host) to surface real DNS/TCP/TLS/TLS-
  version numbers. Clearly labeled as a probe; never presented as the reqwest
  request's own connection. Unavailable fields stay `null`, never fabricated.
- **HTTP/3:** not available in the current stack (reqwest 0.12, http3 only behind an
  unstable cfg). Selecting HTTP/3 returns an explicit capability result
  (`errorType: "http3-unsupported"`), never a silent fallback.
- **No architectural rewrite:** reqwest stays the HTTP client; new code lives in new
  modules; benchmark logic stays separate from UI.

## Current state (verified by inspection)

- Backend: `src-tauri/src/download.rs` (single), `page_speed.rs` (full page),
  `http_client.rs::build_client` (shared reqwest builder), `download_manager.rs` +
  `db.rs` (history; results stored as opaque JSON strings → richer model fits with no
  schema migration).
- reqwest 0.12 features: `rustls-tls, stream, gzip, brotli, deflate, http2`. No http3.
- Timings already use `std::time::Instant` (monotonic) — accuracy requirement met;
  keep f64 precision internally, round only in UI.
- Dev-deps already include `tokio-rustls` + `rcgen` → local TLS/h2 test servers are
  feasible without internet.

## Metric availability matrix (reqwest 0.12, verified)

| Metric | Source | Notes |
|---|---|---|
| TTFB, download, total | `Instant` around send/first-chunk/end | current approach |
| Status code, negotiated protocol | `Response::status()`, `Response::version()` | real negotiated value |
| Remote IP, IP family | `Response::remote_addr()` | exists in 0.12 |
| Redirect count, hops, final URL | `redirect::Policy::custom` via `Attempt::previous()` chain | shared counter state |
| Response size, throughput | streamed byte count | no extra buffering |
| Content-Encoding | response headers | with decode disabled; when auto-decoded reqwest strips it → document |
| Transferred vs decoded bytes | decoded = stream count; transferred only when compression off | `transferredBytes: null` when decoded |
| DNS/TCP/TLS phase, TLS version, ALPN | companion connection probe (optional toggle) | labeled as probe |
| Connection-reused flag | not exposed by reqwest → `null` | cold/warm mode is what we control |
| DNS/TLS separate timeouts | not exposed → best approximation (`connect_timeout`, `timeout`, `read_timeout`) | documented |
| IPv4/IPv6 selection | pre-resolve with family filter + `ClientBuilder::resolve_to_addrs()`; report actual from `remote_addr()` | |

## Data model (Rust, serde camelCase, mirrors TS types)

New module `src-tauri/src/bench.rs`:

- `BenchmarkRun` — one request: `runId`, `url`, `requestedProtocol`,
  `negotiatedProtocol`, `startedAt` (RFC3339), `statusCode: Option<u16>`,
  `dnsMs/connectMs/tlsMs/ttfbMs/downloadMs/totalMs: Option<f64>` (f64, unrounded),
  `responseBytes`, `decodedBytes: Option<u64>`, `transferredBytes: Option<u64>`,
  `throughputBytesPerSecond: Option<f64>`, `remoteIp: Option<String>`,
  `ipVersion: Option<"ipv4"|"ipv6">`, `tlsVersion: Option<String>`,
  `alpn: Option<String>`, `connectionReused: Option<bool>` (always null — documented),
  `connectionMode: "cold"|"warm"`, `probe: Option<ConnectionProbe>`,
  `redirectCount: u32`, `redirects: Vec<String>`, `finalUrl`,
  `contentEncoding: Option<String>`, `success: bool`, `errorType: Option<String>`,
  `errorMessage: Option<String>`.
- `ConnectionProbe` — `dnsMs`, `connectMs`, `tlsMs`, `tlsVersion`, `alpn`,
  `sessionResumed: Option<bool>` (null — rustls resumption not reliably exposed),
  `ipVersion`, `remoteIp`, `error: Option<String>`. Failures never fail the benchmark.
- `BenchmarkSummary` — per metric (totalMs, ttfbMs, downloadMs, …): `count`, `min`,
  `max`, `mean`, `p50`, `p90`, `p95`, `p99`, `stddev` as `Option<f64>`.
- `ConcurrencySummary` — `concurrency`, `totalRequests`, `successfulRequests`,
  `failedRequests`, `requestsPerSecond`, latency stats (avg/p50/p95/p99),
  `totalBytes`, `aggregateThroughputBytesPerSecond`.
- `BenchmarkResult` — `benchmarkId` (UUID-ish: timestamp+counter, no new dep),
  `config` (echo of request), `runs: Vec<BenchmarkRun>`,
  `summaries: Vec<ProtocolSummary>` (one per requested protocol), plus optional
  `concurrency: Option<ConcurrencySummary>`.
- `BenchmarkErrorKind` — string enum: `invalid-url`, `dns`, `connect-timeout`,
  `connect-refused`, `tls`, `certificate`, `http-timeout`, `protocol-negotiation`,
  `http3-unsupported`, `redirect-loop`, `read-failure`, `request`. Mapped from
  reqwest error introspection (`is_timeout()`, `is_connect()`, source chain for
  rustls cert errors); original message preserved in `errorMessage`.

New module `src-tauri/src/bench_stats.rs` (pure std, mirrors `stats.rs` style):

- `percentile(sorted: &[f64], p: f64) -> Option<f64>` — linear interpolation on
  unrounded samples (never from rounded values).
- `summarize(samples: &[f64]) -> MetricStats` — min/max/mean/stddev (population,
  Welford or two-pass on f64), p50/p90/p95/p99.
- Unit-tested with fixed vectors.

## Settings changes

`HttpSettingsDto` (and TS `HttpSettings`) gains: `readTimeoutSec: Option<u64>`
(→ `ClientBuilder::read_timeout`), `ipFamily: "auto"|"ipv4"|"ipv6"`.
Existing fields unchanged; frontend storage version bump
(`verkkokyyla-http-settings-v2`) with fallback to defaults. `HttpVersion` gains
`http3` marker used only to trigger the capability result.

New `BenchmarkConfig` passed to the benchmark command: `url`, `protocols:
Vec<HttpVersion>` (multi-select), `runs: u32` (clamp 1..=100), `connectionMode`,
`concurrency: Option<u32>` (allowed 1/5/10/25/50), `probe: bool`, plus the shared
`HttpSettingsDto`.

## Backend execution flow (`bench.rs::run_benchmark`)

1. Parse URL (reuse `download::parse_url`); invalid → structured error result.
2. Per requested protocol:
   - `http3` → capability run result, skip.
   - Optional probe once per protocol/host (probe is informational, not per-run).
   - **Cold mode:** fresh `reqwest::Client` per run (built via extended
     `build_client`); run 1 warmup-free iteration × `runs`.
   - **Warm mode:** one shared client; first request discarded as warmup (labeled),
     then `runs` measured iterations.
   - Each run: `Instant` around send/headers/first-chunk/end; record
     `Response::version()`, `remote_addr()`, status, redirect chain (custom policy),
     sizes. Failures produce a failed `BenchmarkRun`, not an abort.
3. Optional concurrency phase: shared client (pool), `buffer_unordered(concurrency)`,
   collect per-request latencies + bytes; no fresh client per request.
4. Summaries per protocol from raw runs; comparison deltas computed in UI only when
   configs identical and successful run count ≥ 3.

IP family: when `ipv4`/`ipv6` selected, resolve host with filter (existing
`tokio::net::lookup_host` / `dns-lookup`), pin via `resolve_to_addrs()`; if no
address of that family → structured error run.

Probe (`probe.rs` inside bench module): `lookup_host` (timed) → `TcpStream::connect`
(timed) → `tokio_rustls` handshake with `rustls-platform-verifier`-style roots
(already available via rustls + webpki roots used elsewhere) — TLS version/ALPN read
from the negotiated `ClientConnection`. Skipped for plain http URLs (TLS fields null).
Hard timeout = connect timeout; errors captured into probe, never propagated.

## Tauri / IPC

- New command `run_web_benchmark(config: BenchmarkConfig) ->
  Result<BenchmarkResult, BenchmarkError>` registered in `lib.rs`. No progress
  channel initially (runs are fast); progress can be added later without breaking
  the wire format.
- History: reuse existing `save_download_speed_session` with `mode: "benchmark"` and
  the result JSON — `loadSession` parses by mode. No DB schema change.

## Frontend

- `src/lib/types.ts`: `BenchmarkConfig`, `BenchmarkRun`, `BenchmarkSummary`,
  `ConcurrencySummary`, `BenchmarkResult`; extend `HttpSettings` + `HttpVersion`.
- `src/lib/ipc.ts`: `runWebBenchmark()` wrapper.
- `src/hooks/useHttpSettings.ts`: new fields, storage v2.
- `src/hooks/useDownloadSpeedTest.ts`: add `"benchmark"` mode calling the new
  command; existing modes untouched.
- `src/views/DownloadSpeedView.tsx`: mode option "Benchmark (detailed)"; heading and
  `routing.ts` label → **"Web Benchmark"**; `routing.test.ts` updated.
- New `src/components/BenchmarkResultPanel.tsx`: metric cards (Total, TTFB, DNS,
  Connect, TLS, Download, HTTP version, size, throughput — same card CSS classes),
  per-protocol stats table (avg/p50/p95/p99/min/max), protocol comparison table with
  delta callouts ("HTTP/2 is 14% faster…" only when ≥3 successful runs and identical
  config), concurrency summary block, per-run expandable raw table. No new UI
  framework; reuse existing table/card styles. Chart optional (uplot present) — only
  if trivial.
- Settings panel: read timeout, IP family, probe toggle, runs, concurrency select,
  protocol multi-select (checkboxes).

## Testing

Rust (`bench.rs`, `bench_stats.rs`, using the `TcpListener` mock pattern from
`download.rs` tests):

- percentile/summary math (fixed vectors, incl. single-sample and empty)
- HTTP/1.1 success against local server; status/bytes/TTFB/total populated
- redirect chain counting + final URL (local redirect server)
- followRedirects=off → redirectCount 0, 3xx status preserved
- timeout (sleeping server + short request timeout) → `http-timeout`
- connect refused → `connect-refused`; DNS failure (invalid TLD host) → `dns`
- invalid URL / missing host / scheme → structured errors
- http3 requested → `http3-unsupported`, no fallback
- repeated runs: `runs=5` yields 5 raw runs + correct summary
- cold vs warm: warm shares client (pool) — assert remote addr stable and both modes
  labeled
- concurrency: 10 requests at concurrency 5 on counting server; totals, latency
  percentiles, aggregate bytes
- response sizes: ~10 KB / ~100 KB / 1 MB bodies; streamed count correct
- compression: gzip server → contentEncoding reported; compression off → sizes equal
- HTTP/2 prior-knowledge against local h2 server (tokio-rustls + rcgen) —
  `negotiatedProtocol == "h2"`; fallback behavior against h1-only server reported via
  `Response::version()`
- TLS probe against rcgen-based local server (self-signed → probe error captured,
  benchmark result unaffected); plain-http URL → probe TLS fields null

Frontend (`vitest`): settings load/save v2, comparison-delta helper (threshold
guards), formatting of `null` metrics as "—".

## Verification

`cargo test`, `npx vitest run`, `npm run build` (tsc + vite), and a manual
`cargo tauri dev` smoke run of benchmark mode against a local server. Playwright e2e
updated only where nav label/assertions reference the old name.

## Explicitly out of scope / limitations (documented in result + README notes)

- reqwest cannot expose per-request TCP/TLS phase timings, TLS version, ALPN, or
  connection-reuse flags → `null` unless the optional probe supplies its own
  (clearly labeled) values.
- HTTP/3 unsupported in this build → capability message.
- Wire bytes not observable while reqwest auto-decodes → `transferredBytes: null`
  under compression.
- Probe measures a parallel connection to the same host, not the benchmarked
  request's socket.

## Implementation order

1. `bench_stats.rs` + tests
2. `bench.rs` data model + single-request timing + tests
3. Probe + IP-family pinning + read timeout (http_client.rs extensions) + tests
4. Repeated runs + summaries
5. Cold/warm modes
6. Concurrency phase
7. IPC command + types.ts/ipc.ts
8. UI: benchmark mode, result panel, rename ("Web Benchmark")
9. Frontend tests + Playwright label updates
10. Full suite + smoke run; fix regressions

No unrelated refactors. `download.rs` / `page_speed.rs` and both existing modes stay
functionally unchanged.
