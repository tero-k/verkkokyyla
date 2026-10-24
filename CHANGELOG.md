# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **OPNsense traffic charts**: select multiple interfaces, WireGuard peers,
  and OpenVPN sessions on the Interfaces & VPN tab to graph their live
  receive/transmit bit rates. Histories are bounded to one hour at the
  normal five-second refresh, remain independent per connected firewall,
  and show unknown points rather than stale rates after failed refreshes.
  Enabled/Disabled toggles change only local graph selection, never firewall
  configuration.

### Changed

- **OPNsense firewall rules**: interface groups now use firewall-provided
  identifiers, display names, and interface order, including Floating rules.
  Added one case-insensitive search across all groups and visible rule fields,
  with matching counts and empty-state feedback.

- **Docs**: refreshed the README and in-app Help for the 0.2.0 OPNsense
  release. The README now describes the OPNsense profile workflow (no-scheme
  host, HTTPS/SSH ports, Test connection, device-strip switching) and the
  Web Benchmark, Network scanner, and DNS Toolkit feature sets in full, and
  explains that the optional startup release notice never installs updates.
  Help sections for those tools and all OPNsense tabs now match the actual
  UI. All five README screenshots were regenerated with the current
  navigation, and two OPNsense captures (firewall rules, cached firmware
  status) were added.

## [0.2.0] - 2026-10-16

### Added

- **OPNsense monitoring**: the sidebar's MikroTik group is now Devices,
  with OPNsense (Ctrl 8) alongside MikroTik. OPNsense profiles use a
  dedicated read-only API key (HTTPS GET requests only, stored as one
  OS-keyring entry) and offer firewall rule hit counters grouped by
  interface, a live allowed/blocked firewall log stream, interface and
  link-aggregation status, VPN/Tailscale status, and PF state-table and
  resource health cards. Firmware checks compare the installed version
  with the firewall's cached release changelog without starting an update
  check; missing or stale cache cannot prove current availability. Installed
  plugins show cached upgrade targets when available. Backups can select a
  saved configuration revision and are always stored locally as
  passphrase-encrypted `*.xml.age` files (restore with
  `age -d -o config.xml <file>.xml.age`, then import in the OPNsense GUI).
  The SSH terminal uses a separate interactive user login with explicit
  SHA-256 host-key verification; API credentials never authenticate SSH.
  Help moved to Ctrl 9.

### Fixed

- Wide data tables now show a draggable horizontal control when window or OS
  settings hide native scrollbars. Narrow device and diagnostic tabs wrap
  instead of concealing navigation or table columns.

## [0.1.5] - 2026-09-20

### Fixed

- **Network scanner CSV export did nothing**: the export used a browser-style
  blob download, which Tauri's webview silently ignores. The export now asks
  for a destination with the native save dialog and writes the file through
  the backend, includes a UTF-8 BOM so spreadsheet apps read it correctly,
  and reports write failures in the view.

## [0.1.4] - 2026-09-20

### Added

- **Update notifications**: on startup the app checks GitHub Releases for a
  newer stable version and shows a dismissible notice with a link to the
  release page. It never downloads or installs anything, prereleases are
  never announced, and the check can be turned off in the new Updates
  section of Help (Ctrl 8), which also shows the running version.

## [0.1.3] - 2026-09-19

### Fixed

- **MikroTik profile selector not updating**: creating, editing, or deleting
  a profile in the Profiles tab did not refresh the profile dropdown in the
  view header until the app was restarted. The header selector now refreshes
  immediately after any profile change.

## [0.1.2] - 2026-09-19

### Fixed

- **Startup crash on Windows release builds**: opening the app with a
  database whose migration checksums no longer match (written by an older or
  differently-built binary) panicked in the setup hook. The database is now
  moved to a timestamped backup and recreated instead of crashing.
- Pinned LF line endings via `.gitattributes` — sqlx checksums the embedded
  migration files, and platform-dependent CRLF conversion made identical
  content hash differently.

## [0.1.1] - 2026-09-19

First public release.

### Added

- **Ping / ICMP**: live latency table and graph with reopenable session
  history; the engine is chosen automatically per platform (`surge-ping`
  first, `winicmp` fallback on Windows, `osping` on POSIX).
- **Traceroute**: OS-native traceroute with hops streaming in live, kept in
  history like ping sessions.
- **Web Benchmark**: download-speed test against a URL with live progress and
  final stats.
- **Network scanner**: discovers hosts on the local network.
- **MTU Discovery**: path-MTU probing with Don't-Fragment ICMP (bracketing
  steps, then binary search) and a TCP PLPMTUD fallback on Linux when ICMP is
  filtered.
- **DNS Toolkit**: resolver diagnostics with an evidence chain for pinning
  down where resolution breaks.
- **MikroTik management**:
  - Profiles with credentials stored in the OS keyring (never in the
    database).
  - Live per-device monitoring: resources, interface rates, VLANs, and
    version/firmware status; up to eight concurrent device sessions that keep
    collecting in the background.
  - Per-device live log streams with severity highlighting and filtering.
  - Interactive SSH terminals, one per device, with scrollback surviving
    navigation and a switch warning that names the connection.
  - Backup download over SFTP and a backup library with line-by-line `.rsc`
    config diffing.
- Local SQLite history for everything a tool measures.
- In-app **Help** view, sidebar navigation with `Ctrl 1`–`Ctrl 8` shortcuts,
  and Light/Dark/OS themes.

[0.2.0]: https://github.com/tero-k/verkkokyyla/releases/tag/v0.2.0
[0.1.5]: https://github.com/tero-k/verkkokyyla/releases/tag/v0.1.5
[0.1.4]: https://github.com/tero-k/verkkokyyla/releases/tag/v0.1.4
[0.1.3]: https://github.com/tero-k/verkkokyyla/releases/tag/v0.1.3
[0.1.2]: https://github.com/tero-k/verkkokyyla/releases/tag/v0.1.2
[0.1.1]: https://github.com/tero-k/verkkokyyla/releases/tag/v0.1.1
