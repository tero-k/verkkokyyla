# Multi-device concurrent MikroTik sessions

Goal: allow several switches/routers to be monitored at the same time (parallel
live sessions), presented as a device switcher + status list in the MikroTik
view. Interactive SSH terminals are a follow-up phase, not part of the first
slice.

## Current state (verified against the code)

### Backend

- `src-tauri/src/mikrotik/manager.rs`
  - `MikrotikInner { active: Option<ActiveSession> }` (line 36) — single slot.
  - `start()` returns typed `MikrotikManagerError::AlreadyRunning` when the slot
    is occupied (line 236).
  - `stop()` takes no args and stops the one session (line 293).
  - `clear_active(session_id)`, `is_active_session(session_id)`,
    `active_version_target(profile_id)` already key off IDs — mechanical to
    convert to map lookups.
  - `delete_profile` refuses while the profile backs the ACTIVE session
    (line 165) — must check map membership instead.
- `src-tauri/src/mikrotik/runtime.rs` — `run_mikrotik_session` is fully
  self-contained per session (own API handle, cancel watch, Channels). No
  changes needed for parallelism.
- `src-tauri/src/mikrotik/logs.rs` — `LogStreamManager` has the same
  single-slot shape (`LogInner { active: Option<ActiveLogStream> }`, line 69)
  and its own `AlreadyRunning`.
- `src-tauri/src/lib.rs` — commands `mikrotik_start` (line 307) takes
  `profile_id` + two Tauri Channels; `mikrotik_stop` (line 327) takes nothing.
- DB schema already supports any number of sessions (rows keyed by
  `session_id`); no migration needed.
- russh 0.63 + russh-sftp already used in `backup.rs` with
  `AcceptUnknownHostKey` and keyring password auth (reusable later for the
  terminal phase).

### Frontend

- `src/hooks/useMikrotik.ts` — single-session state: one `running` flag, one
  set of refs with rAF-batched flush, and an unmount effect (lines 78–84) that
  stops the session when leaving the view.
- `src/views/MikrotikView.tsx` — one profile selector + Connect/Disconnect in
  the header; tabs render the single session's data.
- `src/lib/ipc.ts` — `mikrotikStart(profileId, onEvent, onStatus)` wraps two
  Channels; `mikrotikStop()` takes no args (line 299).
- `e2e/` Playwright Tauri mock models the single-session commands.

## Design decisions (agreed)

1. One monitoring session per profile, many profiles in parallel. The active
   map is keyed by `profile_id`; a second start for the same profile is a
   typed `AlreadyRunningForProfile(profile_id)` error.
2. Sessions survive navigation away from the MikroTik view — remove the
   unmount-stop effect. Tasks die with the app on exit.
3. Concurrency cap: 8 simultaneous monitoring sessions
   (`MAX_CONCURRENT_SESSIONS`), typed `TooManySessions` error beyond that.
4. Interactive SSH terminal is phase 2 (separate session type, russh shell
   channel + PTY, xterm.js frontend).

## Phase 1 — Backend: multi-slot manager

Files: `src-tauri/src/mikrotik/manager.rs`, `types.rs`, `logs.rs`,
`src-tauri/src/lib.rs`, plus `src-tauri/tests` manager tests.

- `MikrotikInner.active` → `HashMap<i64 /* profile_id */, ActiveSession>`.
- `start()`:
  - error `AlreadyRunningForProfile(profile_id)` if the profile already has a
    session;
  - error `TooManySessions` at 8 active sessions;
  - otherwise unchanged (create DB session row, spawn runtime, insert into
    map).
- `stop(session_id)`:
  - look up the entry by session_id, remove it, send cancel, await the join
    handle;
  - `NoActiveSession` when the id is unknown.
- `clear_active(session_id)` / `is_active_session(session_id)` — map
  remove/contains (unchanged signatures).
- `active_version_target(profile_id)` — map lookup by profile_id (unchanged
  signature).
- `delete_profile` — refuse with `ProfileInUse` when the profile id is in the
  map.
- New manager method + command `mikrotik_list_active()` returning
  `Vec<ActiveSessionDto { session_id, profile_id }>` so the frontend can
  rehydrate its switcher after a reload.
- `LogStreamManager`: same conversion, keyed by profile_id, `log_stop` gains
  an id argument.
- `lib.rs`: update `mikrotik_stop` / `mikrotik_log_stop` signatures, add
  `mikrotik_list_active` to the handler list.
- Error enum: add `AlreadyRunningForProfile(i64)` and `TooManySessions`;
  keep `AlreadyRunning` only if still used by logs during transition —
  prefer replacing both at once.
- Tests:
  - update existing `AlreadyRunning` assertions to the new error;
  - add: two profiles monitored concurrently both tick and persist snapshots;
    stop(session_id) stops only that session; start-same-profile-twice is
    refused; 9th session is refused with `TooManySessions`;
    delete_profile refused while active.

## Phase 2 — Frontend: per-device state + switcher UI

Files: `src/hooks/useMikrotik.ts` (split), `src/views/MikrotikView.tsx`,
`src/lib/ipc.ts`, `e2e/` mock, vitest specs.

- Refactor `useMikrotik` into:
  - a fleet-level hook: profiles, `activeSessions: Map<profileId, …>`,
    connect/disconnect per profile, rehydrate via `mikrotik_list_active`;
  - a per-profile session state object holding the existing refs + rAF flush
    batching (extract current logic unchanged, keyed by profileId) — a small
    class outside React plus `useSyncExternalStore`, or a sub-hook per active
    device. Pick one and keep the batching semantics identical.
- Remove the unmount-stop effect.
- `MikrotikView`:
  - header: profile selector + Connect adds a device to the active strip;
  - device strip (under the header): one chip per active session — live
    status dot, profile name, click to select, per-device Disconnect;
  - existing tabs (System/Interfaces/VLANs/Logs) render the SELECTED
    device's data unchanged;
  - Logs tab: per-device log stream start/stop.
- `ipc.ts`: `mikrotikStop(sessionId)`, `mikrotikLogStop(id)`,
  `mikrotikListActive()`.
- e2e mock: model multiple concurrent sessions keyed by session_id.
- Tests: vitest for the per-profile state split (two profiles receive
  independent snapshots); Playwright: connect two mocked devices, switch
  between them, disconnect one.

## Phase 3 (follow-up, not in this slice) — Interactive SSH terminals

- `src-tauri/src/mikrotik/terminal.rs`: `TerminalManager` with
  `open(profile_id, cols, rows, on_data Channel)` → russh
  `channel_open_session` + `request_pty` + `request_shell`, reader task
  streaming bytes over the Channel; `write(id, bytes)`, `resize(id, cols,
  rows)`, `close(id)`. Reuse `AcceptUnknownHostKey` and keyring lookup from
  `backup.rs`. SSH port: follow the backup flow (port 22) unless a profile
  field is added.
- Frontend: add `@xterm/xterm` + `@xterm/addon-fit`; a Terminal tab rendering
  the selected device's shell; multiple terminals = multiple backend ids.

## Risks

- IPC breakage: `mikrotik_stop` / `mikrotik_log_stop` gain parameters — update
  `lib.rs`, `ipc.ts`, the e2e mock, and tests in the same change.
- DB write rate multiplies with session count (one snapshot row per session
  per 5s) — fine for SQLite; the history list may need profile grouping later.
- RouterOS service load grows per session; the per-profile + global caps keep
  it bounded.

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml` — including new
  concurrency tests.
- `npx vitest run` and `npx playwright test`.
- Manual: run `npm run tauri -- dev`, connect to two real (or containerized)
  RouterOS devices, confirm both dashboards tick independently and one can be
  disconnected without disturbing the other.
