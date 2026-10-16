# Path MTU Discovery — Implementation Plan

## Goal

A new "MTU Discovery" tool in the Measure group that finds the path MTU to a
target by sending ICMP echo probes with the Don't-Fragment flag: coarse
bracketing steps first, then binary search to 1-byte precision. A TCP
PLPMTUD-style probe (RFC 4821) is included as a fallback for paths where ICMP
is filtered.

## Decisions (confirmed with user)

- **IPv4 only**, probing ceiling 9000 (jumbo Ethernet). IPv6 is out of scope
  (ICMPv6 has no DF flag; PTB handling is different — possible future work).
- **TCP PLPMTUD fallback is in scope for v1** (user explicitly chose this over
  deferring).
- Result reported as IP MTU bytes (payload + 20 IP + 8 ICMP header for ICMP
  probes).

## Key codebase facts (verified)

- `src-tauri/src/engine/`: enum-dispatched engines (`PingEngine`), no traits.
  - `surge.rs`: `SurgePinger::new(addr, scope_id, payload_size, _dont_fragment)`
    — DF param exists but is ignored; **surge-ping 0.9 cannot set DF** (Config
    only exposes kind/ttl/interface/bind/fib — verified against upstream docs).
    The MTU feature will NOT use surge.
  - `winicmp.rs` (Windows): `IcmpSendEcho` currently passes `None` for
    `RequestOptions`. Passing an `IP_OPTION_INFORMATION` with
    `FlagsOffset = IP_FLAG_DF (0x2)` sets DF. Oversized-DF sends surface as
    reply status `IP_PACKET_TOO_BIG (11009)`. Unprivileged, locale-independent,
    no process spawn — this is the Windows MTU engine.
  - `osping.rs` (POSIX): already implements DF argv (`-M do` Linux, `-D`
    macOS), `LC_ALL=C` English-pinned parsing, fake-ping test fixtures. Linux
    iputils prints `Frag needed and DF set (mtu = N)` — a free RFC 1191
    next-hop MTU hint. macOS prints `ping: sendto: Message too long` (no
    hint). This is the POSIX MTU engine.
  - Engines fix payload size at construction; the MTU probe engine must accept
    size per probe call (payload `Vec` built per call is fine — probes are
    infrequent).
- `trace/manager.rs` is the template for a finite-run feature:
  `Arc<Database>` + factory injection + `Mutex<Option<Active>>` + `watch`
  stop channel + `JoinHandle`, `AlreadyRunning` error, DTOs in `types.rs`.
- `lib.rs`: commands take `tauri::ipc::Channel<T>` for streaming; managers are
  `app.manage()`d in `setup()`; commands listed in `generate_handler!`.
- `db.rs`: SQLite (sqlx), migrations embedded; `NewTrace`/hop tables are the
  history pattern to copy.
- Frontend: hash routing in `src/routing.ts` (NAV_ITEMS/NAV_GROUPS/Route
  union), views in `src/views/*.tsx` + CSS modules, shared `vk-*` primitives
  in `src/components/ui/ui.tsx`, design tokens in DESIGN.md. Playwright e2e
  uses `e2e/mock-ipc.ts` to mock Tauri IPC.
- `src-tauri/src/scan/interfaces.rs` exists — check whether it exposes local
  interface MTU; if yes, use it to cap the ceiling and warn when the local
  NIC MTU < requested ceiling.

## Architecture

```
src-tauri/src/mtu/
  mod.rs        — re-exports
  types.rs      — DTOs: ProbeOutcome, MtuProbeEvent, MtuStatusEvent,
                  StartMtuDto, StoppedMtuDto, MtuRunSummaryDto, LoadedMtuRunDto,
                  MtuError (AlreadyRunning/NoActive/Resolve/InvalidInput/...)
  search.rs     — PURE search controller (no I/O): the state machine below
  engine.rs     — probe engines: ProbeEngine enum (WinIcmpDf / OsPingDf / Mock)
                  with `async fn probe(&mut self, payload_size) -> ProbeOutcome`
  tcp_probe.rs  — TCP PLPMTUD engine (RFC 4821 style)
  manager.rs    — MtuManager: clone of TraceManager pattern
  runtime.rs    — run loop: drives controller, streams events, persists
```

### ProbeOutcome

```rust
enum ProbeOutcome {
    Ok { rtt: Duration },
    TooBig { hint_mtu: Option<u32> }, // ICMP Frag Needed; hint = RFC 1191 MTU
    Timeout,                          // no reply — NOT treated as TooBig
    Error(String),
}
```

### Search controller (search.rs, pure + unit-tested)

Config: `floor = 576`, `ceiling = 9000`, `confirm_probes = 2` per size.

1. **Baseline**: 2 probes at payload 56. If both fail → run ends
   `Unreachable`.
2. **Bracket**: probe MTU 1500 (payload 1472). If OK, double 3000 → 6000 →
   9000 (payload = MTU − 28) until first failure or ceiling. Produces
   `[low_ok, high_fail]`. If 1500 itself fails, bracket is `[floor, 1500]`.
3. **Refine**: binary search payload between low_ok+1..high_fail−1 until
   adjacent — 1-byte precision.
4. **Hint shortcut**: any `TooBig { hint_mtu: Some(m) }` clamps the high
   bound to `m` and probes `m − 28` next (skips most of the binary search on
   RFC 1191-compliant paths).
5. **Loss discipline**: `Timeout` at a size retries up to 2 times; persistent
   timeout after a successful baseline is reported as
   `Inconclusive { largest_ok }` with an "ICMP filtering suspected" reason —
   never silently treated as TooBig (that would under-report the MTU).
6. Result kinds: `Exact(mtu)`, `LowerBound(largest_ok, reason)`, `Unreachable`,
   `Failed(engine_error)`.

Worst case ≈ 2 + 4 + ⌈log2(9000−576)⌉ ≈ 20 probes ≈ 20–40 s at 1–2 s per
probe; hint shortcut typically cuts this to < 10.

### TCP PLPMTUD engine (tcp_probe.rs)

- Connect `target:port` (default 443, user-configurable).
- Linux: `setsockopt(IP_MTU_DISCOVER, IP_PMTUDISC_PROBE)`; Windows:
  `socket2` `IP_DONTFRAGMENT` (socket2 0.6 already a dependency).
- Binary-search padded HTTP `HEAD` request sizes; `EMSGSIZE` /
  `WSAEMSGSIZE (10040)` = TooBig. On Linux also read `getsockopt(IP_MTU)`
  after failures as kernel-cached PMTU evidence (shown as a hint).
- macOS: no supported way to set DF on TCP → method reports
  `Unavailable("TCP probing not supported on macOS")`; UI hides the option.
- Result kind is always labeled `TcpEstimate` — it measures the path to a
  TCP peer and can be affected by proxies/MSS clamping; the UI states this.

### Manager / DB / commands

- `MtuManager` mirrors `TraceManager`: factory injection (real engines in prod,
  scripted mock in tests), `AlreadyRunning`, watch-stop, `JoinHandle`.
- DB migration adds:
  - `mtu_runs(id, target_input, resolved_ip, method, floor, ceiling,
    result_kind, result_mtu, probe_count, started_at, ended_at)`
  - `mtu_probes(run_id, seq, payload_size, mtu_size, outcome, rtt_ms, hint_mtu)`
- Tauri commands (register in `generate_handler!`, manage manager in `setup()`):
  - `start_mtu_probe(target, method, ceiling, port, on_event: Channel<MtuProbeEvent>, on_status: Channel<MtuStatusEvent>) -> StartMtuDto`
  - `stop_mtu_probe() -> StoppedMtuDto`
  - `list_mtu_runs() -> Vec<MtuRunSummaryDto>`
  - `load_mtu_run(id) -> LoadedMtuRunDto`
  - `delete_mtu_run(id)`

## Frontend

- `src/routing.ts`: add `"mtu"` to `Route`, `NAV_ITEMS` ("MTU Discovery",
  `#/mtu`), Measure group `Ctrl 4` (Discover/MikroTik shortcuts shift to
  5/6/7 — update `routing.test.ts`).
- `src/views/MtuView.tsx` + `MtuView.module.css`:
  - Header: target input, method `Segmented` (ICMP / TCP), ceiling select
    (1500/9000), TCP port input (visible when method=TCP), Start/Stop.
  - Live probe table: seq, MTU size, payload, outcome badge
    (ok/too-big/timeout/error with vk-badge colors), RTT, hint.
  - Result card: big mono MTU value, result-kind badge, probe count +
    duration, caveat text for `LowerBound`/`TcpEstimate`.
  - History table (target, method, result, date) with load/delete, mirroring
    `TracerouteView`.
- `e2e/mock-ipc.ts`: mock the five mtu commands; new `e2e/mtu.spec.ts`.

## Milestones

1. **M1 — engine + controller**: `mtu/types.rs`, `mtu/search.rs`,
   `mtu/engine.rs` (winicmp DF variant + osping DF reuse). Unit tests:
   controller convergence against a simulated link (hidden MTU → assert exact
   result in ≤ 22 probes), hint shortcut, floor/ceiling edges, inconclusive
   path; osping fake-ping fixtures for `Frag needed and DF set (mtu = N)` and
   `Message too long`.
2. **M2 — manager + DB + commands**: migration, `MtuManager`, runtime,
   lib.rs registration, `src-tauri/tests/mtu.rs` with scripted mock engine
   (pattern: `tests/trace.rs`).
3. **M3 — TCP PLPMTUD**: `tcp_probe.rs` + fallback wiring + unit tests with a
   loopback TCP server fixture.
4. **M4 — frontend**: routing, `MtuView`, mock-ipc, `mtu.spec.ts` Playwright
   e2e; verify against DESIGN.md tokens (no raw hex, `vk-*` classes).
5. **M5 — evidence + docs**: `--ignored` real-adapter e2e (e.g. probe a LAN
   gateway and a known-1500 internet host), README section, debug journal.

## Risks / open items

- **Windows DF spike (do in M1 first)**: verify `IcmpSendEcho` with
  `FlagsOffset = IP_FLAG_DF` returns `IP_PACKET_TOO_BIG (11009)` for oversized
  sends and whether router PTB messages surface through ICMP.dll with the
  hint MTU. Evidence to `.omo/evidence/`. Fallback if DF is unreliable via
  IPHlpAPI: socket2 raw socket with `set_dont_fragment(true)` (admin on
  Windows).
- **Interface MTU cap**: check `scan/interfaces.rs` for local MTU; if absent,
  probe ceiling silently clamped by the OS is acceptable (send fails locally →
  treated as TooBig, which is still correct for the *path*), but a UI note is
  better.
- **surge.rs `_dont_fragment`**: leave as-is; add a comment noting DF is
  honored only in the mtu engines, not the continuous-ping surge path.
- **Black-hole PTB filtering**: binary search alone under-reports; mitigated
  by `LowerBound`/`Inconclusive` result kinds and the TCP fallback — do NOT
  paper over it by treating timeouts as TooBig.
