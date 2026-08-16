# HTTP Settings for Download/Page Speed Tests

## Goal

Allow users to configure the HTTP client used by the single-file download and full-page download speed tests. Settings are exposed through a collapsible panel in the existing download speed view, persisted to `localStorage`, and applied on both test modes.

## Scope

### In scope

- HTTP version selection (auto, HTTP/1.1 only, HTTP/2 prior knowledge).
- Connect timeout (seconds).
- Request timeout (seconds).
- Redirect handling (follow / don't follow, max redirect count).
- Compression toggle (gzip, brotli, deflate enabled/disabled together).
- Custom User-Agent header.
- Persist settings to `localStorage` with a versioned key.
- Reset-to-defaults button.
- Apply settings to both `run_download_speed_test` and `run_page_speed_test`.

### Out of scope

- Custom request headers beyond User-Agent.
- TLS certificate validation overrides.
- Proxy configuration.
- Per-mode settings (single-file vs. full-page).
- Global application settings page.

## Data Model

```ts
export const HTTP_VERSIONS = ["auto", "http1.1", "http2"] as const
export type HttpVersion = (typeof HTTP_VERSIONS)[number]

export const DEFAULT_HTTP_SETTINGS = {
  version: "auto" satisfies HttpVersion,
  connectTimeoutSec: 10,
  requestTimeoutSec: 60,
  followRedirects: true,
  maxRedirects: 10,
  compression: true,
  userAgent: "",
} as const

export type HttpSettings = {
  readonly version: HttpVersion
  readonly connectTimeoutSec: number
  readonly requestTimeoutSec: number
  readonly followRedirects: boolean
  readonly maxRedirects: number
  readonly compression: boolean
  readonly userAgent: string
}
```

`userAgent` is stored as an empty string when the built-in default should be used. When the user supplies a custom value it overrides both the single-download default (`verkkokyyla/0.1.0 download-speed-test`) and the page-test default (`verkkokyyla/0.1.0 page-speed-test`).

## Backend Design

### Rust DTO

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpSettingsDto {
    pub version: HttpVersion,
    pub connect_timeout_sec: u64,
    pub request_timeout_sec: u64,
    pub follow_redirects: bool,
    pub max_redirects: u32,
    pub compression: bool,
    pub user_agent: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HttpVersion {
    Auto,
    #[serde(rename = "http1.1")]
    Http1_1,
    #[serde(rename = "http2")]
    Http2,
}
```

### Command signatures

Both commands accept the settings DTO as a second argument:

```rust
pub async fn run_download_speed_test<F>(
    url: &str,
    settings: HttpSettingsDto,
    on_progress: F,
) -> Result<DownloadSpeedResultDto, DownloadError>

pub async fn run_page_speed_test<F>(
    url: &str,
    settings: HttpSettingsDto,
    on_progress: F,
) -> Result<PageSpeedResultDto, DownloadError>
```

The Tauri command registration in `src-tauri/src/lib.rs` is updated to deserialize the new `settings` argument and pass it through.

### Client builder

A shared helper in a new module `src-tauri/src/http_client.rs` builds the `reqwest::Client` from `HttpSettingsDto`. Both `download.rs` and `page_speed.rs` replace their existing inline client construction with a call to this helper.

```rust
fn build_client(settings: &HttpSettingsDto) -> Result<reqwest::Client, DownloadError> {
    let mut builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(settings.connect_timeout_sec))
        .timeout(Duration::from_secs(settings.request_timeout_sec));

    builder = match settings.version {
        HttpVersion::Auto => builder,
        HttpVersion::Http1_1 => builder.http1_only(),
        HttpVersion::Http2 => builder.http2_prior_knowledge(),
    };

    builder = if settings.follow_redirects {
        builder.redirect(reqwest::redirect::Policy::limited(settings.max_redirects as usize))
    } else {
        builder.redirect(reqwest::redirect::Policy::none())
    };

    if !settings.compression {
        builder = builder.gzip(false).brotli(false).deflate(false);
    }

    if !settings.user_agent.is_empty() {
        builder = builder.user_agent(&settings.user_agent);
    }

    builder.build().map_err(|e| DownloadError::Request(e.to_string()))
}
```

**HTTP/2 note:** `http2_prior_knowledge()` forces HTTP/2 without ALPN and assumes the server supports plaintext h2c or prior-knowledge TLS. This is labeled clearly in the UI.

### Backend validation

`build_client` also validates inputs:

- `connect_timeout_sec` and `request_timeout_sec` are clamped to `1..=300`.
- `max_redirects` is clamped to `0..=100`.
- Values outside the range are adjusted rather than rejected so a bad IPC call cannot crash the backend.

### Dependency changes

Add compression features to `reqwest` in `src-tauri/Cargo.toml`:

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "stream", "gzip", "brotli", "deflate"] }
```

Also bump `src-tauri/Cargo.toml` package version from `0.1.0` to `0.1.1` to match `package.json` and `tauri.conf.json`.

## Frontend Design

### State

A dedicated hook `useHttpSettings` manages settings state. `useDownloadSpeedTest` consumes the current snapshot when starting a test.

- On mount the hook reads from `localStorage` at key `verkkokyyla-http-settings-v1` and falls back to `DEFAULT_HTTP_SETTINGS`.
- Stored objects are shallow-merged over `DEFAULT_HTTP_SETTINGS` so missing fields fall back to defaults.
- If parsing fails or `localStorage` is unavailable, defaults are used.
- Every change writes back to `localStorage`.
- A `resetHttpSettings` function restores defaults and removes the stored key.

### UI

A collapsible "HTTP settings" panel is added below the URL/mode controls and above the progress/results area in `DownloadSpeedView`. The panel is implemented with native `<details>`/`<summary>` so it needs no extra state. It is collapsed by default; the open state is not persisted.

It contains:

| Setting | Control | Label / validation |
|---|---|---|
| HTTP version | `<select>` | options: `Auto`, `HTTP/1.1`, `HTTP/2 (prior knowledge)` |
| Connect timeout | number input | `min=1`, `max=300`, clamp on change |
| Request timeout | number input | `min=1`, `max=300`, clamp on change |
| Follow redirects | checkbox | — |
| Max redirects | number input | `min=0`, `max=100`, disabled when follow redirects is off |
| Compression | checkbox | label: `Enable gzip/brotli/deflate compression` |
| User-Agent | text input | optional; empty uses built-in default |

A "Reset to defaults" button restores default values.

**UI hint for HTTP/2:** below the version select, a small hint reads: *"HTTP/2 uses prior knowledge and only works with servers that support it."*

### IPC changes

`runDownloadSpeedTest` and `runPageSpeedTest` in `src/lib/ipc.ts` receive a second `HttpSettings` argument and pass it to the invoke payload.

## Error Handling

- Invalid numeric values are clamped by the UI and again by the backend.
- If `build_client` fails, the error is returned as a `DownloadError::Request` and rendered in the existing error banner.
- Empty `userAgent` falls back to the current built-in default strings.
- If `localStorage` is unavailable (private mode, quota, disabled), the app silently uses defaults and changes are not persisted.

## Testing

### Rust

- Unit test: `build_client` with defaults succeeds.
- Unit test: `build_client` clamps timeouts and max redirects.
- Integration test with a local HTTP server:
  - Custom User-Agent is present on the incoming request.
  - With `follow_redirects: false`, a 3xx response is returned to the caller.
  - With `follow_redirects: true` and `max_redirects: 1`, the second redirect returns an error or the first redirect response.
- Existing download/page-speed tests continue to use default-equivalent settings.

### Frontend

- Unit test for the `localStorage` round-trip of `HttpSettings`.
- Unit test for default values and reset behavior.
- Unit test for merging stored settings over defaults.

### E2E

- Open the HTTP settings panel.
- Change User-Agent to a custom value and run a single-file test; assert the mock receives the custom value.
- Toggle compression off and run a full-page test; assert the mock receives `compression: false`.
- Click "Reset to defaults" and assert values revert.
- Collapse and reopen the panel; assert values persist.

### Mock updates

`e2e/mock-ipc.ts` is updated so both download commands accept a `settings` payload and expose the last-received value on `window.__TAURI_MOCK_LAST_HTTP_SETTINGS__` for assertions.

## Migration / Backwards Compatibility

- Existing Tauri command signatures change: `run_download_speed_test` and `run_page_speed_test` now require a settings argument. The frontend is updated together with the backend.
- If a stored settings object cannot be parsed (wrong version or corrupted), it is discarded and defaults are used.

## Open Questions

None remaining; design approved by user.
