# Plan: opt-out update notifier

Notify the user when a newer Verkkokyylä release exists, with a link to the
release page. No auto-download, no auto-install, and **not**
`tauri-plugin-updater` (that plugin is built around signed download/install).

## Decisions (locked)

1. **Flag storage: localStorage** on the frontend, following the
   `usePortScanConsent.ts` / `useHttpSettings.ts` precedent. No new migration,
   no settings IPC. (Rejected alternative: SQLite `app_settings` table —
   survives webview data wipes but costs two extra commands for UI state.)
2. **Prereleases: permanently ignored.** Only stable releases ever trigger the
   banner. This holds even if prerelease/beta tags are published later — users
   must never be nudged onto a prerelease. GitHub's `/releases/latest`
   endpoint already excludes drafts and prereleases, so this is the default
   behavior; the tag-parsing code must not loosen it.

## Source of truth

- Current version: `env!("CARGO_PKG_VERSION")` (kept in sync with
  `package.json` / `tauri.conf.json` at release time).
- Latest version: `GET https://api.github.com/repos/tero-k/verkkokyyla/releases/latest`.
  The release workflow (`.github/workflows/release.yml`) publishes non-draft,
  non-prerelease releases on `v*` tags via tauri-action, so the endpoint is
  reliable. Response fields used: `tag_name` (e.g. `v0.1.4`), `html_url`.

## Behavior spec

- Check runs **once per app launch**, ~2 s after the UI mounts. Non-blocking,
  off the startup path; no re-checks while the app runs.
- Any failure — offline, timeout, DNS, GitHub outage, malformed JSON,
  non-semver tag — resolves to "no update" **silently**. No error UI, no log
  spam beyond a debug-level trace.
- Banner text: "Verkkokyylä vX.Y.Z is available (you have vA.B.C)" +
  **View release** button + dismiss (✕).
- **Dismissal is per-version**: dismissing the vX.Y.Z banner hides it until a
  strictly newer version appears.
- **Opt-out toggle**: "Check for updates on startup" (default **on**) in the
  Help view. Off = the frontend never invokes the command; no request leaves
  the machine.

## Backend: new module `src-tauri/src/update.rs`

- `#[tauri::command] async fn check_for_update() -> Option<UpdateInfo>`;
  `UpdateInfo { version: String, url: String }` (serde, camelCase).
- Dedicated `reqwest` client: hardcoded 5 s connect/total timeout, redirects
  off, User-Agent `verkkokyyla/<CARGO_PKG_VERSION>` (GitHub API requires a
  UA). Deliberately separate from `http_client.rs` — that path is driven by
  user benchmark settings (HTTP/2-only, custom UA, pinned DNS) which an update
  check must not inherit.
- Parse `tag_name`, strip one leading `v`, compare with
  `semver::Version::parse` against `env!("CARGO_PKG_VERSION")`.
  - New dependency: `semver` (do not hand-roll version comparison).
  - Equal or older → `None`. Unparseable tag → `None`.
- URL: use `html_url` only if it starts with `https://github.com/`; otherwise
  fall back to the hardcoded
  `https://github.com/tero-k/verkkokyyla/releases/latest`.
- Opening the link: new dependency `tauri-plugin-opener` — three wiring steps:
  Cargo dependency, `.plugin(tauri_plugin_opener::init())` in the
  `tauri::Builder` chain in `lib.rs` (next to the existing
  `tauri_plugin_dialog::init()` at `lib.rs:605`), and a permission entry in
  `src-tauri/capabilities/default.json`. The frontend side needs the matching
  npm package `@tauri-apps/plugin-opener`. `window.open` would navigate the
  webview itself, so the opener plugin is required.
- Register in `src-tauri/src/lib.rs`: `mod update;` and add
  `update::check_for_update` to the existing `tauri::generate_handler![...]`
  list (handler list starts at `lib.rs:666`).

## Frontend

- `src/hooks/useUpdateCheck.ts` — mirrors `usePortScanConsent.ts`:
  - `verkkokyyla-update-check-enabled-v1` → opt-out flag; absent/`"true"` =
    enabled (default on), anything else = disabled.
  - `verkkokyyla-update-dismissed-v1` → dismissed version string.
  - On mount: if enabled, call `checkForUpdate()`; expose
    `{ update, dismiss, enabled, setEnabled }`. `dismiss()` writes the current
    `update.version` and clears the banner state.
- `src/components/UpdateBanner.tsx` (+ `UpdateBanner.module.css`, per the
  per-component CSS-module convention). Mounted in `src/App.tsx` inside
  `styles.shell`, directly above `<main className={styles.content}>` so it
  spans all views.
- The **View release** button calls the opener plugin's `openUrl(update.url)`.
- `src/views/HelpView.tsx`: new "Updates" section with the toggle, and show
  the running app version next to it via `getVersion()` from
  `@tauri-apps/api/app` (Help currently displays no version).
- `src/lib/types.ts`: `UpdateInfoDto`. `src/lib/ipc.ts`: typed
  `checkForUpdate(): Promise<UpdateInfoDto | null>` wrapper.

## Tests

- **Rust** (`src-tauri/src/update.rs` tests + integration):
  - Unit: tag parsing (`v` prefix, missing prefix, junk), semver compare
    (newer / equal / older / prerelease tag ignored).
  - HTTP: `wiremock` (already a dev-dependency) — 200 newer, 200 older,
    malformed JSON, non-GitHub `html_url` falls back, timeout → `None`.
- **Frontend**: vitest for `useUpdateCheck` (default-on, opt-out suppresses
  the invoke, per-version dismissal, re-show on newer version), following
  `src/hooks/__tests__/useHttpSettings.test.ts`.
- **E2E**: extend `e2e/mock-ipc.ts` with a `check_for_update` mock; one
  Playwright spec: banner appears → dismiss hides it → Help toggle disables
  the call on next launch.

## Files

New:
- `src-tauri/src/update.rs`
- `src/hooks/useUpdateCheck.ts`, `src/hooks/__tests__/useUpdateCheck.test.ts`
- `src/components/UpdateBanner.tsx`, `src/components/UpdateBanner.module.css`
- `e2e/update-check.spec.ts`

Edited:
- `src-tauri/Cargo.toml` (+`semver`, +`tauri-plugin-opener`)
- `package.json` (+`@tauri-apps/plugin-opener`)
- `src-tauri/src/lib.rs` (module + handler + opener plugin init)
- `src-tauri/capabilities/default.json` (opener permission)
- `src/lib/types.ts`, `src/lib/ipc.ts`
- `src/App.tsx` (banner mount)
- `src/views/HelpView.tsx` (toggle + version display)
- `e2e/mock-ipc.ts`
- `CHANGELOG.md`

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml` (new + existing pass)
- `npx vitest run`
- `npx playwright test e2e/update-check.spec.ts`
- `npm run tauri -- dev`: banner shows when the mock/real API reports a newer
  version; dismiss + toggle behave; offline (network disabled) shows nothing
  and no error UI.

## Out of scope

- Downloading or installing anything.
- Checking more than once per launch / scheduled checks.
- Showing release notes inline (the link covers it).
- Prerelease channels of any kind.
