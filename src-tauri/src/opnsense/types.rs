//! DTOs, request types and typed errors for OPNsense monitoring.
//!
//! The OPNsense API key/secret NEVER appears in any DTO: profiles serialize
//! `has_credentials` only. All device traffic is read-only HTTP GET; see
//! `client.rs` for the endpoint allowlist.

use std::fmt;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::Serialize;

use crate::db::DbError;
use crate::opnsense::client::{OpnsenseConnection, OpnsenseError};
use crate::secrets::SecretError;

/// Factory that builds a per-connection read-only API handle. Production
/// wraps [`crate::opnsense::client::OpnsenseClient`]; tests inject fakes.
pub type OpnsenseApiFactory = Arc<dyn Fn(OpnsenseConnection) -> OpnsenseApiFuture + Send + Sync>;
pub type OpnsenseApiFuture =
    BoxFuture<'static, Result<Arc<dyn crate::opnsense::client::OpnsenseApi>, OpnsenseManagerError>>;

pub type OpnsenseStatusSink = Arc<dyn Fn(OpnsenseStatusEvent) + Send + Sync>;
pub type OpnsenseSnapshotSink = Arc<dyn Fn(OpnsenseSnapshotDto) + Send + Sync>;
/// Return `false` if the IPC channel receiver has gone away, stopping the
/// log reader instead of leaving an orphaned stream open.
pub type OpnsenseLogEventSink = Arc<dyn Fn(OpnsenseLogEvent) -> bool + Send + Sync>;
pub type OpnsenseLogStatusSink = Arc<dyn Fn(OpnsenseLogStatusEvent) + Send + Sync>;

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

/// Profile DTO for the UI. NEVER serializes `secret_key` (backend-only) and
/// never contains the API key or secret.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseProfileDto {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub allow_invalid_certs: bool,
    pub ssh_port: i64,
    pub has_credentials: bool,
    pub ssh_host_key_verified: bool,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOpnsenseProfileRequest {
    pub name: String,
    pub host: String,
    pub port: i64,
    pub allow_invalid_certs: bool,
    pub ssh_port: i64,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOpnsenseProfileRequest {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub allow_invalid_certs: bool,
    pub ssh_port: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseDeleteResultDto {
    pub deleted: bool,
    pub secret_deleted: bool,
    pub warning: Option<String>,
}

/// `GET /api/core/system/status` probe result: connected and authorized,
/// without echoing any device response payload.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseTestConnectionDto {
    pub connected: bool,
    pub authorized: bool,
}

// ---------------------------------------------------------------------------
// Monitoring snapshots
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseStartDto {
    pub profile_id: i64,
}

/// One entry of the active-monitor list: lets a reloaded view distinguish
/// running-but-detached monitors.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseActiveMonitorDto {
    pub profile_id: i64,
}

/// Machine-readable section failure carried beside last-good data.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseSectionErrorDto {
    pub kind: String,
    pub message: String,
}

impl OpnsenseSectionErrorDto {
    pub fn from_error(err: &OpnsenseError) -> Self {
        Self {
            kind: err.kind().to_owned(),
            message: err.to_string(),
        }
    }
}

/// One independently-fetched snapshot section: fresh data, or an error with
/// no data (the view keeps showing the last good data with a stale
/// timestamp).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseSectionDto<T> {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<OpnsenseSectionErrorDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> OpnsenseSectionDto<T> {
    pub fn ok(data: T) -> Self {
        Self {
            ok: true,
            error: None,
            data: Some(data),
        }
    }

    pub fn error(err: &OpnsenseError) -> Self {
        Self {
            ok: false,
            error: Some(OpnsenseSectionErrorDto::from_error(err)),
            data: None,
        }
    }
}

/// Monitoring tick: `rules` is omitted on the intervening 5 s system/VPN
/// ticks so large rule tables are not cloned and serialized unnecessarily.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseSnapshotDto {
    pub profile_id: i64,
    /// RFC 3339 UTC timestamp of this snapshot.
    pub at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<OpnsenseSectionDto<OpnsenseRulesSectionDto>>,
    pub system: OpnsenseSectionDto<OpnsenseSystemSectionDto>,
    pub vpn: OpnsenseSectionDto<OpnsenseVpnSectionDto>,
}

/// Lifecycle / failure events on the monitor status channel.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum OpnsenseStatusEvent {
    Started { profile_id: i64 },
    Stopped { profile_id: i64 },
    Warning { profile_id: i64, message: String },
    Error { profile_id: i64, message: String },
}

// ---------------------------------------------------------------------------
// Firewall rules
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseRulesSectionDto {
    pub rules: Vec<OpnsenseRuleDto>,
    /// Total rows the device reported (may exceed `rules.len()` when
    /// truncated).
    pub total: i64,
    /// True when the 10,000-rule cap cut the listing short.
    pub truncated: bool,
    /// Present when rule definitions loaded but the counters endpoint
    /// failed (e.g. 403 on `filter_util/ruleStats`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counters_error: Option<OpnsenseSectionErrorDto>,
    /// Active PF rule IDs with stats that match no reported rule UUID.
    pub unmapped_running: Vec<OpnsenseUnmappedRuleDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseRuleDto {
    pub uuid: String,
    pub action: String,
    pub interface: String,
    pub description: String,
    pub source: String,
    pub destination: String,
    pub protocol: String,
    pub enabled: bool,
    /// "Hits (packets)" — None (rendered "—") when counters are missing,
    /// never fabricated as zero.
    pub packets: Option<i64>,
    pub evaluations: Option<i64>,
    pub bytes: Option<i64>,
    pub states: Option<i64>,
}

/// An active PF rule ID (or stats entry) that matches no reported rule
/// UUID. Numeric PF IDs are NEVER joined to UUIDs.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseUnmappedRuleDto {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub packets: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub states: Option<i64>,
}

// ---------------------------------------------------------------------------
// System / interfaces
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseSystemSectionDto {
    pub interfaces: Vec<OpnsenseInterfaceDto>,
    /// The firewall reported more interfaces than read-only GET pagination
    /// could return; never present this partial list as complete.
    pub interfaces_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pf_states: Option<OpnsensePfStatesDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pf_states_error: Option<OpnsenseSectionErrorDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<OpnsenseResourcesDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources_error: Option<OpnsenseSectionErrorDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseInterfaceDto {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub enabled: bool,
    /// Link state as reported ("up"/"down"), None when not reported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rx_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rx_packets: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_packets: Option<i64>,
}

/// PF state table capacity — a firewall health signal.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsensePfStatesDto {
    pub current: Option<i64>,
    pub max: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseResourcesDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_usage_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_used_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mem_total_bytes: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uptime: Option<String>,
}

// ---------------------------------------------------------------------------
// VPN / Tailscale
// ---------------------------------------------------------------------------

/// One VPN service family, independent of the others: a missing plugin or a
/// denied ACL must not break sibling cards.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseVpnServiceDto<T> {
    /// "ok" | "unavailable" (404 / plugin missing / malformed) |
    /// "access-denied" (403) | "error" (anything else).
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> OpnsenseVpnServiceDto<T> {
    pub fn ok(data: T) -> Self {
        Self {
            state: "ok".to_owned(),
            message: None,
            data: Some(data),
        }
    }

    /// Classify a fetch error: 403 → access-denied, 404 → unavailable,
    /// parse failure → unavailable, anything else → error.
    pub fn failed(err: &OpnsenseError) -> Self {
        let (state, message) = match err {
            OpnsenseError::Forbidden => (
                "access-denied",
                "Access denied — the API key lacks privileges for this service".to_owned(),
            ),
            OpnsenseError::NotFound(_) => (
                "unavailable",
                "Plugin unavailable (not installed or API unsupported)".to_owned(),
            ),
            OpnsenseError::Parse(_) => (
                "unavailable",
                "Unavailable (unexpected response)".to_owned(),
            ),
            other => ("error", other.to_string()),
        };
        Self {
            state: state.to_owned(),
            message: Some(message),
            data: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseVpnSectionDto {
    pub wireguard: OpnsenseVpnServiceDto<OpnsenseWireguardStatusDto>,
    pub openvpn: OpnsenseVpnServiceDto<Vec<OpnsenseOpenvpnSessionDto>>,
    pub ipsec: OpnsenseVpnServiceDto<Vec<OpnsenseIpsecSessionDto>>,
    pub tailscale: OpnsenseVpnServiceDto<OpnsenseTailscaleStatusDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseWireguardStatusDto {
    pub interfaces: Vec<OpnsenseWireguardInterfaceDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseWireguardInterfaceDto {
    pub name: String,
    pub peers: Vec<OpnsenseWireguardPeerDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseWireguardPeerDto {
    /// Peer name/comment when known, else the public key prefix.
    pub name: String,
    /// Seconds since the latest handshake; None when never handshaked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handshake_age_secs: Option<i64>,
    /// "active" | "stale" | "never" | "unknown" — stale is DISTINCT from
    /// offline.
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseOpenvpnSessionDto {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// "client" | "server" | "unknown".
    pub kind: String,
    pub connected: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseIpsecSessionDto {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub connected: bool,
}

/// Tailscale status with sensitive raw plugin JSON intentionally dropped:
/// only these parsed fields reach the UI.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseTailscaleStatusDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_running: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    pub own_ips: Vec<String>,
    pub peers: Vec<OpnsenseTailscalePeerDto>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseTailscalePeerDto {
    pub name: String,
    pub ips: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<String>,
}

// ---------------------------------------------------------------------------
// Firmware (cached status only — the app never triggers a remote check)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseFirmwareStatusDto {
    /// "cached-updates" | "cached-none" | "no-cache" | "unavailable" |
    /// "error".
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Remote check timestamp from `product.product_check`, when provided.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checked_at: Option<String>,
    /// Installed core version from `GET /api/core/firmware/info`, when
    /// available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    /// Core and system-set candidates (all non-`os-*` packages).
    pub core_upgrades: Vec<OpnsenseUpgradeDto>,
    pub plugin_upgrades: Vec<OpnsenseUpgradeDto>,
    pub reboot_required: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseUpgradeDto {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub new_version: Option<String>,
}

// ---------------------------------------------------------------------------
// Backups (saved revisions, downloaded read-only, encrypted locally)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseRevisionDto {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_iso: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesize: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseBackupResultDto {
    /// Local path of the written `<name>.xml.age` file.
    pub path: String,
    /// The saved firewall revision this backup was taken from — possibly
    /// older than unsaved changes on the device.
    pub source_revision_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_time_iso: Option<String>,
}

// ---------------------------------------------------------------------------
// Firewall log stream
// ---------------------------------------------------------------------------

/// One normalized firewall log line. `action` is "pass" (allowed), "block"
/// (blocked; `reject` maps here) or "other" (shown in the All tab only).
/// Absent fields stay None — never fabricated.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseLogEvent {
    pub profile_id: i64,
    /// Local receive time (RFC 3339 UTC).
    pub received_at: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proto: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dst: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dst_port: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Lifecycle / failure events for the log stream.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum OpnsenseLogStatusEvent {
    Started { profile_id: i64 },
    Stopped { profile_id: i64 },
    Warning { profile_id: i64, message: String },
    Error { profile_id: i64, message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseLogStartDto {
    pub profile_id: i64,
}

// ---------------------------------------------------------------------------
// SSH terminal
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpnsenseTerminalOpenDto {
    pub terminal_id: String,
}

// ---------------------------------------------------------------------------
// Manager error
// ---------------------------------------------------------------------------

/// Typed manager/command errors, serialized for the frontend as
/// `{kind, message}` (plus `fingerprint` for host-key prompts).
#[derive(Debug)]
pub enum OpnsenseManagerError {
    /// That profile already backs a running monitor.
    AlreadyMonitoring(i64),
    NoActiveMonitor,
    /// The 8-monitor concurrency cap is full.
    TooManySessions,
    /// The 8-terminal concurrency cap is full.
    TooManyTerminals,
    TerminalNotFound(String),
    ProfileNotFound(i64),
    /// A monitor, log stream or terminal is active for this profile.
    ProfileInUse(i64),
    NotStored,
    Unauthorized,
    /// Host is not a bare DNS name or IP (scheme/path/userinfo present).
    InvalidHost(String),
    InvalidPort(i64),
    InvalidSshPort(i64),
    /// API key or secret was blank.
    InvalidCredentials,
    /// First SSH connection: host key not yet pinned. The UI prompts for
    /// explicit verification and retries with `confirmed_fingerprint`.
    UnknownHostKey {
        fingerprint: String,
    },
    /// Presented host key differs from the pinned one.
    HostKeyMismatch {
        expected: String,
        actual: String,
    },
    /// SSH password authentication was rejected.
    SshAuthFailed,
    Api(OpnsenseError),
    Db(DbError),
}

impl OpnsenseManagerError {
    fn kind(&self) -> &'static str {
        match self {
            Self::AlreadyMonitoring(_) => "already-monitoring",
            Self::NoActiveMonitor => "no-active-monitor",
            Self::TooManySessions => "too-many-sessions",
            Self::TooManyTerminals => "too-many-terminals",
            Self::TerminalNotFound(_) => "terminal-not-found",
            Self::ProfileNotFound(_) => "profile-not-found",
            Self::ProfileInUse(_) => "profile-in-use",
            Self::NotStored => "not-stored",
            Self::Unauthorized => "unauthorized",
            Self::InvalidHost(_) => "invalid-host",
            Self::InvalidPort(_) => "invalid-port",
            Self::InvalidSshPort(_) => "invalid-ssh-port",
            Self::InvalidCredentials => "invalid-credentials",
            Self::UnknownHostKey { .. } => "unknown-host-key",
            Self::HostKeyMismatch { .. } => "host-key-mismatch",
            Self::SshAuthFailed => "ssh-auth-failed",
            Self::Api(_) => "api",
            Self::Db(_) => "db",
        }
    }
}

impl fmt::Display for OpnsenseManagerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyMonitoring(id) => {
                write!(f, "profile {id} is already being monitored")
            }
            Self::NoActiveMonitor => f.write_str("no OPNsense monitor is running for that profile"),
            Self::TooManySessions => {
                f.write_str("too many concurrent OPNsense monitors — disconnect one first")
            }
            Self::TooManyTerminals => {
                f.write_str("too many concurrent terminals — close one first")
            }
            Self::TerminalNotFound(id) => write!(f, "no terminal with id {id}"),
            Self::ProfileNotFound(id) => write!(f, "no OPNsense profile with id {id}"),
            Self::ProfileInUse(id) => write!(
                f,
                "OPNsense profile {id} has an active monitor, log stream or terminal — disconnect first"
            ),
            Self::NotStored => f.write_str("API credentials not stored — re-enter"),
            Self::Unauthorized => f.write_str("authentication failed (401)"),
            Self::InvalidHost(host) => write!(
                f,
                "invalid host {host:?}: enter a DNS name or IP without scheme, path or credentials"
            ),
            Self::InvalidPort(port) => write!(f, "invalid HTTPS port {port} (1-65535)"),
            Self::InvalidSshPort(port) => write!(f, "invalid SSH port {port} (1-65535)"),
            Self::InvalidCredentials => {
                f.write_str("API key and secret must both be non-empty")
            }
            Self::UnknownHostKey { fingerprint } => write!(
                f,
                "unknown SSH host key (SHA-256 {fingerprint}) — verify the fingerprint before connecting"
            ),
            Self::HostKeyMismatch { expected, actual } => write!(
                f,
                "SSH host key changed (was SHA-256 {expected}, now {actual}) — refusing to connect"
            ),
            Self::SshAuthFailed => f.write_str("SSH authentication failed for that user"),
            Self::Api(err) => write!(f, "{err}"),
            Self::Db(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for OpnsenseManagerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Api(err) => Some(err),
            Self::Db(err) => Some(err),
            _ => None,
        }
    }
}

impl Serialize for OpnsenseManagerError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("OpnsenseManagerError", 3)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        match self {
            Self::UnknownHostKey { fingerprint }
            | Self::HostKeyMismatch {
                actual: fingerprint,
                ..
            } => state.serialize_field("fingerprint", fingerprint)?,
            _ => state.skip_field("fingerprint")?,
        }
        state.end()
    }
}

impl From<DbError> for OpnsenseManagerError {
    fn from(err: DbError) -> Self {
        Self::Db(err)
    }
}

impl From<OpnsenseError> for OpnsenseManagerError {
    fn from(err: OpnsenseError) -> Self {
        match err {
            OpnsenseError::Unauthorized => Self::Unauthorized,
            other => Self::Api(other),
        }
    }
}

impl From<SecretError> for OpnsenseManagerError {
    fn from(err: SecretError) -> Self {
        match err {
            SecretError::NotStored => Self::NotStored,
            SecretError::Keyring(message) => Self::Api(OpnsenseError::Connect(message)),
        }
    }
}
