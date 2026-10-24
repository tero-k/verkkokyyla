//! Per-profile OPNsense read-only API client.
//!
//! A fresh reqwest client is built per connection (same reasoning as
//! `mikrotik/client.rs`: per-client certificate policy): rustls TLS, 5s
//! connect timeout, 10s per finite request timeout, HTTPS only, HTTP Basic
//! auth from the profile's API key/secret. The secret is never logged and
//! never included in any error message.
//!
//! READ-ONLY CONTRACT: this client exposes ONLY fixed GET endpoints. There
//! is deliberately no POST/PUT/PATCH/DELETE method and no arbitrary
//! path/method invocation — not for firmware checks (the POST variant
//! triggers a synchronous probe), not for backups (the API offers no
//! create-revision action anyway), not for anything else. The SSE log
//! stream uses the same client WITHOUT a total response deadline so the
//! stream can stay open.

use std::time::Duration;

use async_trait::async_trait;
use futures_util::stream::{BoxStream, StreamExt};
use serde_json::Value;
use thiserror::Error;

use crate::opnsense::types::{
    OpnsenseInterfaceDto, OpnsenseIpsecSessionDto, OpnsenseLaggDto, OpnsenseLaggMemberDto,
    OpnsenseOpenvpnSessionDto, OpnsensePfStatesDto, OpnsenseResourcesDto, OpnsenseRevisionDto,
    OpnsenseRuleDto, OpnsenseTailscalePeerDto, OpnsenseTailscaleStatusDto, OpnsenseUnmappedRuleDto,
    OpnsenseWireguardInterfaceDto, OpnsenseWireguardPeerDto, OpnsenseWireguardStatusDto,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// Downloading a saved config revision legitimately exceeds 10s on slow
/// links; the byte bound below still caps the transfer.
pub const DOWNLOAD_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Upper bound for a downloaded config revision (16 MiB).
pub const MAX_REVISION_BYTES: usize = 16 * 1024 * 1024;
/// Upper bound for large JSON payloads (firmware info is package-heavy).
pub const MAX_JSON_BYTES: usize = 16 * 1024 * 1024;

/// Errors produced by the OPNsense API client and its defensive parsers.
/// Never carries credentials.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum OpnsenseError {
    /// TCP connect failed, the connection was refused, or the connection
    /// dropped before the response completed.
    #[error("connection failed: {0}")]
    Connect(String),

    /// Connect or request timeout.
    #[error("timeout: {0}")]
    Timeout(String),

    /// TLS handshake or certificate validation failure.
    #[error("TLS error: {0}")]
    Tls(String),

    /// HTTP 401 — API key/secret rejected.
    #[error("authentication failed (401): check the API key and secret")]
    Unauthorized,

    /// HTTP 403 — the API key lacks privileges for this endpoint.
    #[error("access denied (403): the API key lacks privileges for this endpoint")]
    Forbidden,

    /// HTTP 404 — endpoint or plugin missing on this OPNsense version.
    #[error("not found (404): {0}")]
    NotFound(String),

    /// Any other non-success HTTP status.
    #[error("OPNsense API error {status}: {message}")]
    Http { status: u16, message: String },

    /// Response body could not be decoded as the expected JSON shape.
    #[error("parse error: {0}")]
    Parse(String),

    /// A bounded download exceeded its byte cap.
    #[error("download exceeded the {0}-byte limit")]
    TooLarge(usize),
}

impl OpnsenseError {
    /// Stable machine-readable discriminator.
    pub fn kind(&self) -> &'static str {
        match self {
            OpnsenseError::Connect(_) => "connect",
            OpnsenseError::Timeout(_) => "timeout",
            OpnsenseError::Tls(_) => "tls",
            OpnsenseError::Unauthorized => "unauthorized",
            OpnsenseError::Forbidden => "forbidden",
            OpnsenseError::NotFound(_) => "not-found",
            OpnsenseError::Http { .. } => "http",
            OpnsenseError::Parse(_) => "parse",
            OpnsenseError::TooLarge(_) => "too-large",
        }
    }
}

/// Classify a reqwest transport error (pattern: `mikrotik/error.rs`).
pub(crate) fn classify_transport(err: &reqwest::Error) -> OpnsenseError {
    use std::error::Error as _;

    let message = err.to_string();

    if err.is_timeout() {
        return OpnsenseError::Timeout(message);
    }

    let mut source = err.source();
    let mut tls_hint =
        message.to_lowercase().contains("certificate") || message.to_lowercase().contains("tls");
    while let Some(s) = source {
        let text = s.to_string().to_lowercase();
        if text.contains("certificate") || text.contains("tls") {
            tls_hint = true;
        }
        source = s.source();
    }

    if tls_hint {
        OpnsenseError::Tls(message)
    } else {
        OpnsenseError::Connect(message)
    }
}

/// Connection parameters for one OPNsense profile. The API key/secret
/// arrive keyring-fetched from the caller and are used only for the Basic
/// auth header. HTTPS is unconditional.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpnsenseConnection {
    pub host: String,
    pub port: u16,
    pub allow_invalid_certs: bool,
    pub api_key: String,
    pub api_secret: String,
}

/// Raw rows from search endpoints. GET pagination does not exist on
/// OPNsense (searchRecordsetBase reads POST only), so every search
/// endpoint is a single GET returning all rows; `total` is the reported
/// total for truncation detection (`rows < total`).
#[derive(Clone)]
pub struct RuleSearchPage {
    pub rules: Vec<OpnsenseRuleDto>,
    pub total: i64,
}

#[derive(Clone)]
pub struct InterfaceInfoPage {
    pub interfaces: Vec<OpnsenseInterfaceDto>,
    pub total: i64,
}

/// Fresh per-UUID counters from `filter_util/ruleStats`.
#[derive(Clone, Debug, Default)]
pub struct RuleStat {
    pub packets: Option<i64>,
    pub evaluations: Option<i64>,
    pub bytes: Option<i64>,
    pub states: Option<i64>,
}

/// One active PF rule ID from `diagnostics/firewall/listRuleIds`.
#[derive(Clone, Debug)]
pub struct PfRuleId {
    pub id: String,
    pub description: Option<String>,
}

/// Parsed `diagnostics/system/system_time` payload.
#[derive(Clone, Debug, Default)]
pub struct SystemTimePayload {
    pub uptime: Option<String>,
    pub loadavg: Option<String>,
}

/// One entry of the firewall's cached release changelog (`info.changelog`):
/// series + version + release date. The firewall fetches and
/// signature-verifies this list itself during its own update checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirmwareChangelogItem {
    pub series: Option<String>,
    pub version: String,
    pub date: Option<String>,
}

/// Parsed `core/firmware/info` payload.
#[derive(Clone, Debug, Default)]
pub struct FirmwareInfoPayload {
    pub product_version: Option<String>,
    /// Package ABI as the device reports it, e.g. "26.7" (CORE_ABI).
    pub abi: Option<String>,
    /// Installed plugins: (name, version).
    pub plugins: Vec<(String, Option<String>)>,
    /// Upgrade targets from the device's *previous* check. No probe is
    /// started here; absent cache leaves plugin availability unknown.
    pub cached_plugin_upgrades: Vec<(String, String)>,
    /// Cached release changelog (all series). Empty when the firewall has
    /// never fetched one.
    pub changelog: Vec<FirmwareChangelogItem>,
}

/// Byte stream of an open SSE response (no total deadline).
pub type SseByteStream = BoxStream<'static, Result<bytes::Bytes, OpnsenseError>>;

/// The fixed read-only endpoint surface. Tests inject fakes; production is
/// [`OpnsenseClient`].
#[async_trait]
pub trait OpnsenseApi: Send + Sync {
    /// `GET /api/core/system/status` — connectivity/authorization probe.
    async fn get_system_status(&self) -> Result<(), OpnsenseError>;

    /// `GET /api/firewall/filter/searchRule?show_all=1` — ALL rules in one
    /// GET (pagination params are POST-only and ignored on GET).
    async fn search_rules(&self) -> Result<RuleSearchPage, OpnsenseError>;

    /// `GET /api/firewall/filter_util/ruleStats` — counters keyed by UUID.
    async fn rule_stats(&self) -> Result<Vec<(String, RuleStat)>, OpnsenseError>;

    /// `GET /api/diagnostics/firewall/listRuleIds` — active PF rule IDs.
    async fn list_rule_ids(&self) -> Result<Vec<PfRuleId>, OpnsenseError>;

    /// `GET /api/interfaces/overview/interfacesInfo` — ALL interfaces in
    /// one GET, including lagg membership (`laggport`) and media.
    async fn interfaces_info(&self) -> Result<InterfaceInfoPage, OpnsenseError>;

    /// `GET /api/diagnostics/interface/get_interface_statistics`.
    async fn interface_statistics(&self) -> Result<Vec<OpnsenseInterfaceDto>, OpnsenseError>;

    /// `GET /api/diagnostics/firewall/pfStates` — state table usage.
    async fn pf_states(&self) -> Result<OpnsensePfStatesDto, OpnsenseError>;

    /// `GET /api/diagnostics/system/system_resources` — memory only.
    async fn system_resources(&self) -> Result<OpnsenseResourcesDto, OpnsenseError>;

    /// `GET /api/diagnostics/system/system_time` — uptime and loadavg.
    async fn system_time(&self) -> Result<SystemTimePayload, OpnsenseError>;

    /// `GET /api/diagnostics/cpu_usage/stream` — the only live CPU source
    /// (SSE, `iostat -w 1`); opens the stream, takes the first INTERVAL
    /// sample (the first event is a since-boot average) and closes it.
    async fn cpu_usage_sample(&self) -> Result<f64, OpnsenseError>;

    /// `GET /api/wireguard/service/show`.
    async fn wireguard_show(&self) -> Result<OpnsenseWireguardStatusDto, OpnsenseError>;

    /// `GET /api/openvpn/service/searchSessions`.
    async fn openvpn_sessions(&self) -> Result<Vec<OpnsenseOpenvpnSessionDto>, OpnsenseError>;

    /// `GET /api/ipsec/sessions/searchPhase1`.
    async fn ipsec_phase1(&self) -> Result<Vec<OpnsenseIpsecSessionDto>, OpnsenseError>;

    /// `GET /api/tailscale/status/status` (+ `service/status` merged in by
    /// the caller; this client exposes them separately).
    async fn tailscale_status(&self) -> Result<OpnsenseTailscaleStatusDto, OpnsenseError>;

    /// `GET /api/tailscale/service/status` — service running flag only.
    async fn tailscale_service_status(&self) -> Result<Option<bool>, OpnsenseError>;

    /// `GET /api/core/firmware/info` — installed core version, package
    /// ABI, and installed plugins. Never POSTs (that would trigger a
    /// synchronous check on the firewall).
    async fn firmware_info(&self) -> Result<FirmwareInfoPayload, OpnsenseError>;

    /// `GET /api/core/backup/backups/this` — saved config revisions.
    async fn backup_revisions(&self) -> Result<Vec<OpnsenseRevisionDto>, OpnsenseError>;

    /// `GET /api/core/backup/download/this/<id>` — bounded to
    /// [`MAX_REVISION_BYTES`].
    async fn download_revision(&self, id: &str) -> Result<Vec<u8>, OpnsenseError>;

    /// `GET /api/diagnostics/firewall/streamLog` — SSE byte stream without
    /// a total response deadline; cancellation drops the stream.
    async fn open_log_stream(&self) -> Result<SseByteStream, OpnsenseError>;
}

pub struct OpnsenseClient {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    api_secret: String,
}

impl OpnsenseClient {
    /// Build a fresh client for a profile. HTTPS-only: the base URL is
    /// always `https://host:port`.
    pub fn new(conn: &OpnsenseConnection) -> Result<Self, OpnsenseError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            // No default total timeout: finite requests pass their own
            // per-request timeout, the SSE stream must stay open.
            .danger_accept_invalid_certs(conn.allow_invalid_certs)
            .build()
            .map_err(|e| OpnsenseError::Connect(e.to_string()))?;
        let host = if conn.host.parse::<std::net::Ipv6Addr>().is_ok() {
            format!("[{}]", conn.host)
        } else {
            conn.host.clone()
        };
        Ok(Self {
            client,
            base_url: format!("https://{host}:{}", conn.port),
            api_key: conn.api_key.clone(),
            api_secret: conn.api_secret.clone(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/{}", self.base_url, path.trim_start_matches('/'))
    }

    fn request(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .get(self.url(path))
            .basic_auth(&self.api_key, Some(&self.api_secret))
    }

    async fn get_json(&self, path: &str, query: &[(&str, String)]) -> Result<Value, OpnsenseError> {
        let response = self
            .request(path)
            .query(query)
            .timeout(DEFAULT_REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|e| classify_transport(&e))?;
        let response = check_status(response)?;
        read_bounded_text(response, MAX_JSON_BYTES)
            .await
            .and_then(|body| {
                serde_json::from_str(&body)
                    .map_err(|_| OpnsenseError::Parse("response is not valid JSON".to_owned()))
            })
    }

    /// SSE/streaming GET without a total deadline; errors surface as stream
    /// items so cancellation stays responsive.
    async fn get_stream(&self, path: &str) -> Result<SseByteStream, OpnsenseError> {
        let response = self
            .request(path)
            .send()
            .await
            .map_err(|e| classify_transport(&e))?;
        let response = check_status(response)?;
        Ok(response
            .bytes_stream()
            .map(|chunk| chunk.map_err(|e| classify_transport(&e)))
            .boxed())
    }

    /// Open the `cpu_usage/stream` SSE feed and take one interval sample.
    /// `iostat`'s first event is a since-boot average, so the SECOND event
    /// (first real ~1 s interval) is preferred; if the stream ends after a
    /// single event that one is used. The caller wraps this in the
    /// per-request timeout.
    async fn cpu_usage_sample_inner(&self) -> Result<f64, OpnsenseError> {
        let mut stream = self.get_stream("diagnostics/cpu_usage/stream").await?;
        let mut buf: Vec<u8> = Vec::new();
        let mut first: Option<f64> = None;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if buf.len() + chunk.len() > 64 * 1024 {
                return Err(OpnsenseError::TooLarge(64 * 1024));
            }
            buf.extend_from_slice(&chunk);
            while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
                let sample = std::str::from_utf8(&buf[..pos])
                    .ok()
                    .and_then(|line| line.trim().strip_prefix("data:"))
                    .and_then(|data| serde_json::from_str::<Value>(data.trim()).ok())
                    .and_then(|value| field(&value, &["total"]).and_then(opt_f64));
                buf.drain(..=pos);
                let Some(total) = sample else {
                    continue;
                };
                match first {
                    None => first = Some(total),
                    // Second event = first true interval sample.
                    Some(_) => return Ok(total),
                }
            }
        }
        first.ok_or_else(|| OpnsenseError::Parse("CPU stream ended without a sample".to_owned()))
    }
}

/// Classify the HTTP status without displaying the untrusted response body:
/// diagnostic pages can echo request data, including authentication fields.
fn check_status(response: reqwest::Response) -> Result<reqwest::Response, OpnsenseError> {
    let status = response.status().as_u16();
    if (200..=299).contains(&status) {
        return Ok(response);
    }
    Err(match status {
        401 => OpnsenseError::Unauthorized,
        403 => OpnsenseError::Forbidden,
        404 => OpnsenseError::NotFound("endpoint or plugin unavailable".to_owned()),
        _ => OpnsenseError::Http {
            status,
            message: "request failed".to_owned(),
        },
    })
}

/// Read a response body with a hard byte cap.
async fn read_bounded_text(
    mut response: reqwest::Response,
    max: usize,
) -> Result<String, OpnsenseError> {
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| classify_transport(&e))? {
        if buf.len() + chunk.len() > max {
            return Err(OpnsenseError::TooLarge(max));
        }
        buf.extend_from_slice(&chunk);
    }
    String::from_utf8(buf).map_err(|e| OpnsenseError::Parse(format!("response is not UTF-8: {e}")))
}

/// Read a bounded binary body (config revision download).
async fn read_bounded_bytes(
    mut response: reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, OpnsenseError> {
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| classify_transport(&e))? {
        if buf.len() + chunk.len() > max {
            return Err(OpnsenseError::TooLarge(max));
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

// ---------------------------------------------------------------------------
// Defensive JSON helpers — OPNsense returns numbers as strings, booleans as
// "1"/"0"/true, and occasionally alternative field spellings. Unknown
// fields are ignored; missing fields stay None.
// ---------------------------------------------------------------------------

fn opt_str(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn opt_i64(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

fn opt_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn opt_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_i64().map(|v| v != 0),
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" | "up" | "running" | "connected" => Some(true),
            "0" | "false" | "no" | "off" | "down" | "stopped" | "disconnected" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// Fetch the first present key from an object.
fn field<'a>(obj: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|k| obj.get(*k))
}

fn str_field(obj: &Value, keys: &[&str]) -> Option<String> {
    field(obj, keys).and_then(opt_str)
}

/// OPNsense sometimes renders list-ish fields as objects keyed by value or
/// comma-separated strings; normalize to a readable string.
fn net_field(obj: &Value, keys: &[&str]) -> String {
    match field(obj, keys) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Object(map)) => map.keys().cloned().collect::<Vec<_>>().join(","),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(opt_str)
            .collect::<Vec<_>>()
            .join(","),
        _ => String::new(),
    }
}

fn i64_field(obj: &Value, keys: &[&str]) -> Option<i64> {
    field(obj, keys).and_then(opt_i64)
}

fn rows_of(value: &Value) -> Vec<&Value> {
    field(value, &["rows", "items"])
        .and_then(Value::as_array)
        .map(|rows| rows.iter().collect())
        .unwrap_or_default()
}

fn total_of(value: &Value, fallback: i64) -> i64 {
    i64_field(value, &["total", "rowCount"]).unwrap_or(fallback)
}

// ---------------------------------------------------------------------------
// Endpoint implementations
// ---------------------------------------------------------------------------

#[async_trait]
impl OpnsenseApi for OpnsenseClient {
    async fn get_system_status(&self) -> Result<(), OpnsenseError> {
        self.get_json("core/system/status", &[]).await?;
        Ok(())
    }

    async fn search_rules(&self) -> Result<RuleSearchPage, OpnsenseError> {
        let value = self
            .get_json(
                "firewall/filter/searchRule",
                &[("show_all", "1".to_owned())],
            )
            .await?;
        let rules = rows_of(&value)
            .into_iter()
            .map(parse_rule_row)
            .collect::<Vec<_>>();
        Ok(RuleSearchPage {
            total: total_of(&value, rules.len() as i64),
            rules,
        })
    }

    async fn rule_stats(&self) -> Result<Vec<(String, RuleStat)>, OpnsenseError> {
        let value = self.get_json("firewall/filter_util/ruleStats", &[]).await?;
        Ok(parse_rule_stats(&value))
    }

    async fn list_rule_ids(&self) -> Result<Vec<PfRuleId>, OpnsenseError> {
        let value = self
            .get_json("diagnostics/firewall/listRuleIds", &[])
            .await?;
        Ok(parse_rule_ids(&value))
    }

    async fn interfaces_info(&self) -> Result<InterfaceInfoPage, OpnsenseError> {
        let value = self
            .get_json("interfaces/overview/interfacesInfo", &[])
            .await?;
        let mut interfaces = rows_of(&value)
            .into_iter()
            .map(parse_interface_row)
            .collect::<Vec<_>>();
        join_lagg_members(&mut interfaces);
        Ok(InterfaceInfoPage {
            total: total_of(&value, interfaces.len() as i64),
            interfaces,
        })
    }

    async fn interface_statistics(&self) -> Result<Vec<OpnsenseInterfaceDto>, OpnsenseError> {
        let value = self
            .get_json("diagnostics/interface/get_interface_statistics", &[])
            .await?;
        Ok(parse_interface_statistics(&value))
    }

    async fn pf_states(&self) -> Result<OpnsensePfStatesDto, OpnsenseError> {
        let value = self.get_json("diagnostics/firewall/pfStates", &[]).await?;
        Ok(parse_pf_states(&value))
    }

    async fn system_resources(&self) -> Result<OpnsenseResourcesDto, OpnsenseError> {
        let value = self
            .get_json("diagnostics/system/system_resources", &[])
            .await?;
        Ok(parse_system_resources(&value))
    }

    async fn system_time(&self) -> Result<SystemTimePayload, OpnsenseError> {
        let value = self.get_json("diagnostics/system/system_time", &[]).await?;
        Ok(parse_system_time(&value))
    }

    async fn cpu_usage_sample(&self) -> Result<f64, OpnsenseError> {
        let sample = tokio::time::timeout(DEFAULT_REQUEST_TIMEOUT, self.cpu_usage_sample_inner())
            .await
            .map_err(|_| {
                OpnsenseError::Timeout("CPU usage stream produced no sample".to_owned())
            })??;
        Ok(sample)
    }

    async fn wireguard_show(&self) -> Result<OpnsenseWireguardStatusDto, OpnsenseError> {
        let value = self.get_json("wireguard/service/show", &[]).await?;
        Ok(parse_wireguard_show(&value))
    }

    async fn openvpn_sessions(&self) -> Result<Vec<OpnsenseOpenvpnSessionDto>, OpnsenseError> {
        let value = self.get_json("openvpn/service/searchSessions", &[]).await?;
        Ok(rows_of(&value).into_iter().map(parse_openvpn_row).collect())
    }

    async fn ipsec_phase1(&self) -> Result<Vec<OpnsenseIpsecSessionDto>, OpnsenseError> {
        let value = self.get_json("ipsec/sessions/searchPhase1", &[]).await?;
        Ok(rows_of(&value).into_iter().map(parse_ipsec_row).collect())
    }

    async fn tailscale_status(&self) -> Result<OpnsenseTailscaleStatusDto, OpnsenseError> {
        let value = self.get_json("tailscale/status/status", &[]).await?;
        parse_tailscale_status(&value)
    }

    async fn tailscale_service_status(&self) -> Result<Option<bool>, OpnsenseError> {
        let value = self.get_json("tailscale/service/status", &[]).await?;
        Ok(field(&value, &["status"]).and_then(opt_bool))
    }

    async fn firmware_info(&self) -> Result<FirmwareInfoPayload, OpnsenseError> {
        let value = self.get_json("core/firmware/info", &[]).await?;
        Ok(parse_firmware_info(&value))
    }

    async fn backup_revisions(&self) -> Result<Vec<OpnsenseRevisionDto>, OpnsenseError> {
        let value = self.get_json("core/backup/backups/this", &[]).await?;
        let items: Vec<&Value> = match &value {
            Value::Array(items) => items.iter().collect(),
            other => rows_of(other),
        };
        Ok(items.into_iter().map(parse_revision_row).collect())
    }

    async fn download_revision(&self, id: &str) -> Result<Vec<u8>, OpnsenseError> {
        let response = self
            .request(&format!("core/backup/download/this/{id}"))
            .timeout(DOWNLOAD_REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|e| classify_transport(&e))?;
        let response = check_status(response)?;
        read_bounded_bytes(response, MAX_REVISION_BYTES).await
    }

    async fn open_log_stream(&self) -> Result<SseByteStream, OpnsenseError> {
        self.get_stream("diagnostics/firewall/streamLog").await
    }
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

fn parse_rule_row(row: &Value) -> OpnsenseRuleDto {
    OpnsenseRuleDto {
        uuid: str_field(row, &["uuid"]).unwrap_or_default(),
        action: str_field(row, &["action"]).unwrap_or_default(),
        interface: net_field(row, &["interface", "interfaces", "interfacename"]),
        description: str_field(row, &["description", "descr"]).unwrap_or_default(),
        source: net_field(row, &["source_net", "source", "src"]),
        destination: net_field(row, &["destination_net", "destination", "dst"]),
        protocol: str_field(row, &["protocol", "proto"]).unwrap_or_default(),
        enabled: field(row, &["enabled"]).and_then(opt_bool).unwrap_or(true),
        // Counters are joined separately from ruleStats by the monitor.
        packets: None,
        evaluations: None,
        bytes: None,
        states: None,
    }
}

fn parse_rule_stats(value: &Value) -> Vec<(String, RuleStat)> {
    // FilterUtilController::ruleStatsAction returns
    // {"status":"ok","stats":{"<uuid>":{...}}}; accept the legacy
    // "items" and direct-map shapes as well.
    let map = match value {
        Value::Object(map) => map
            .get("stats")
            .or_else(|| map.get("items"))
            .and_then(Value::as_object)
            .unwrap_or(map),
        _ => return Vec::new(),
    };
    map.iter()
        .filter_map(|(uuid, stats)| {
            let packets = i64_field(stats, &["packets", "pkts"]);
            let evaluations = i64_field(stats, &["evaluations", "evals"]);
            let bytes = i64_field(stats, &["bytes"]);
            let states = i64_field(stats, &["states", "active_states"]);
            if packets.is_none() && evaluations.is_none() && bytes.is_none() && states.is_none() {
                return None;
            }
            Some((
                uuid.clone(),
                RuleStat {
                    packets,
                    evaluations,
                    bytes,
                    states,
                },
            ))
        })
        .collect()
}

fn parse_rule_ids(value: &Value) -> Vec<PfRuleId> {
    let items: Vec<&Value> = match value {
        Value::Array(items) => items.iter().collect(),
        other => rows_of(other),
    };
    items
        .into_iter()
        .filter_map(|item| match item {
            Value::String(id) => Some(PfRuleId {
                id: id.clone(),
                description: None,
            }),
            Value::Object(_) => {
                let id = str_field(item, &["id", "rule_id", "label"])?;
                Some(PfRuleId {
                    id,
                    description: str_field(item, &["description", "descr"]),
                })
            }
            _ => None,
        })
        .collect()
}

fn parse_interface_row(row: &Value) -> OpnsenseInterfaceDto {
    // Real payloads key the device as "device"; older spellings kept.
    let name = str_field(
        row,
        &["device", "name", "identifier", "ifname", "interface"],
    )
    .unwrap_or_default();
    let (ipv4, ipv6) = parse_addresses(row);
    let macaddr = str_field(row, &["macaddr", "mac"])
        // ifconfig reports 00:00:00:00:00:00 for virtual devices — noise.
        .filter(|mac| mac != "00:00:00:00:00:00");
    let lagg = field(row, &["laggport"])
        .and_then(Value::as_object)
        .map(|ports| {
            OpnsenseLaggDto {
                proto: str_field(row, &["laggproto"]),
                members: ports
                    .iter()
                    .map(|(port, info)| OpnsenseLaggMemberDto {
                        name: port.clone(),
                        state: field(info, &["state"])
                            .and_then(Value::as_array)
                            .map(|flags| flags.iter().filter_map(opt_str).collect())
                            .unwrap_or_default(),
                        // Filled by join_lagg_members from the port's own row.
                        link: None,
                        media: None,
                    })
                    .collect(),
            }
        });
    OpnsenseInterfaceDto {
        name,
        identifier: str_field(row, &["identifier"]),
        description: str_field(row, &["description", "descr"]),
        enabled: field(row, &["enabled"]).and_then(opt_bool).unwrap_or(true),
        link: str_field(row, &["link", "status"]).map(|s| s.to_ascii_lowercase()),
        ipv4,
        ipv6,
        macaddr,
        mtu: i64_field(row, &["mtu"]),
        media: str_field(row, &["media"]),
        lagg,
        rx_bytes: None,
        tx_bytes: None,
        rx_packets: None,
        tx_packets: None,
    }
}

/// Members of a lagg carry no speed in the parent row; each member appears
/// as its own interface row with media/link. Join by exact device name.
fn join_lagg_members(interfaces: &mut [OpnsenseInterfaceDto]) {
    let details: Vec<(String, Option<String>, Option<String>)> = interfaces
        .iter()
        .map(|iface| (iface.name.clone(), iface.link.clone(), iface.media.clone()))
        .collect();
    for iface in interfaces.iter_mut() {
        let Some(lagg) = iface.lagg.as_mut() else {
            continue;
        };
        for member in &mut lagg.members {
            if let Some((_, link, media)) = details.iter().find(|(name, _, _)| *name == member.name)
            {
                member.link = link.clone();
                member.media = media.clone();
            }
        }
        // Deterministic member order.
        lagg.members.sort_by(|a, b| a.name.cmp(&b.name));
    }
}

/// Addresses come as primary strings (`addr4`/`addr6`, "" when absent)
/// plus `ipv4`/`ipv6` arrays of objects carrying `ipaddr` with CIDR.
fn parse_addresses(row: &Value) -> (Vec<String>, Vec<String>) {
    let collect = |primary_keys: &[&str], array_keys: &[&str]| -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        if let Some(primary) = str_field(row, primary_keys) {
            out.push(primary);
        }
        if let Some(items) = field(row, array_keys).and_then(Value::as_array) {
            for item in items {
                let addr = match item {
                    Value::String(_) => opt_str(item),
                    Value::Object(_) => str_field(item, &["ipaddr", "address", "addr"]),
                    _ => None,
                };
                if let Some(addr) = addr {
                    if !out.contains(&addr) {
                        out.push(addr);
                    }
                }
            }
        }
        out
    };
    (
        collect(&["addr4"], &["ipv4", "ipv4_addresses"]),
        collect(&["addr6"], &["ipv6", "ipv6_addresses"]),
    )
}

fn parse_interface_statistics(value: &Value) -> Vec<OpnsenseInterfaceDto> {
    // Real shape (Diagnostics InterfaceController::getInterfaceStatisticsAction):
    // {"statistics": {"[WAN] (igc0) / a8:b8:e0:02:5a:53": {"name": "igc0",
    //  "received-bytes": 143428806637, ...}, "[WAN] (igc0) / 100.64.104.184":
    //  {"name": "igc0", ...}, ...}}
    // The MAP KEY is a display label, not the device name — the node's own
    // `name` field carries the device ("igc0") that the monitor merges on.
    // One device appears once PER ADDRESS (link layer + each IP); the
    // link-layer ("<Link#N>") entry holds the interface totals, so merging
    // takes the MAX of each counter — never the sum, which would double
    // count traffic.
    let stats = match field(value, &["statistics"]).and_then(Value::as_object) {
        Some(map) => map,
        None => return Vec::new(),
    };
    let mut by_device: std::collections::HashMap<String, OpnsenseInterfaceDto> =
        std::collections::HashMap::new();
    for (key, entry) in stats {
        let find = |keys: &[&str]| -> Option<i64> {
            keys.iter().find_map(|k| entry.get(*k)).and_then(opt_i64)
        };
        let name = str_field(entry, &["name"])
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| device_from_stats_label(key));
        let merged = by_device
            .entry(name)
            .or_insert_with_key(|name| OpnsenseInterfaceDto {
                name: name.clone(),
                identifier: None,
                description: None,
                enabled: true,
                link: None,
                ipv4: Vec::new(),
                ipv6: Vec::new(),
                macaddr: None,
                mtu: None,
                media: None,
                lagg: None,
                rx_bytes: None,
                tx_bytes: None,
                rx_packets: None,
                tx_packets: None,
            });
        let keep_max = |slot: &mut Option<i64>, candidate: Option<i64>| {
            if let Some(v) = candidate {
                if slot.is_none_or(|cur| v > cur) {
                    *slot = Some(v);
                }
            }
        };
        keep_max(
            &mut merged.rx_bytes,
            find(&[
                "bytes received",
                "received-bytes",
                "rx-bytes",
                "rx_bytes",
                "ibytes",
            ]),
        );
        keep_max(
            &mut merged.tx_bytes,
            find(&[
                "bytes transmitted",
                "sent-bytes",
                "tx-bytes",
                "tx_bytes",
                "obytes",
            ]),
        );
        keep_max(
            &mut merged.rx_packets,
            find(&[
                "packets received",
                "received-packets",
                "rx-packets",
                "rx_packets",
                "ipackets",
            ]),
        );
        keep_max(
            &mut merged.tx_packets,
            find(&[
                "packets transmitted",
                "sent-packets",
                "tx-packets",
                "tx_packets",
                "opackets",
            ]),
        );
    }
    by_device.into_values().collect()
}

/// Device name out of a statistics map label: "[WAN] (igc0) / 10.0.0.1"
/// → "igc0"; "[igc2] / -" → "igc2". Anything unrecognized passes through
/// unchanged so a bare device name still merges.
fn device_from_stats_label(label: &str) -> String {
    if let Some(start) = label.rfind('(') {
        if let Some(end) = label[start..].find(')') {
            let inner = &label[start + 1..start + end];
            if !inner.is_empty() {
                return inner.to_owned();
            }
        }
    }
    label
        .strip_prefix('[')
        .and_then(|rest| rest.split(']').next())
        .filter(|inner| !inner.is_empty())
        .unwrap_or(label)
        .to_owned()
}

fn parse_pf_states(value: &Value) -> OpnsensePfStatesDto {
    // Versions differ: some return a search-style page (current = total
    // rows), newer ones expose explicit counters. Take what exists.
    let current = i64_field(value, &["current", "count"])
        .or_else(|| i64_field(value, &["total"]))
        .or_else(|| {
            value
                .get("states")
                .and_then(Value::as_array)
                .map(|a| a.len() as i64)
        });
    let max = i64_field(value, &["max", "limit", "max_states"]);
    OpnsensePfStatesDto { current, max }
}

/// `system_resources` reports MEMORY ONLY (CPU comes from the
/// `cpu_usage/stream` SSE feed, uptime/load from `system_time`); memory
/// values may be the localized string "N/A" when sysctl data is missing.
fn parse_system_resources(value: &Value) -> OpnsenseResourcesDto {
    let memory = field(value, &["memory", "mem"]);
    let (mem_used, mem_total) = match memory {
        Some(m) => (
            i64_field(m, &["used", "used_bytes"]),
            i64_field(m, &["total", "total_bytes"]),
        ),
        None => (None, None),
    };
    OpnsenseResourcesDto {
        cpu_usage_percent: None,
        mem_used_bytes: mem_used,
        mem_total_bytes: mem_total,
        uptime: None,
        loadavg: None,
    }
}

fn parse_system_time(value: &Value) -> SystemTimePayload {
    let clean = |keys: &[&str]| str_field(value, keys).filter(|s| !s.eq_ignore_ascii_case("n/a"));
    SystemTimePayload {
        uptime: clean(&["uptime"]),
        loadavg: clean(&["loadavg"]),
    }
}

fn parse_wireguard_show(value: &Value) -> OpnsenseWireguardStatusDto {
    let rows = rows_of(value);
    let mut interfaces: Vec<OpnsenseWireguardInterfaceDto> = Vec::new();
    // Device ("if" = wg0) alongside the configured name, for peer joins.
    let mut devices: Vec<String> = Vec::new();
    for row in &rows {
        let kind = str_field(row, &["type"]).unwrap_or_default();
        if kind == "interface" {
            devices.push(str_field(row, &["if"]).unwrap_or_default());
            interfaces.push(OpnsenseWireguardInterfaceDto {
                name: str_field(row, &["name", "if", "ifname"]).unwrap_or_default(),
                peers: Vec::new(),
            });
        }
    }
    for row in &rows {
        let kind = str_field(row, &["type"]).unwrap_or_default();
        if kind != "peer" {
            continue;
        }
        // Peer rows attach to their interface via "if" (the wgN device),
        // NOT "ifname" (which is the server instance name).
        let parent = str_field(row, &["if", "interface", "ifname"]).unwrap_or_default();
        let name = str_field(row, &["name", "descr", "description"])
            .or_else(|| {
                str_field(row, &["public-key", "public_key", "pubkey"])
                    .map(|key| key.chars().take(12).collect::<String>())
            })
            .unwrap_or_else(|| "(peer)".to_owned());
        let handshake_age = i64_field(row, &["latest-handshake-age", "latest_handshake_age"])
            .or_else(|| i64_field(row, &["latest-handshake", "latest_handshake"]));
        // The firewall computes peer-status itself (online ≤300s); trust
        // it when present, derive otherwise.
        let status = str_field(row, &["peer-status", "peer_status"])
            .map(|s| match s.as_str() {
                "online" => "active".to_owned(),
                other => other.to_owned(),
            })
            .unwrap_or_else(|| match handshake_age {
                None => "never".to_owned(),
                Some(age) if age <= 300 => "active".to_owned(),
                Some(_) => "stale".to_owned(),
            });
        let endpoint = str_field(row, &["endpoint"]).filter(|e| !e.is_empty() && e != "(none)");
        let peer = OpnsenseWireguardPeerDto {
            name,
            handshake_age_secs: handshake_age,
            status,
            endpoint,
            transfer_rx_bytes: i64_field(row, &["transfer-rx", "transfer_rx"]),
            transfer_tx_bytes: i64_field(row, &["transfer-tx", "transfer_tx"]),
        };
        // Peers carry the device; interface rows may only expose their
        // configured name — match either.
        let idx = interfaces
            .iter()
            .enumerate()
            .find(|(i, iface)| {
                iface.name == parent
                    || devices.get(*i).is_some_and(|device| *device == parent)
                    || str_field(row, &["ifname"]).as_deref() == Some(iface.name.as_str())
            })
            .map(|(i, _)| i);
        match idx {
            Some(i) => interfaces[i].peers.push(peer),
            None => {
                devices.push(parent.clone());
                interfaces.push(OpnsenseWireguardInterfaceDto {
                    name: parent,
                    peers: vec![peer],
                });
            }
        }
    }
    OpnsenseWireguardStatusDto { interfaces }
}

fn parse_openvpn_row(row: &Value) -> OpnsenseOpenvpnSessionDto {
    let kind = str_field(row, &["type", "mode", "role"]).unwrap_or_else(|| "unknown".to_owned());
    // Client rows carry status "connected"; server rows "ok" when the
    // management socket answered. Older payloads use a connected bool.
    let connected = field(row, &["connected"])
        .and_then(opt_bool)
        .or_else(|| {
            str_field(row, &["status"]).map(|s| matches!(s.as_str(), "connected" | "ok" | "up"))
        })
        .unwrap_or(false);
    OpnsenseOpenvpnSessionDto {
        name: str_field(row, &["common_name", "name", "id"]).unwrap_or_default(),
        description: str_field(row, &["description", "descr"]),
        kind,
        connected,
        real_address: str_field(row, &["real_address"]),
        virtual_address: str_field(row, &["virtual_address"]),
        connected_since: str_field(row, &["connected_since"]),
        bytes_received: i64_field(row, &["bytes_received"]),
        bytes_sent: i64_field(row, &["bytes_sent"]),
    }
}

fn parse_ipsec_row(row: &Value) -> OpnsenseIpsecSessionDto {
    let connected = field(row, &["connected"])
        .and_then(opt_bool)
        .unwrap_or(false);
    OpnsenseIpsecSessionDto {
        name: str_field(row, &["name", "connection", "id"]).unwrap_or_default(),
        description: str_field(row, &["phase1desc", "description", "descr"]),
        connected,
        remote_host: str_field(row, &["remote-host", "remote_host"]).filter(|h| !h.is_empty()),
        version: str_field(row, &["version"]),
    }
}

fn parse_tailscale_status(value: &Value) -> Result<OpnsenseTailscaleStatusDto, OpnsenseError> {
    if !value.is_object() {
        return Err(OpnsenseError::Parse(
            "tailscale status is not a JSON object".to_owned(),
        ));
    }
    let ips_of = |node: &Value| -> Vec<String> {
        field(node, &["TailscaleIPs", "tailscaleIPs", "ips"])
            .and_then(Value::as_array)
            .map(|ips| ips.iter().filter_map(opt_str).collect())
            .unwrap_or_default()
    };
    let self_node = field(value, &["Self", "self"]);
    let peers = match field(value, &["Peer", "peers"]) {
        Some(Value::Object(map)) => map
            .values()
            .map(|peer| OpnsenseTailscalePeerDto {
                name: str_field(peer, &["HostName", "hostName", "hostname", "name"])
                    .unwrap_or_default(),
                ips: ips_of(peer),
                online: field(peer, &["Online", "online"]).and_then(opt_bool),
                last_seen: str_field(peer, &["LastSeen", "lastSeen"]),
            })
            .collect(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|peer| OpnsenseTailscalePeerDto {
                name: str_field(peer, &["HostName", "hostName", "hostname", "name"])
                    .unwrap_or_default(),
                ips: ips_of(peer),
                online: field(peer, &["Online", "online"]).and_then(opt_bool),
                last_seen: str_field(peer, &["LastSeen", "lastSeen"]),
            })
            .collect(),
        _ => Vec::new(),
    };
    Ok(OpnsenseTailscaleStatusDto {
        backend_state: str_field(value, &["BackendState", "backendState"]),
        service_running: None, // merged from service/status by the caller
        hostname: self_node.and_then(|s| str_field(s, &["HostName", "hostName", "hostname"])),
        own_ips: self_node.map(ips_of).unwrap_or_default(),
        peers,
    })
}

/// `core/firmware/info`: installed version/ABI under `product` (newer
/// releases) or top level, plus the installed plugin list.
fn parse_firmware_info(value: &Value) -> FirmwareInfoPayload {
    let product = value.get("product");
    let pick = |keys: &[&str]| {
        product
            .and_then(|p| field(p, keys))
            .or_else(|| field(value, keys))
            .and_then(opt_str)
    };
    let plugins = field(value, &["plugin", "plugins"])
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| {
                    field(item, &["installed"])
                        .and_then(opt_bool)
                        .unwrap_or(false)
                })
                .filter_map(|item| {
                    let name = str_field(item, &["name"])?;
                    Some((name, str_field(item, &["version"])))
                })
                .collect()
        })
        .unwrap_or_default();
    let cached_plugin_upgrades = product
        .and_then(|p| p.get("product_check"))
        .and_then(|check| check.get("upgrade_packages"))
        .and_then(Value::as_array)
        .map(|packages| {
            packages
                .iter()
                .filter_map(|package| {
                    let name = str_field(package, &["name"])?;
                    let version = str_field(package, &["new_version"])?;
                    name.starts_with("os-").then_some((name, version))
                })
                .collect()
        })
        .unwrap_or_default();
    let changelog = field(value, &["changelog"])
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let version = str_field(item, &["version"])?;
                    Some(FirmwareChangelogItem {
                        series: str_field(item, &["series"]),
                        version,
                        date: str_field(item, &["date"]),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    FirmwareInfoPayload {
        product_version: str_field(value, &["product_version"]).or_else(|| {
            pick(&[
                "product_version",
                "version",
                "CORE_PKGVERSION",
                "CORE_VERSION",
            ])
        }),
        abi: pick(&["product_abi", "abi", "CORE_ABI"]),
        plugins,
        cached_plugin_upgrades,
        changelog,
    }
}

fn parse_revision_row(row: &Value) -> OpnsenseRevisionDto {
    OpnsenseRevisionDto {
        id: str_field(row, &["id", "filename", "name"]).unwrap_or_default(),
        time_iso: str_field(row, &["time_iso", "timeIso", "time"]),
        description: str_field(row, &["description", "descr"]),
        filesize: i64_field(row, &["filesize", "size"]),
    }
}

/// Merge per-UUID counters into rule rows (join by UUID ONLY — never by
/// numeric PF ID). Returns IDs/stats that matched no rule.
pub fn join_rule_counters(
    rules: &mut [OpnsenseRuleDto],
    stats: &[(String, RuleStat)],
    pf_ids: &[PfRuleId],
) -> Vec<OpnsenseUnmappedRuleDto> {
    let mut unmapped = Vec::new();
    for (uuid, stat) in stats {
        match rules.iter_mut().find(|r| r.uuid == *uuid) {
            Some(rule) => {
                rule.packets = stat.packets;
                rule.evaluations = stat.evaluations;
                rule.bytes = stat.bytes;
                rule.states = stat.states;
            }
            None => unmapped.push(OpnsenseUnmappedRuleDto {
                id: uuid.clone(),
                description: None,
                packets: stat.packets,
                bytes: stat.bytes,
                states: stat.states,
            }),
        }
    }
    for pf_id in pf_ids {
        let known = rules.iter().any(|r| r.uuid == pf_id.id)
            || unmapped
                .iter()
                .any(|u: &OpnsenseUnmappedRuleDto| u.id == pf_id.id);
        if !known {
            unmapped.push(OpnsenseUnmappedRuleDto {
                id: pf_id.id.clone(),
                description: pf_id.description.clone(),
                packets: None,
                bytes: None,
                states: None,
            });
        }
    }
    unmapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_interface_row_reads_real_interfaces_info_shape() {
        let row = json!({
            "device": "igb0",
            "description": "WAN",
            "identifier": "wan",
            "enabled": "1",
            "status": "up",
            "macaddr": "aa:bb:cc:dd:ee:ff",
            "mtu": "1500",
            "media": "1000baseT <full-duplex>",
            "addr4": "198.51.100.1/24",
            "addr6": "",
            "ipv4": [{"ipaddr": "198.51.100.1/24"}, {"ipaddr": "198.51.100.2/24"}],
            "ipv6": [{"ipaddr": "2001:db8::1/64"}]
        });
        let iface = parse_interface_row(&row);
        assert_eq!(iface.name, "igb0");
        assert_eq!(iface.identifier.as_deref(), Some("wan"));
        assert_eq!(iface.link.as_deref(), Some("up"));
        assert_eq!(
            iface.ipv4,
            vec!["198.51.100.1/24".to_owned(), "198.51.100.2/24".to_owned()],
            "primary first, no duplicates"
        );
        assert_eq!(iface.ipv6, vec!["2001:db8::1/64".to_owned()]);
        assert_eq!(iface.macaddr.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(iface.mtu, Some(1500));
        assert_eq!(iface.media.as_deref(), Some("1000baseT <full-duplex>"));
        assert!(iface.lagg.is_none());
    }

    #[test]
    fn parse_interface_row_drops_zero_mac_and_empty_addresses() {
        let row = json!({
            "device": "lo0",
            "macaddr": "00:00:00:00:00:00",
            "addr4": "",
            "addr6": ""
        });
        let iface = parse_interface_row(&row);
        assert!(iface.macaddr.is_none());
        assert!(iface.ipv4.is_empty());
        assert!(iface.ipv6.is_empty());
    }

    #[test]
    fn parse_interface_statistics_resolves_device_from_labelled_keys() {
        // Real payload (OPNsense 25.7): one entry PER ADDRESS, keyed by
        // display label; counters use ifconfig-style "received-bytes" keys;
        // the link-layer entry carries the interface totals.
        let value = json!({
            "statistics": {
                "[WAN] (igc0) / a8:b8:e0:02:5a:53": {
                    "name": "igc0", "network": "<Link#1>", "address": "a8:b8:e0:02:5a:53",
                    "received-packets": 123488770, "received-bytes": 143428806637i64,
                    "sent-packets": 49507047, "sent-bytes": 29502684429i64
                },
                "[WAN] (igc0) / 100.64.104.184": {
                    "name": "igc0", "address": "100.64.104.184",
                    "received-packets": 1142069, "received-bytes": 564088745,
                    "sent-packets": 1710989, "sent-bytes": 1037328038
                },
                "[igc2] / -": {
                    "received-bytes": 1
                },
                "lo0": {
                    "bytes received": 2, "packets received": 3
                }
            }
        });
        let stats = parse_interface_statistics(&value);
        let by_name = |n: &str| stats.iter().find(|s| s.name == n).expect(n);
        // Interface totals come from the link-layer entry (the max), never
        // a sum over the per-address entries.
        let wan = by_name("igc0");
        assert_eq!(wan.rx_bytes, Some(143428806637));
        assert_eq!(wan.tx_bytes, Some(29502684429));
        assert_eq!(wan.rx_packets, Some(123488770));
        assert_eq!(wan.tx_packets, Some(49507047));
        // Label without "(device)" falls back to the bracketed token.
        let unassigned = by_name("igc2");
        assert_eq!(unassigned.rx_bytes, Some(1));
        assert_eq!(unassigned.tx_bytes, None);
        // Bare device-name keys (older builds) pass through unchanged.
        let bare = by_name("lo0");
        assert_eq!(bare.rx_bytes, Some(2));
        assert_eq!(bare.rx_packets, Some(3));
        assert!(
            !stats.iter().any(|s| s.name.contains('[')),
            "no display label must leak into the device name"
        );
        assert_eq!(stats.len(), 3, "per-address entries merge into one row");
    }

    #[test]
    fn lagg_members_join_link_and_media_from_member_rows() {
        let mut interfaces = vec![
            parse_interface_row(&json!({
                "device": "lagg0",
                "description": "UPLINK",
                "status": "up",
                "laggproto": "lacp",
                "laggport": {
                    "em1": {"state": ["attached", "active"]},
                    "em0": {"state": ["attached"]}
                }
            })),
            parse_interface_row(&json!({
                "device": "em0", "status": "up", "media": "1000baseT <full-duplex>"
            })),
            parse_interface_row(&json!({
                "device": "em1", "status": "no carrier"
            })),
        ];
        join_lagg_members(&mut interfaces);
        let lagg = interfaces[0].lagg.as_ref().expect("lagg");
        assert_eq!(lagg.proto.as_deref(), Some("lacp"));
        assert_eq!(lagg.members.len(), 2);
        // Sorted by member name; link/media joined from the member rows.
        assert_eq!(lagg.members[0].name, "em0");
        assert_eq!(
            lagg.members[0].media.as_deref(),
            Some("1000baseT <full-duplex>")
        );
        assert_eq!(lagg.members[0].link.as_deref(), Some("up"));
        assert_eq!(lagg.members[1].name, "em1");
        assert_eq!(lagg.members[1].link.as_deref(), Some("no carrier"));
        assert!(lagg.members[1].media.is_none());
    }

    #[test]
    fn parse_system_time_reads_uptime_and_loadavg() {
        let value = json!({
            "uptime": "2 days, 14:03:11",
            "datetime": "Thu Oct 9 12:00:00 CEST 2026",
            "loadavg": "0.12, 0.08, 0.06"
        });
        let time = parse_system_time(&value);
        assert_eq!(time.uptime.as_deref(), Some("2 days, 14:03:11"));
        assert_eq!(time.loadavg.as_deref(), Some("0.12, 0.08, 0.06"));
        // Localized "N/A" must not reach the UI.
        let none = parse_system_time(&json!({"uptime": "N/A", "loadavg": "N/A"}));
        assert!(none.uptime.is_none());
        assert!(none.loadavg.is_none());
    }

    #[test]
    fn parse_wireguard_show_joins_peers_by_device_and_trusts_peer_status() {
        let value = json!({
            "rows": [
                {"type": "interface", "if": "wg0", "name": "Roadwarriors", "status": "up"},
                {"type": "interface", "if": "wg1", "name": "SiteB", "status": "up"},
                {"type": "peer", "if": "wg0", "ifname": "Roadwarriors", "name": "laptop",
                 "peer-status": "online", "latest-handshake-age": 42,
                 "endpoint": "203.0.113.9:51820", "transfer-rx": 1024, "transfer-tx": 2048},
                {"type": "peer", "if": "wg1", "ifname": "SiteB", "name": "branch",
                 "peer-status": "stale", "latest-handshake-age": 900, "endpoint": ""}
            ]
        });
        let status = parse_wireguard_show(&value);
        assert_eq!(status.interfaces.len(), 2);
        assert_eq!(status.interfaces[0].name, "Roadwarriors");
        assert_eq!(status.interfaces[0].peers.len(), 1);
        let peer = &status.interfaces[0].peers[0];
        assert_eq!(peer.name, "laptop");
        assert_eq!(peer.status, "active");
        assert_eq!(peer.handshake_age_secs, Some(42));
        assert_eq!(peer.endpoint.as_deref(), Some("203.0.113.9:51820"));
        assert_eq!(peer.transfer_rx_bytes, Some(1024));
        let stale = &status.interfaces[1].peers[0];
        assert_eq!(stale.status, "stale");
        assert!(stale.endpoint.is_none(), "empty endpoint stays None");
    }

    #[test]
    fn parse_wireguard_show_derives_status_when_peer_status_absent() {
        let value = json!({
            "rows": [
                {"type": "peer", "if": "wg0", "name": "new", "latest-handshake-age": null},
                {"type": "peer", "if": "wg0", "name": "old", "latest-handshake-age": 3600}
            ]
        });
        let status = parse_wireguard_show(&value);
        assert_eq!(status.interfaces[0].peers[0].status, "never");
        assert_eq!(status.interfaces[0].peers[1].status, "stale");
    }

    #[test]
    fn parse_openvpn_row_reads_real_session_shape() {
        let row = json!({
            "type": "client",
            "is_client": true,
            "common_name": "roadwarrior-1",
            "description": "Road warriors",
            "status": "connected",
            "real_address": "203.0.113.7:1194",
            "virtual_address": "10.8.0.2",
            "connected_since": "2026-10-09 08:15:00",
            "bytes_received": 12345,
            "bytes_sent": 6789
        });
        let session = parse_openvpn_row(&row);
        assert_eq!(session.name, "roadwarrior-1");
        assert_eq!(session.kind, "client");
        assert!(session.connected);
        assert_eq!(session.real_address.as_deref(), Some("203.0.113.7:1194"));
        assert_eq!(session.virtual_address.as_deref(), Some("10.8.0.2"));
        assert_eq!(session.bytes_received, Some(12345));
    }

    #[test]
    fn parse_ipsec_row_reads_real_phase1_shape() {
        let row = json!({
            "name": "site-to-site",
            "phase1desc": "Branch tunnel",
            "connected": true,
            "remote-host": "203.0.113.1",
            "version": "IKEv2"
        });
        let session = parse_ipsec_row(&row);
        assert_eq!(session.name, "site-to-site");
        assert_eq!(session.description.as_deref(), Some("Branch tunnel"));
        assert!(session.connected);
        assert_eq!(session.remote_host.as_deref(), Some("203.0.113.1"));
        assert_eq!(session.version.as_deref(), Some("IKEv2"));
    }

    #[test]
    fn parse_rule_row_accepts_string_numbers_and_alt_spellings() {
        let row = json!({
            "uuid": "abc-123",
            "action": "pass",
            "interface": {"wan": "wan"},
            "descr": "allow web",
            "source_net": "any",
            "destination_net": "lan net",
            "protocol": "tcp",
            "enabled": "1"
        });
        let rule = parse_rule_row(&row);
        assert_eq!(rule.uuid, "abc-123");
        assert_eq!(rule.interface, "wan");
        assert_eq!(rule.description, "allow web");
        assert!(rule.enabled);
        assert_eq!(rule.packets, None);
    }

    #[test]
    fn parse_rule_stats_accepts_nested_items_and_string_counts() {
        let value = json!({
            "status": "ok",
            "stats": {
                "u1": {"packets": "12", "evaluations": 30, "bytes": "900", "states": "2"},
                "u2": {"packets": 0}
            }
        });
        let stats = parse_rule_stats(&value);
        assert_eq!(stats.len(), 2);
        let (uuid, stat) = stats.iter().find(|(u, _)| u == "u1").unwrap();
        assert_eq!(uuid, "u1");
        assert_eq!(stat.packets, Some(12));
        assert_eq!(stat.evaluations, Some(30));
        assert_eq!(stat.bytes, Some(900));
        assert_eq!(stat.states, Some(2));
        let (_, stat) = stats.iter().find(|(u, _)| u == "u2").unwrap();
        assert_eq!(stat.packets, Some(0));
        assert_eq!(stat.evaluations, None);
    }

    #[test]
    fn join_rule_counters_joins_by_uuid_only_and_collects_unmapped() {
        let mut rules = vec![
            parse_rule_row(&json!({"uuid": "u1", "action": "pass"})),
            parse_rule_row(&json!({"uuid": "u2", "action": "block"})),
        ];
        let stats = vec![
            (
                "u1".to_owned(),
                RuleStat {
                    packets: Some(12),
                    evaluations: None,
                    bytes: None,
                    states: None,
                },
            ),
            (
                "u2".to_owned(),
                RuleStat {
                    packets: Some(0),
                    evaluations: None,
                    bytes: None,
                    states: None,
                },
            ),
            (
                "ghost".to_owned(),
                RuleStat {
                    packets: Some(5),
                    evaluations: None,
                    bytes: None,
                    states: None,
                },
            ),
        ];
        let pf_ids = vec![
            PfRuleId {
                id: "u2".to_owned(),
                description: None,
            },
            PfRuleId {
                id: "pf-only".to_owned(),
                description: Some("anchor".to_owned()),
            },
        ];
        let unmapped = join_rule_counters(&mut rules, &stats, &pf_ids);
        assert_eq!(rules[0].packets, Some(12));
        assert_eq!(rules[1].packets, Some(0));
        let ids: Vec<&str> = unmapped.iter().map(|u| u.id.as_str()).collect();
        assert!(ids.contains(&"ghost"));
        assert!(ids.contains(&"pf-only"));
        assert!(!ids.contains(&"u2"));
    }

    #[test]
    fn parse_firmware_info_extracts_version_abi_and_installed_plugins() {
        let value = json!({
            "product": {
                "product_version": "25.7.3",
                "product_abi": "FreeBSD:14:amd64",
                "product_check": {"upgrade_packages": [
                    {"name": "os-tailscale", "new_version": "1.3"},
                    {"name": "opnsense", "new_version": "25.7.11"}
                ]}
            },
            "plugin": [
                {"name": "os-tailscale", "version": "1.2", "installed": "1"},
                {"name": "os-nginx", "version": "1.0", "installed": "0"}
            ]
        });
        let payload = parse_firmware_info(&value);
        assert_eq!(payload.product_version.as_deref(), Some("25.7.3"));
        assert_eq!(payload.abi.as_deref(), Some("FreeBSD:14:amd64"));
        assert_eq!(
            payload.plugins,
            vec![("os-tailscale".to_owned(), Some("1.2".to_owned()))]
        );
        assert_eq!(
            payload.cached_plugin_upgrades,
            vec![("os-tailscale".to_owned(), "1.3".to_owned())]
        );
    }

    #[test]
    fn parse_firmware_info_accepts_top_level_version_fields() {
        let value = json!({"product_version": "25.7", "product_abi": "FreeBSD:14:amd64"});
        let payload = parse_firmware_info(&value);
        assert_eq!(payload.product_version.as_deref(), Some("25.7"));
        assert!(payload.plugins.is_empty());
    }

    #[test]
    fn parse_firmware_info_reads_real_cached_changelog_and_core_abi() {
        let value = json!({
            "product_version": "26.7.6_3",
            "product": {"CORE_VERSION": "26.7.6", "CORE_ABI": "26.7"},
            "changelog": [
                {"series": "26.7", "version": "26.7.7", "date": "2026-10-13"},
                {"series": "26.7", "version": "26.7.6", "date": "2026-10-08"},
                {"invalid": true}
            ]
        });
        let payload = parse_firmware_info(&value);
        assert_eq!(payload.product_version.as_deref(), Some("26.7.6_3"));
        assert_eq!(payload.abi.as_deref(), Some("26.7"));
        assert_eq!(payload.changelog.len(), 2);
        assert_eq!(payload.changelog[0].series.as_deref(), Some("26.7"));
        assert_eq!(payload.changelog[0].version, "26.7.7");
        assert_eq!(payload.changelog[0].date.as_deref(), Some("2026-10-13"));
    }

    #[test]
    fn parse_tailscale_status_extracts_self_and_peers() {
        let value = json!({
            "BackendState": "Running",
            "Self": {"HostName": "fw", "TailscaleIPs": ["100.64.0.1"]},
            "Peer": {
                "k1": {"HostName": "laptop", "TailscaleIPs": ["100.64.0.2"], "Online": true}
            }
        });
        let status = parse_tailscale_status(&value).unwrap();
        assert_eq!(status.backend_state.as_deref(), Some("Running"));
        assert_eq!(status.hostname.as_deref(), Some("fw"));
        assert_eq!(status.own_ips, vec!["100.64.0.1".to_owned()]);
        assert_eq!(status.peers.len(), 1);
        assert_eq!(status.peers[0].online, Some(true));
    }

    #[test]
    fn parse_tailscale_status_rejects_non_object() {
        assert!(matches!(
            parse_tailscale_status(&json!("oops")),
            Err(OpnsenseError::Parse(_))
        ));
    }
}
