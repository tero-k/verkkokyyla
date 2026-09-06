# Verkkokyylä

A small Tauri 2 desktop ping utility for Windows, macOS, and Linux. It pings a target, streams results over Tauri Channels, and keeps a local SQLite history of sessions.

## Development

Requires [Rust](https://rustup.rs/) and [Node.js](https://nodejs.org/).

The app includes Ping, Download Speed, Traceroute, and MikroTik tabs. Traceroute uses the operating system's native traceroute command, streams hops in real time, and keeps a local history just like the other tools. MikroTik monitoring connects to RouterOS devices for snapshots and backups.

```bash
npm install
```

## Tests

```bash
# Rust unit + integration tests
cargo test --manifest-path src-tauri/Cargo.toml

# Rust E2E tests that hit real network adapters (requires admin/root on Windows)
cargo test --manifest-path src-tauri/Cargo.toml -- --ignored e2e

# Frontend unit tests
npx vitest run

# UI E2E tests with Playwright (uses Tauri mock)
npx playwright test
```

## Running

```bash
# Vite dev server + Tauri in dev mode
npm run tauri -- dev

# Production build
npm run tauri -- build
```

## MikroTik monitoring

The MikroTik tab monitors RouterOS devices through the REST API and can create downloadable router backups over SSH/SFTP.

Requirements:

- RouterOS v7.1 or newer with the `www-ssl` service enabled for HTTPS REST API access.
- Plain HTTP REST access is supported only on RouterOS v7.9 or newer with the `www` service enabled.
- The router SSH service must be enabled for backup creation and SFTP download.

MikroTik credentials are stored in the operating system keyring. The app keeps connection metadata in its local database, but the MikroTik password is not stored there.

Backup flow: the app asks RouterOS to create a backup file on the router, downloads that file with SFTP, and then removes the temporary file from the router.

Use the MikroTik tab for both live RouterOS monitoring and on-demand MikroTik backups.

Accepted risk: for typical LAN-managed routers, the app accepts unknown SSH host keys on first connection instead of requiring a pre-seeded known-hosts entry.

## Project layout

- `src-tauri/src/` — Rust backend: session manager, ping engines, stats, SQLite persistence.
- `src/` — React + TypeScript frontend: live view, graphs, session history.
- `e2e/` — Playwright E2E tests with an in-browser Tauri mock.
- `src-tauri/tests/` — Rust integration tests against the real backend.

## Notes

- On Windows the app tries the `surge-ping` engine first and falls back to `winicmp`.
- On POSIX it tries `surge-ping` first and falls back to `osping`.
- The live table caps visible rows to 500; the graph can show the full session.
