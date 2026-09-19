# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[0.1.2]: https://github.com/tero-k/verkkokyyla/releases/tag/v0.1.2
[0.1.1]: https://github.com/tero-k/verkkokyyla/releases/tag/v0.1.1
