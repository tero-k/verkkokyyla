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
    OpnsenseInterfaceDto, OpnsenseIpsecSessionDto, OpnsenseOpenvpnSessionDto, OpnsensePfStatesDto,
    OpnsenseResourcesDto, OpnsenseRevisionDto, OpnsenseRuleDto, OpnsenseTailscalePeerDto,
    OpnsenseTailscaleStatusDto, OpnsenseUnmappedRuleDto, OpnsenseWireguardInterfaceDto,
    OpnsenseWireguardPeerDto, OpnsenseWireguardStatusDto,
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

/// Raw pages/rows from search endpoints.
/// `current` is the page number returned by the firewall; on releases
/// where searchRecordsetBase only reads POST fields, GET pagination is
/// ignored and the caller must stop rather than append page 1 again.
pub struct RuleSearchPage {
    pub rules: Vec<OpnsenseRuleDto>,
    pub total: i64,
    pub current: Option<u32>,
}

pub struct InterfaceInfoPage {
    pub interfaces: Vec<OpnsenseInterfaceDto>,
    pub total: i64,
    pub current: Option<u32>,
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

/// Raw firmware status payload parsed for the cached-status card.
#[derive(Clone, Debug, Default)]
pub struct FirmwareStatusPayload {
    pub status: Option<String>,
    pub status_msg: Option<String>,
    /// `product.product_check` is non-null, whether it is a string or the
    /// actual cached result object returned by FirmwareController.
    pub has_cached_check: bool,
    /// Remote check time only when the firewall explicitly provides one.
    pub checked_at: Option<String>,
    pub reboot_required: bool,
    /// (name, old, new) for every upgrade candidate; the caller splits
    /// core from `os-*` plugins.
    pub upgrades: Vec<(String, Option<String>, Option<String>)>,
}

/// Byte stream of an open SSE response (no total deadline).
pub type SseByteStream = BoxStream<'static, Result<Vec<u8>, OpnsenseError>>;

/// The fixed read-only endpoint surface. Tests inject fakes; production is
/// [`OpnsenseClient`].
#[async_trait]
pub trait OpnsenseApi: Send + Sync {
    /// `GET /api/core/system/status` — connectivity/authorization probe.
    async fn get_system_status(&self) -> Result<(), OpnsenseError>;

    /// `GET /api/firewall/filter/searchRule?show_all=1` — one page.
    async fn search_rules(
        &self,
        current: u32,
        row_count: u32,
    ) -> Result<RuleSearchPage, OpnsenseError>;

    /// `GET /api/firewall/filter_util/ruleStats` — counters keyed by UUID.
    async fn rule_stats(&self) -> Result<Vec<(String, RuleStat)>, OpnsenseError>;

    /// `GET /api/diagnostics/firewall/listRuleIds` — active PF rule IDs.
    async fn list_rule_ids(&self) -> Result<Vec<PfRuleId>, OpnsenseError>;

    /// `GET /api/interfaces/overview/interfacesInfo` — one page.
    async fn interfaces_info(
        &self,
        current: u32,
        row_count: u32,
    ) -> Result<InterfaceInfoPage, OpnsenseError>;

    /// `GET /api/diagnostics/interface/get_interface_statistics`.
    async fn interface_statistics(&self) -> Result<Vec<OpnsenseInterfaceDto>, OpnsenseError>;

    /// `GET /api/diagnostics/firewall/pfStates` — state table usage.
    async fn pf_states(&self) -> Result<OpnsensePfStatesDto, OpnsenseError>;

    /// `GET /api/diagnostics/system/system_resources`.
    async fn system_resources(&self) -> Result<OpnsenseResourcesDto, OpnsenseError>;

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

    /// `GET /api/core/firmware/status` — LAST firewall-side check only;
    /// never POSTs (that would trigger a synchronous probe).
    async fn firmware_status(&self) -> Result<FirmwareStatusPayload, OpnsenseError>;

    /// `GET /api/core/firmware/info` — installed core version.
    async fn firmware_info(&self) -> Result<Option<String>, OpnsenseError>;

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
            .map(|chunk| {
                chunk
                    .map(|b| b.to_vec())
                    .map_err(|e| classify_transport(&e))
            })
            .boxed())
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

    async fn search_rules(
        &self,
        current: u32,
        row_count: u32,
    ) -> Result<RuleSearchPage, OpnsenseError> {
        let value = self
            .get_json(
                "firewall/filter/searchRule",
                &[
                    ("show_all", "1".to_owned()),
                    ("current", current.to_string()),
                    ("rowCount", row_count.to_string()),
                ],
            )
            .await?;
        let rules = rows_of(&value)
            .into_iter()
            .map(parse_rule_row)
            .collect::<Vec<_>>();
        Ok(RuleSearchPage {
            total: total_of(&value, rules.len() as i64),
            current: i64_field(&value, &["current"]).and_then(|n| u32::try_from(n).ok()),
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

    async fn interfaces_info(
        &self,
        current: u32,
        row_count: u32,
    ) -> Result<InterfaceInfoPage, OpnsenseError> {
        let value = self
            .get_json(
                "interfaces/overview/interfacesInfo",
                &[
                    ("current", current.to_string()),
                    ("rowCount", row_count.to_string()),
                ],
            )
            .await?;
        let interfaces = rows_of(&value)
            .into_iter()
            .map(parse_interface_row)
            .collect::<Vec<_>>();
        Ok(InterfaceInfoPage {
            total: total_of(&value, interfaces.len() as i64),
            current: i64_field(&value, &["current"]).and_then(|n| u32::try_from(n).ok()),
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

    async fn firmware_status(&self) -> Result<FirmwareStatusPayload, OpnsenseError> {
        let value = self.get_json("core/firmware/status", &[]).await?;
        Ok(parse_firmware_status(&value))
    }

    async fn firmware_info(&self) -> Result<Option<String>, OpnsenseError> {
        let value = self.get_json("core/firmware/info", &[]).await?;
        Ok(value
            .get("product")
            .and_then(|p| str_field(p, &["product_version", "version"])))
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
    let name = str_field(row, &["name", "identifier", "ifname", "interface"]).unwrap_or_default();
    let (ipv4, ipv6) = parse_addresses(row);
    OpnsenseInterfaceDto {
        name,
        description: str_field(row, &["description", "descr"]),
        enabled: field(row, &["enabled"]).and_then(opt_bool).unwrap_or(true),
        link: str_field(row, &["link", "status"]).map(|s| s.to_ascii_lowercase()),
        ipv4,
        ipv6,
        rx_bytes: None,
        tx_bytes: None,
        rx_packets: None,
        tx_packets: None,
    }
}

fn parse_addresses(row: &Value) -> (Option<String>, Option<String>) {
    let take = |keys: &[&str]| -> Option<String> {
        match field(row, keys) {
            Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
            Some(Value::Array(items)) => items.iter().find_map(opt_str),
            Some(Value::Object(map)) => map.values().find_map(opt_str),
            _ => None,
        }
    };
    (
        take(&["ipv4", "ipv4_address", "ipaddr"]),
        take(&["ipv6", "ipv6_address", "ipaddr6"]),
    )
}

fn parse_interface_statistics(value: &Value) -> Vec<OpnsenseInterfaceDto> {
    // Shape: {"statistics": {"em0": {"bytes received": ..., ...}}}
    let stats = match field(value, &["statistics"]).and_then(Value::as_object) {
        Some(map) => map,
        None => return Vec::new(),
    };
    stats
        .iter()
        .map(|(name, entry)| {
            let find = |keys: &[&str]| -> Option<i64> {
                keys.iter().find_map(|k| entry.get(*k)).and_then(opt_i64)
            };
            OpnsenseInterfaceDto {
                name: name.clone(),
                description: None,
                enabled: true,
                link: None,
                ipv4: None,
                ipv6: None,
                rx_bytes: find(&["bytes received", "rx-bytes", "rx_bytes", "ibytes"]),
                tx_bytes: find(&["bytes transmitted", "tx-bytes", "tx_bytes", "obytes"]),
                rx_packets: find(&["packets received", "rx-packets", "rx_packets", "ipackets"]),
                tx_packets: find(&[
                    "packets transmitted",
                    "tx-packets",
                    "tx_packets",
                    "opackets",
                ]),
            }
        })
        .collect()
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

fn parse_system_resources(value: &Value) -> OpnsenseResourcesDto {
    let cpu = field(value, &["cpu"]);
    let cpu_usage = cpu
        .and_then(|c| field(c, &["used", "usage", "load"]).and_then(opt_f64))
        .or_else(|| i64_field(value, &["cpu_usage", "cpu"]).map(|v| v as f64));
    let memory = field(value, &["memory", "mem"]);
    let (mem_used, mem_total) = match memory {
        Some(m) => (
            i64_field(m, &["used", "used_bytes"]),
            i64_field(m, &["total", "total_bytes"]),
        ),
        None => (None, None),
    };
    OpnsenseResourcesDto {
        cpu_usage_percent: cpu_usage,
        mem_used_bytes: mem_used,
        mem_total_bytes: mem_total,
        uptime: str_field(value, &["uptime"]),
    }
}

fn parse_wireguard_show(value: &Value) -> OpnsenseWireguardStatusDto {
    let rows = rows_of(value);
    let mut interfaces: Vec<OpnsenseWireguardInterfaceDto> = Vec::new();
    for row in &rows {
        let kind = str_field(row, &["type"]).unwrap_or_default();
        if kind == "interface" {
            let name = str_field(row, &["name", "interface"]).unwrap_or_default();
            interfaces.push(OpnsenseWireguardInterfaceDto {
                name,
                peers: Vec::new(),
            });
        }
    }
    for row in &rows {
        let kind = str_field(row, &["type"]).unwrap_or_default();
        if kind != "peer" {
            continue;
        }
        let parent = str_field(row, &["interface", "ifname"]).unwrap_or_default();
        let name = str_field(row, &["name", "descr", "description"])
            .or_else(|| {
                str_field(row, &["public-key", "public_key", "pubkey"])
                    .map(|key| key.chars().take(12).collect::<String>())
            })
            .unwrap_or_else(|| "(peer)".to_owned());
        let handshake_age = i64_field(row, &["latest-handshake", "latest_handshake"]);
        let status = match handshake_age {
            None => "never",
            Some(age) if age <= 180 => "active",
            Some(_) => "stale",
        };
        let peer = OpnsenseWireguardPeerDto {
            name,
            handshake_age_secs: handshake_age,
            status: status.to_owned(),
        };
        match interfaces.iter_mut().find(|i| i.name == parent) {
            Some(iface) => iface.peers.push(peer),
            None => interfaces.push(OpnsenseWireguardInterfaceDto {
                name: parent,
                peers: vec![peer],
            }),
        }
    }
    OpnsenseWireguardStatusDto { interfaces }
}

fn parse_openvpn_row(row: &Value) -> OpnsenseOpenvpnSessionDto {
    let kind = str_field(row, &["type", "mode", "role"]).unwrap_or_else(|| "unknown".to_owned());
    let connected = field(row, &["connected"])
        .and_then(opt_bool)
        .or_else(|| str_field(row, &["status"]).and_then(|s| opt_bool(&Value::String(s))))
        .unwrap_or(false);
    OpnsenseOpenvpnSessionDto {
        name: str_field(row, &["name", "common_name", "id"]).unwrap_or_default(),
        description: str_field(row, &["description", "descr"]),
        kind,
        connected,
    }
}

fn parse_ipsec_row(row: &Value) -> OpnsenseIpsecSessionDto {
    let connected = field(row, &["connected"])
        .and_then(opt_bool)
        .unwrap_or(false);
    OpnsenseIpsecSessionDto {
        name: str_field(row, &["name", "connection", "id"]).unwrap_or_default(),
        description: str_field(row, &["description", "descr"]),
        connected,
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

fn parse_firmware_status(value: &Value) -> FirmwareStatusPayload {
    let product = value.get("product");
    let mut upgrades = Vec::new();
    for key in ["all_packages", "all_sets"] {
        if let Some(Value::Object(packages)) = value.get(key) {
            for (name, entry) in packages {
                upgrades.push((
                    name.clone(),
                    str_field(entry, &["old", "old_version", "installed"]),
                    str_field(entry, &["new", "new_version", "candidate"]),
                ));
            }
        }
    }
    let check = product.and_then(|p| p.get("product_check"));
    FirmwareStatusPayload {
        status: str_field(value, &["status"]),
        status_msg: str_field(value, &["status_msg", "statusMessage"]),
        has_cached_check: check.is_some_and(|v| !v.is_null()),
        checked_at: check
            .and_then(opt_str)
            .or_else(|| check.and_then(|v| str_field(v, &["checked_at", "timestamp", "time"])))
            .or_else(|| product.and_then(|p| str_field(p, &["product_lastcheck"]))),
        reboot_required: field(value, &["upgrade_needs_reboot", "reboot_required"])
            .and_then(opt_bool)
            .unwrap_or(false),
        upgrades,
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
    fn parse_firmware_status_splits_candidates_and_check_time() {
        let value = json!({
            "status": "update",
            "status_msg": "There are updates available",
            "product": {"product_check": {"status": "update"}, "product_version": "26.1"},
            "upgrade_needs_reboot": "1",
            "all_packages": {
                "os-tailscale": {"old": "1.0", "new": "1.1"},
                "opnsense": {"old": "26.1", "new": "26.1.1"}
            }
        });
        let payload = parse_firmware_status(&value);
        assert_eq!(payload.status.as_deref(), Some("update"));
        assert!(payload.has_cached_check);
        assert!(payload.checked_at.is_none());
        assert!(payload.reboot_required);
        assert_eq!(payload.upgrades.len(), 2);
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
