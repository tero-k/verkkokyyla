# Local Network Scanner (LAN device discovery with OUI vendor lookup)

**Date**: 2026-08-19
**Status**: Approved decisions, ready for execution
**Pre-planning**: Metis analysis 2026-08-18 (session `ses_fea3df2f2ffeqLRdZJGmOkY1PN`)

## Goal

Add a fourth tool to verkkokyyla: a **local network scanner** that discovers devices on the
LAN (IP address + MAC address), resolves each MAC's OUI prefix to a manufacturer name using
a **bundled local database** (no per-device live lookups), and resolves hostnames via
reverse DNS. Results stream live to the UI and persist in SQLite like ping/trace sessions.

## Locked Decisions (user-approved 2026-08-19)

| # | Fork | Decision |
|---|------|----------|
| 1 | Hostnames | **Include reverse DNS** (existing `dns-lookup` crate); extra scan time accepted |
| 2 | Persistence | **Persist** scans in SQLite (`scans` + `scan_hosts` tables), history view like traces |
| 3 | OUI updates | **Bundle-and-forget**: Wireshark `manuf` file shipped as a Tauri resource |
| 4 | IP family | **IPv4 only**; IPv6/NDP explicitly deferred |
| 5 | Interface scope | **User-selectable interface**, defaulting to the primary one (interface with default gateway); CIDR override allowed, clamped to /22 |
| 6 | Discovery | **Ping sweep primary + TCP connect fallback** for ICMP-blocked hosts; TCP fallback is a **user-toggleable option (default ON)** |

Discovery method detail: ping sweep every host in the subnet (reuse `PingEngine`), then read
the OS neighbor/ARP table **after the sweep completes** (Windows: `GetIpNetTable2` via the
already-enabled `Win32_NetworkManagement_IpHelper` feature; macOS/Linux: parse `arp -a` /
`/proc/net/arp`). When TCP fallback is enabled, hosts that did not answer ping are also
TCP-connect-probed on ports 80/443/445/22 (any successful connect or RST still populates ARP).
**No raw sockets, no pcap, no admin requirement** beyond what ping already needs.

## Architecture (mirrors existing patterns)

### Backend (Rust, `src-tauri/src/`)

New module `src-tauri/src/scan/` with:

1. **`scan/mod.rs`** — module root, DTOs (`ScanSummaryDto`, `ScanHostDto`, `StartScanDto`,
   `ScanEvent`, `ScanStatusEvent`, `InterfaceDto`), `ScanError` (thiserror, serde-serializable
   like `TraceError`).
2. **`scan/manager.rs`** — `ScanManager` patterned after `TraceManager`
   (`src-tauri/src/trace/manager.rs:13`) and `SessionManager` (`src-tauri/src/session.rs:375`):
   owns `Arc<Database>`, at most one active scan, `watch::channel` stop signal,
   `mpsc` producer/consumer split with batch flush (mirror `consume_probes`,
   `src-tauri/src/session.rs:640`), Channel-streamed events:
   - `ScanEvent::Host { ip, mac, vendor, hostname }` per discovered host
   - `ScanStatusEvent::Progress { done, total }`, `EngineSelected`-style info, `ScanStopped`
3. **`scan/discovery.rs`** — the sweep:
   - Expand CIDR to host list (clamp to /22 = 1022 hosts max, reject larger with a clear error).
   - Ping each host via the existing `PingEngine` factory
     (`src-tauri/src/engine/mod.rs:112`, same privilege-probe fallback chain as ping sessions),
     bounded by `tokio::sync::Semaphore` (128 concurrent), per-host timeout ~500 ms.
   - If TCP fallback enabled: for non-responders, `tokio::net::TcpStream::connect` with short
     timeout (~300 ms) on ports [80, 443, 445, 22], same semaphore.
   - **After** all probes finish, read the neighbor table once (see `scan/arp.rs`), then
     optionally re-read after a short delay (500 ms) to catch late ARP entries.
4. **`scan/arp.rs`** — OS neighbor-table reader behind `#[cfg]`:
   - Windows: `GetIpNetTable2` (IpHelper, feature already enabled in Cargo.toml).
   - macOS/Linux: parse `arp -a` output, fallback `/proc/net/arp` on Linux.
   - Normalize MACs to `AA:BB:CC:DD:EE:FF`; skip incomplete entries
     (`00-00-00-00-00-00`, broadcast, own host handled by marking, not skipping).
5. **`scan/interfaces.rs`** — interface enumeration:
   - Windows: `GetAdaptersAddresses` (IpHelper). Unix: `getifaddrs` via `libc` or `if-addrs` crate.
   - Filter: `OperStatus == Up`, non-loopback, exclude tunnel/virtual IfTypes
     (VPN/Tailscale/Hyper-V/WSL/Docker), IPv4 address present.
   - Mark the interface holding the default gateway as `is_primary`.
   - Return `InterfaceDto { name, description, ipv4, prefix_len, is_primary }`.
6. **`scan/oui.rs`** — OUI vendor lookup:
   - Bundled `manuf` file (Wireshark, ~2 MB) parsed once into a
     `Vec<(prefix_bits, prefix_bytes, vendor)>` sorted for **longest-prefix match**
     (manuf contains 24/28/36-bit prefixes — naive 3-byte match is wrong).
   - Locally-administered/randomized MAC detection: if bit 1 of the first octet is set,
     return `"Randomized / locally administered"` **before** vendor lookup.
   - Loaded lazily via `OnceLock`; testable with an in-memory fixture slice.
7. **`scan/rdns.rs`** — reverse DNS via `dns_lookup::lookup_addr` in `spawn_blocking`
   (pattern already in `src-tauri/src/lib.rs:55`), bounded concurrency (same semaphore),
   best-effort: failure → `None`, never blocks scan completion.

### Persistence (`src-tauri/src/db.rs` + migration)

New migration `src-tauri/migrations/0004_scan.sql`:

```sql
CREATE TABLE scans (
    id INTEGER PRIMARY KEY,
    interface_name TEXT NOT NULL,
    cidr TEXT NOT NULL,
    tcp_fallback INTEGER NOT NULL DEFAULT 1,
    started_at TEXT NOT NULL,
    ended_at TEXT,
    status TEXT NOT NULL DEFAULT 'running' CHECK(status IN ('running','completed','cancelled','error')),
    host_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE scan_hosts (
    id INTEGER PRIMARY KEY,
    scan_id INTEGER NOT NULL REFERENCES scans(id) ON DELETE CASCADE,
    ip TEXT NOT NULL,
    mac TEXT,
    vendor TEXT,
    hostname TEXT,
    found_by TEXT NOT NULL CHECK(found_by IN ('ping','tcp','arp')),
    at TEXT NOT NULL
);

CREATE INDEX idx_scan_hosts_scan ON scan_hosts(scan_id, ip);
```

Add `Database` methods: `create_scan`, `insert_scan_host` (batch), `finish_scan`,
`list_scans`, `load_scan`, `delete_scan` — mirroring the existing trace methods
(`src-tauri/src/db.rs:212`).

### Tauri commands (`src-tauri/src/lib.rs`)

Register `ScanManager` via `app.manage(...)` (line ~218) and add commands following the
existing `start_trace`/`stop_trace`/`list_traces`/`load_trace`/`delete_trace` shapes:

- `list_interfaces() -> Vec<InterfaceDto>`
- `start_scan(interface_name, cidr, tcp_fallback, on_event: Channel<ScanEvent>, on_status: Channel<ScanStatusEvent>) -> StartScanDto`
- `stop_scan() -> StoppedScanDto`
- `list_scans() -> Vec<ScanSummaryDto>`
- `load_scan(id) -> LoadedScanDto`
- `delete_scan(id)`

### Resource bundling

- Place `manuf` at `src-tauri/resources/manuf` with a `manuf.SHA256` + source-URL comment
  file recording dataset version.
- Add to `tauri.conf.json` bundle resources; resolve at runtime via
  `app.path().resource_dir()`. In dev/tests, fall back to the repo-relative path.

### Frontend (React + TS, `src/`)

1. **`src/routing.ts`** — add `"lan-scan"` route + nav item `"Network scanner"` (hash
   `#/lan-scan`).
2. **`src/hooks/useLanScan.ts`** — mirror `useTraceroute.ts`: start/stop, live host list,
   progress, interface list loading, scan history, open/delete saved scans.
3. **`src/views/LanScanView.tsx`** (+ `.module.css`) — mirror `TracerouteView.tsx`:
   - Controls: interface dropdown (primary pre-selected), CIDR input (default from selected
     interface's /24), TCP-fallback checkbox (default checked), Start/Stop, progress bar.
   - Live results table: IP, MAC, Vendor, Hostname, Found-by. Cap visible rows at 500
     (existing idiom).
4. **`src/components/ScanSessionPanel.tsx`** — scan history list (open/delete), patterned
   after `TraceSessionPanel.tsx` (no compare feature — out of scope).
5. **`src/lib/types.ts`** — DTO mirrors of the Rust types.

## Execution Waves

- **Wave 1 (parallel)**: (a) migration `0004_scan.sql` + `Database` scan methods + unit tests;
  (b) `scan/oui.rs` + fixture tests (24/28/36-bit longest-match, randomized-MAC);
  (c) `scan/interfaces.rs` + `list_interfaces` command.
- **Wave 2 (depends on 1a)**: `scan/arp.rs`, `scan/discovery.rs`, `scan/rdns.rs`,
  `scan/manager.rs` + commands in `lib.rs`.
- **Wave 3 (depends on 2)**: frontend hook, view, panel, routing, types.
- **Wave 4**: tests + gates (below).

## Acceptance Criteria (agent-executable)

1. `cargo test` passes, including new unit tests:
   - OUI longest-prefix match with a fixture slice incl. 28/36-bit entries and a
     locally-administered MAC (`02:...`) returning the randomized label.
   - CIDR expansion: /24 → 254 hosts, /22 → 1022, /21 rejected with error.
   - ARP parsing: `arp -a` sample text and `/proc/net/arp` sample parsed; incomplete
     entries skipped; MAC normalization from `xx-xx-xx` and `xx:xx:xx`.
   - DB round-trip: create scan, insert hosts, finish, list, load, delete (cascade).
   - Discovery tested behind a `#[cfg(test)]` seam (fake ARP table + fake prober), no real
     network in unit tests.
2. `cargo clippy --all-targets -- -D warnings` clean.
3. `npx vitest run` passes (hook logic tests where practical).
4. Playwright E2E with Tauri mock: open `#/lan-scan`, mock interfaces + start scan, see
   streamed host rows and progress, stop, history entry appears, reopen saved scan.
5. `cargo tauri build` succeeds with `resources/manuf` bundled.
6. Manual smoke (documented, not automated): real scan of primary /24 on Windows finds the
   gateway with a non-empty vendor, completes under ~15 s with progress updates, TCP
   fallback toggle visible and respected.

## Must NOT

- No raw sockets / pcap / packet injection anywhere.
- No IPv6, NDP, mDNS, SSDP, NetBIOS, port-scan feature surface, or scan-compare.
- No live OUI lookup per device; no "update OUI DB" button (bundle-and-forget).
- No type-error suppression (`as any`, `@ts-ignore`), no `unwrap()` outside tests,
  no empty catch blocks.
- Do not refactor existing ping/trace/download code; reuse via public APIs only.

## Pitfalls (from Metis analysis — guard in review)

- ARP timing: read the neighbor table **after** the sweep, never incrementally; entries
  evict in 15–45 s, so don't re-read lazily long after.
- Interface pollution: VPN/virtual adapters must be filtered (OperStatus/IfType/gateway).
- Concurrency: semaphore-bounded (128) or /22 exhausts fds/threads.
- OUI prefix-length variance: longest-prefix match or vendors come out wrong.
- Randomized MACs: phones randomize; show the randomized label, not a bogus vendor.
