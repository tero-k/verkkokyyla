# Plan: port & service detection for the LAN scanner

## Objective
Extend the existing local-network scanner so that, when enabled, every discovered host is probed on a curated set of common TCP ports. Open ports are labeled with their probable service name and displayed in a new column. A persistent warning and a first-time consent dialog make the user aware that active TCP scanning can trigger firewall/IDS/SIEM alerts.

## Context
The scanner currently does a ping sweep, optional TCP fallback on four ports (80, 443, 445, 22), ARP/neighbour-table enrichment, hostname RDNS, and OUI vendor lookup. Hosts are emitted from `ScanRunner::production`, batched by `consume_hosts`, and persisted in `scan_hosts`. The frontend renders the results in `LanScanView` + `LanScanTable`.

User-chosen decisions from planning:
- Curated 25-port set (nmap top-20 plus 515, 554, 631, 8443) for LAN relevance.
- Connect-only probes; no banner grabbing, no protocol payloads, no version detection.
- Persistent warning banner + first-time consent dialog shown when the user enables port scanning.
- “Scan common ports” toggle, default OFF (opt-in).

## Decisions
1. **Port set** — `COMMON_PORTS` constant in `src-tauri/src/scan/ports.rs` containing 25 `(u16, &'static str)` entries ordered by nmap-services frequency, augmented with printer/camera/alt-HTTPS LAN ports.
2. **Probe depth** — TCP `connect()` only. Result is `(port, probable_service)` for open connections; closed/refused/timeout ports are dropped.
3. **Enrichment point** — After `DiscoveryServices::discover` returns, in `ScanRunner::production` (or an equivalent place) loop over discovered hosts and run the port scan before sending the completed host through the mpsc channel. This preserves one coherent `ScanEvent::Host` DTO.
4. **Concurrency/rate defaults** — Global concurrency 32, per-host sequential probing, 500 ms connect timeout, token-bucket pacing at ~10 connection attempts/second. These constants live in `ports.rs` and are configurable by changing code, not by UI.
5. **Persistence** — New migration `0005_scan_ports.sql` adds `open_ports` as JSON TEXT on `scan_hosts`; backend DTOs and DB helpers serialize/deserialize a `Vec<OpenPort>`.
6. **Frontend warning** — Persistent `Alert`/`Callout` banner above scan controls in `LanScanView`; first-time consent stored in `localStorage` key `verkkokyyla-port-scan-consent-v1`, shown when the user toggles port scanning ON for the first time.
7. **Scope boundaries** — No UDP/SYN/raw scans; no IDS evasion; no custom port UI; no per-scan rate tuning.

## Scope IN
- `src-tauri/src/scan/ports.rs` — new module with port list, probable-service map, async `probe_ports` function, and fake-probe seam.
- `src-tauri/src/scan/discovery.rs` — extend `DiscoveryServices` with a port-probe service.
- `src-tauri/src/scan/manager.rs` — call port probe after discovery, before host emission; wire result into `ScanEvent::Host`.
- `src-tauri/src/scan/mod.rs` — add `open_ports: Vec<OpenPort>` to `ScanHostDto`, `ScanEvent::Host`, and migration/types.
- `src-tauri/migrations/0005_scan_ports.sql` — add `open_ports` column, default `[]`/`null`.
- `src-tauri/src/db.rs` — `ScanHostRow`, insert/load/update SQL for the new column.
- `src-tauri/src/scan_commands.rs` — accept `ports_enabled: bool` in `StartScanRequest` and thread it through.
- `src/lib/types.ts` — add `OpenPort`, extend `ScanHostDto` / frontend event types.
- `src/hooks/useLanScan.ts` — add `portsEnabled` state, pass it to `startScan`, merge ports in host events.
- `src/views/LanScanView.tsx` — add “Scan common ports” toggle, persistent warning banner, consent dialog.
- `src/components/LanScanTable.tsx` — add Ports column rendering a concise list.
- `src/hooks/useLanScan.test.ts` — test port event merging and toggle behavior.
- `src-tauri/tests/scan_foundations.rs` — test DB round-trip with `open_ports` and migration shape.
- `e2e/mock-ipc.ts` + `e2e/lan-scan.spec.ts` — extend mock host/events with ports and assert column/banner rendering.

## Scope OUT (must NOT have)
- No banner grabbing, protocol payloads, TLS negotiation, or version detection.
- No UDP scanning, no SYN/stealth/raw-socket scans.
- No custom per-scan port input or rate-limit UI.
- No scanning of undiscovered hosts (port probes only for hosts already found).
- No IDS evasion, throttling-to-silence, or randomized delays beyond the token bucket.
- No scheduling, repeated background scans, or network-wide alert suppression.

## Constraints
- Port probes must be cancellable and must not outlive the scan session; reuse the existing scan-stop signal.
- Only open ports are stored; do not persist closed/filtered triage.
- Service names are “probable” only — never displayed as certainty.
- The consent dialog must show: target range, host count, port count, estimated total probe count, and pacing summary.
- Default toggle state is OFF; the user must opt in.
- Rate defaults must keep a /24 × 25-port scan from looking like a single burst (token bucket + bounded concurrency).
- No `as any`, `@ts-ignore`, empty catch blocks, or deleted failing tests.

## Approach / Architecture
1. Add a `ports.rs` module with:
   - `COMMON_PORTS: [(u16, &'static str); 25]`.
   - `probable_service(port: u16) -> &'static str`.
   - `PortProber` trait/closure type returning `Vec<OpenPort>` for one host.
   - `probe_ports(host, prober, token_bucket, semaphore, cancel) -> Vec<OpenPort>` helper.
2. Extend `DiscoveryServices`:
   - Add `ports: PortProber` field.
   - `DiscoveryServices::production` instantiates a real TCP connector.
   - Tests inject a fake prober returning deterministic ports.
3. In `ScanRunner::production`:
   - After `discovery.discover(...).await`, if `ports_enabled` is true, for each `DiscoveredHost` call the port prober.
   - Attach `open_ports` to the host before `tx.send(host)`.
4. Extend the event/persistence pipeline:
   - Migration adds `open_ports TEXT` (JSON array).
   - `ScanHostDto` / `ScanHostRow` carry `Vec<OpenPort>`.
   - DB insert/load round-trip JSON with serde.
5. Frontend:
   - `useLanScan` stores `portsEnabled` (default false) and passes it to `startScan`.
   - `LanScanView` shows warning banner under controls and a `PortScanConsentDialog` when toggle flips to true for the first time.
   - `LanScanTable` renders ports as e.g. `22 ssh, 80 http, 443 https` with tooltip for full list.
6. Tests mirror each layer with fake probes and mocked events.

## Execution waves
### Wave 1: backend ports module + discovery seam
- Create `src-tauri/src/scan/ports.rs` with constants, probable-service map, and probe function.
- Add `PortProber` to `DiscoveryServices` and `discover_with` signature; update tests.
- Wire production prober to `std::net::TcpStream::connect` with timeout via async I/O (tokio `timeout` + `TcpStream::connect`).
- Add unit tests for probe behavior (open, closed, timeout, cancellation).

### Wave 2: manager + persistence + commands
- Update `DiscoveredHost` / `ScanHostDto` / `ScanEvent::Host` / `ScanHostRow` with `open_ports`.
- Add migration `0005_scan_ports.sql`.
- Update DB insert/load helpers.
- In `ScanRunner::production`, conditionally run port scan and attach results.
- Add `ports_enabled` to `StartScanRequest`; update `scan_commands.rs`.
- Add backend tests (migration round-trip, manager event with ports).

### Wave 3: frontend controls, warning, and table
- Extend `src/lib/types.ts` with `OpenPort`.
- Add `portsEnabled` state to `useLanScan.ts` and include it in `startScan`.
- Implement consent dialog component and `localStorage` persistence hook.
- Add persistent warning banner and toggle to `LanScanView`.
- Add Ports column to `LanScanTable`.
- Add frontend unit tests for merge logic and toggle state.

### Wave 4: E2E + verification gates
- Update `e2e/mock-ipc.ts` host fixtures and events to include ports.
- Add E2E assertions for banner presence, consent dialog, toggle, and Ports column.
- Run the standard gates:
  - `cargo test --lib`
  - `cargo test --test scan_foundations`
  - `cargo clippy --all-targets -- -D warnings`
  - `npx vitest run`
  - `npx playwright test`
  - `cargo tauri build`

## Verification
- **Backend**: unit tests for port probe (fake prober seam), integration tests for migration and DB round-trip, manager event verification.
- **Frontend**: vitest for hook merge and consent state; Playwright for banner, dialog, toggle, and rendered Ports column.
- **Build**: all existing and new Rust/TS tests pass; clippy clean; Tauri production build succeeds.
- **Manual QA**: run a scan on a trusted local network with port scanning enabled; verify that only a handful of open ports appear per host and that toggling off returns to device-only scan.

## Risks
1. **IDS/SIEM alerts** — Mitigated by connect-only probes, low concurrency, and token-bucket pacing; not eliminated. The consent dialog and persistent banner explicitly disclose the risk.
2. **Long scan duration** — A /24 × 25-port scan at ~10 attempts/s is ~10–11 minutes worst case; bounded concurrency keeps memory flat but the UI must show progress and remain cancellable.
3. **Misidentification** — Service names are port-number guesses; UI labels them “probable”.
4. **Firewall blocking outbound connects** — Some hosts/ports will time out; that is expected and handled.
5. **Cross-platform TCP connect behavior** — Tested via unit tests and manual QA on Windows/Linux/macOS as needed.

## Rollback
- Revert the migration in dev by deleting `src-tauri/migrations/0005_scan_ports.sql` and the DB file; for released builds, ship a follow-up migration if needed.
- Feature is gated by a frontend toggle defaulting OFF; users can disable without code changes.

## Open questions
- (none — user confirmed the four planning forks)
