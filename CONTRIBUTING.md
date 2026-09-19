# Contributing

Thanks for your interest in Verkkokyylä! This document covers what you need
to get a change from idea to merged pull request.

## Development setup

Requires [Rust](https://rustup.rs/) and [Node.js](https://nodejs.org/).

```bash
npm install
npm run tauri -- dev
```

## Tests

Every change should keep all four suites green:

```bash
# Rust unit + integration tests
cargo test --manifest-path src-tauri/Cargo.toml

# Frontend unit tests
npx vitest run

# UI E2E tests with Playwright (uses a Tauri mock, no hardware needed)
npx playwright test
```

The Rust E2E tests that hit real network adapters
(`cargo test --manifest-path src-tauri/Cargo.toml -- --ignored e2e`) need
admin/root on Windows and real hardware — run them when your change touches
the network engines, but they are not required for a PR.

## Conventions

- **Commits**: [Conventional Commits](https://www.conventionalcommits.org/)
  with a scope, e.g. `feat(mikrotik): add backup library history`,
  `fix(dns): handle truncated TCP responses`, `test(e2e): expect MikroTik nav
  item`. Look at `git log` for the house style.
- **Rust**: `cargo fmt` clean and `cargo clippy` warning-free.
- **TypeScript**: `npx tsc --noEmit` clean.
- **UI changes**: follow [DESIGN.md](DESIGN.md) — all colors come from the
  token palette in `src/App.css`, measurements render in the mono font, and
  shared primitives live in `src/components/ui/`.

## Pull requests

- Keep PRs focused: one feature or fix per PR.
- Include tests for behavior changes — Rust integration tests for backend
  logic, vitest for hooks/components, Playwright for user journeys.
- Include a screenshot for visible UI changes.
- Update the `Unreleased` section of [CHANGELOG.md](CHANGELOG.md) for
  user-facing changes.

## Reporting bugs

Open an issue with the bug report template: what you did, what you expected,
what happened, your platform and app version. For security issues, please see
[SECURITY.md](SECURITY.md) instead of filing a public issue.
