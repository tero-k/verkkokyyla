# Work plan status

Migrated 2026-09-20 from `.omo/boulder.json` (Oh My Opencode). All tracked
works below are **shipped** — verify against `CHANGELOG.md`, which is the
authoritative record from now on. The boulder "paused" states were stale.

| Plan | Shipped in | Evidence |
| --- | --- | --- |
| `netdebug-ping.md` | 0.1.1 | Ping/ICMP tool with history |
| `traceroute-tool.md` | 0.1.1 | Traceroute tool with history |
| `dns-tester.md` | 0.1.1 | DNS Toolkit |
| `web-benchmark-plan.md` | 0.1.1 | Web Benchmark |
| `2026-08-19-local-network-scanner.md` | 0.1.1 | Network scanner |
| `2026-08-19-scanner-port-service-scan.md` | 0.1.1 | Network scanner |
| `mtu-discovery.md` | 0.1.1 | MTU Discovery |
| `mikrotik-monitoring.md` | 0.1.1 | MikroTik management (F1–F5 resolved per boulder notes; F3b waived, F2 accepted as tech debt: `runtime.rs`/`backup.rs`/`parse.rs`/`client.rs`/`types.rs`/`manager.rs` split) |
| `multi-mikrotik-sessions.md` | 0.1.1 | Up to 8 concurrent device sessions |
| `update-notifier.md` | 0.1.4 | Update notifications |

## Known tech debt carried forward

- MikroTik F2: large modules accepted as-is; candidate split into
  `runtime.rs` / `backup.rs` / `parse.rs` / `client.rs` / `types.rs` /
  `manager.rs` remains unscheduled.

## Conventions going forward

- New work plans: write to `.omp/plans/<slug>.md` when a task warrants a
  written plan; small tasks need no plan file.
- Record outcomes in `CHANGELOG.md` (Unreleased section), not here.
- QA screenshots go to the gitignored `test-results/` (Playwright default);
  README screenshots regenerate via `UPDATE_README_SHOTS=1 npx playwright
  test e2e/readme-shots.spec.ts` into `docs/screenshots/`.
