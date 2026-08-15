# NetDebug Ping

A small Tauri 2 desktop ping utility for Windows, macOS, and Linux. It pings a target, streams results over Tauri Channels, and keeps a local SQLite history of sessions.

## Development

Requires [Rust](https://rustup.rs/) and [Node.js](https://nodejs.org/).

```bash
npm install
```

## Tests

```bash
# Rust unit + integration tests
cargo test

# Rust E2E tests that hit real network adapters (requires admin/root on Windows)
cargo test -- --ignored e2e

# Frontend unit tests
npx vitest run

# UI E2E tests with Playwright (uses Tauri mock)
npx playwright test
```

## Running

```bash
# Vite dev server + Tauri in dev mode
cargo tauri dev

# Production build
cargo tauri build
```

## Project layout

- `src-tauri/src/` — Rust backend: session manager, ping engines, stats, SQLite persistence.
- `src/` — React + TypeScript frontend: live view, graphs, session history.
- `e2e/` — Playwright E2E tests with an in-browser Tauri mock.
- `src-tauri/tests/` — Rust integration tests against the real backend.

## Notes

- On Windows the app tries the `surge-ping` engine first and falls back to `winicmp`.
- On POSIX it tries `surge-ping` first and falls back to `osping`.
- The live table caps visible rows to 500; the graph can show the full session.
