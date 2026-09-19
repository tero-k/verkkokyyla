//! Serde DTOs, event enums, and typed errors for the MikroTik polling engine.
//!
//! Wire contract (locked): `MikrotikEvent` and `MikrotikStatusEvent` are
//! enums with `#[serde(tag = "event", rename_all = "kebab-case",
//! rename_all_fields = "camelCase")]` — exactly like `trace/types.rs:25-46`.
//! The snapshot serializes as `{"event":"snapshot",...}` and each status
//! variant as `{"event":"<kebab-variant>",...}`; serde_json round-trip tests
//! assert these exact tag strings.

use std::fmt;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::Serialize;

use crate::db::{Database, DbError};
use crate::mikrotik::client::MikrotikConnection;
use crate::mikrotik::error::MikrotikError;
use crate::mikrotik::parse::{
    BondingDto, BridgeVlanDto, EthernetMonitorDto, EthernetStatsDto, InterfaceDto, LogEntryDto,
    ResourceDto, RouterboardDto, SensorDto, UpdateStatusDto, VlanDto,
};

pub use crate::mikrotik::secrets::SecretError;

/// Factory that builds the per-session REST API handle. Production wraps
/// [`crate::mikrotik::client::MikrotikClient`]; tests inject a scripted fake
/// (pattern: `tests/trace.rs` factory injection).
pub type MikrotikApiFactory = Arc<dyn Fn(MikrotikConnection) -> MikrotikApiFuture + Send + Sync>;
pub type MikrotikApiFuture = BoxFuture<'static, Result<Arc<dyn MikrotikApi>, MikrotikError>>;

/// The REST surface the polling runtime consumes. Todo 2's client implements
/// it; scripted fakes implement it in `mikrotik_runtime_*` tests.
#[async_trait::async_trait]
pub trait MikrotikApi: Send + Sync {
    async fn get_resource(&self) -> Result<ResourceDto, MikrotikError>;
    async fn get_interfaces(&self) -> Result<Vec<InterfaceDto>, MikrotikError>;
    async fn get_health(&self) -> Result<Vec<SensorDto>, MikrotikError>;
    async fn get_interface_stats_detail(&self) -> Result<Vec<InterfaceDto>, MikrotikError>;
    async fn get_ethernet_stats(&self) -> Result<Vec<EthernetStatsDto>, MikrotikError>;
    async fn get_ethernet_monitor(
        &self,
        name: &str,
    ) -> Result<Vec<EthernetMonitorDto>, MikrotikError>;
    async fn get_vlans(&self) -> Result<Vec<VlanDto>, MikrotikError>;
    async fn get_bridge_vlans(&self) -> Result<Vec<BridgeVlanDto>, MikrotikError>;

    /// `GET /rest/interface/bonding` — bonding masters and their slave
    /// ports. Defaults to "unsupported" so scripted fakes need not implement
    /// it; the runtime treats that as "no bonding to aggregate" (silent).
    async fn get_bonding(&self) -> Result<Vec<BondingDto>, MikrotikError> {
        Err(MikrotikError::Api {
            status: 404,
            message: "bonding endpoint not implemented".to_owned(),
        })
    }
    async fn get_update_status(&self) -> Result<UpdateStatusDto, MikrotikError> {
        Err(MikrotikError::Api {
            status: 404,
            message: "update endpoint not implemented".to_owned(),
        })
    }
    async fn check_for_updates(&self) -> Result<UpdateStatusDto, MikrotikError> {
        Err(MikrotikError::Api {
            status: 404,
            message: "update endpoint not implemented".to_owned(),
        })
    }
    async fn get_routerboard(&self) -> Result<RouterboardDto, MikrotikError> {
        Err(MikrotikError::Api {
            status: 404,
            message: "routerboard endpoint not implemented".to_owned(),
        })
    }

    /// `POST /rest/log/print` — in-memory log entries. REST has no streaming
    /// mode (official docs rule out continuous commands), so the log stream
    /// runtime polls this and dedupes by record id.
    async fn get_log(&self) -> Result<Vec<LogEntryDto>, MikrotikError> {
        Err(MikrotikError::Api {
            status: 404,
            message: "log endpoint not implemented".to_owned(),
        })
    }
}

/// One tick's core resource sample (`/system/resource`).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MikrotikResourcesDto {
    pub cpu_load: Option<f64>,
    pub mem_used_bytes: Option<u64>,
    pub mem_total_bytes: Option<u64>,
    pub uptime: Option<String>,
    pub board_name: Option<String>,
    pub routeros_version: Option<String>,
    pub architecture_name: Option<String>,
}

/// Flattened health sensor for the wire and for `sensors_json`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MikrotikSensorDto {
    pub name: String,
    pub value: f64,
    pub unit: Option<String>,
    pub kind: String,
}

/// Per-interface wire DTO: base counters from `/interface/print stats`,
/// driver counters merged from `get_ethernet_stats` (by name, matching
/// `default-name` when ports were renamed), monitor rate/duplex merged from
/// `get_ethernet_monitor`, and computed bit rates from ACTUAL elapsed time.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MikrotikInterfaceDto {
    pub name: String,
    #[serde(rename = "type")]
    pub iface_type: Option<String>,
    pub running: Option<bool>,
    pub disabled: Option<bool>,
    pub rx_byte: Option<u64>,
    pub tx_byte: Option<u64>,
    pub rx_packet: Option<u64>,
    pub tx_packet: Option<u64>,
    pub tx_queue_drop: Option<u64>,
    pub link_downs: Option<u64>,
    pub rx_error: Option<u64>,
    pub tx_error: Option<u64>,
    pub rx_drop: Option<u64>,
    pub rx_error_events: Option<u64>,
    pub tx_error_events: Option<u64>,
    pub rx_fcs_error: Option<u64>,
    pub rx_align_error: Option<u64>,
    pub tx_collision: Option<u64>,
    pub tx_drop: Option<u64>,
    pub rate: Option<String>,
    pub full_duplex: Option<bool>,
    /// Operator comment from `/interface/print` (null when unset).
    pub comment: Option<String>,
    pub rx_bits_per_second: Option<f64>,
    pub tx_bits_per_second: Option<f64>,
}
/// Exactly ONE `Snapshot` event per tick. VLAN fields are `Some` only on the
/// ticks they were fetched (session start and every 12th tick); otherwise
/// `null` and the UI keeps the last known.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MikrotikSnapshotPayload {
    pub session_id: i64,
    pub at: String,
    pub resources: Option<MikrotikResourcesDto>,
    /// `Some` once health has succeeded at least once this session; `None`
    /// when the endpoint has never succeeded (or is not supported).
    pub sensors: Option<Vec<MikrotikSensorDto>>,
    /// `false` only when the board reports no-such-command / 404 for health —
    /// a stable, nonterminal "not supported" state (no warning spam).
    pub sensors_supported: bool,
    pub interfaces: Vec<MikrotikInterfaceDto>,
    pub vlans: Option<Vec<VlanDto>>,
    pub bridge_vlans: Option<Vec<BridgeVlanDto>>,
    /// Enricher failure note for this tick (`null` on a clean tick).
    pub warning: Option<String>,
}

/// Channel events streamed to the live UI.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MikrotikEvent {
    Snapshot(MikrotikSnapshotPayload),
}

/// Channel events streamed to the live log view. The stream emits entries in
/// router order (oldest first); the UI caps and filters its buffer.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MikrotikLogEvent {
    Entries { entries: Vec<LogEntryDto> },
}

/// Lifecycle / failure events for the log stream. Unlike
/// `MikrotikStatusEvent` there is no DB session behind a log stream (live
/// only, nothing persisted), so these carry no session id.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MikrotikLogStatusEvent {
    Started { profile_id: i64 },
    Stopped,
    Warning { message: String },
    Error { message: String },
}

pub type MikrotikLogEventSink = Arc<dyn Fn(MikrotikLogEvent) + Send + Sync>;
pub type MikrotikLogStatusSink = Arc<dyn Fn(MikrotikLogStatusEvent) + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MikrotikLogStartDto {
    pub profile_id: i64,
}

/// Lifecycle / failure events on the status channel. Enricher failures only
/// ever produce `Warning`; CORE failures produce `Warning` per failure and a
/// terminal `Error` after 3 consecutive failures. Todo 6 adds a
/// `VersionFirmware` variant — the enum must stay extensible.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MikrotikStatusEvent {
    Started {
        session_id: i64,
        profile_id: i64,
    },
    Stopped {
        session_id: i64,
        snapshot_count: u64,
    },
    Cancelled {
        session_id: i64,
        snapshot_count: u64,
    },
    Warning {
        session_id: i64,
        source: String,
        message: String,
    },
    Error {
        session_id: i64,
        message: String,
    },
    VersionFirmware {
        session_id: i64,
        update_status: crate::mikrotik::version::UpdateStatusResultDto,
        firmware_status: crate::mikrotik::version::FirmwareStatusDto,
    },
}

pub type MikrotikStatusSink = Arc<dyn Fn(MikrotikStatusEvent) + Send + Sync>;
/// Profile DTO for the UI. NEVER serializes `secret_key` (backend-only).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MikrotikProfileDto {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub use_tls: bool,
    pub allow_invalid_certs: bool,
    pub username: String,
    pub has_password: bool,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateMikrotikProfileRequest {
    pub name: String,
    pub host: String,
    pub port: i64,
    pub use_tls: bool,
    pub allow_invalid_certs: bool,
    pub username: String,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMikrotikProfileRequest {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub use_tls: bool,
    pub allow_invalid_certs: bool,
    pub username: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteProfileResultDto {
    pub deleted: bool,
    pub secret_deleted: bool,
    pub warning: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MikrotikStartDto {
    pub session_id: i64,
    pub profile_id: i64,
}

/// One entry of the active-session list: lets the frontend rehydrate its
/// device switcher after a reload without any channel replay.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveSessionDto {
    pub session_id: i64,
    pub profile_id: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MikrotikStoppedDto {
    pub session_id: i64,
    pub snapshot_count: u64,
    pub ended_at: String,
    pub status: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MikrotikTestConnectionDto {
    pub board_name: Option<String>,
    pub routeros_version: Option<String>,
    pub architecture_name: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MikrotikSessionSummaryDto {
    pub id: i64,
    pub profile_id: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub board_name: Option<String>,
    pub routeros_version: Option<String>,
    pub architecture_name: Option<String>,
    pub update_status_json: Option<String>,
    pub firmware_status_json: Option<String>,
    pub snapshot_count: i64,
}

/// One persisted tick as loaded for history (JSON columns stay raw strings so
/// history renders identically to live).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MikrotikSnapshotDto {
    pub id: i64,
    pub session_id: i64,
    pub at: String,
    pub cpu_load: Option<f64>,
    pub mem_used_bytes: Option<i64>,
    pub mem_total_bytes: Option<i64>,
    pub uptime: Option<String>,
    pub warning: Option<String>,
    pub sensors_json: Option<String>,
    pub interfaces_json: Option<String>,
    pub vlans_json: Option<String>,
    pub bridge_vlans_json: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedMikrotikSessionDto {
    pub session: MikrotikSessionSummaryDto,
    pub snapshots: Vec<MikrotikSnapshotDto>,
}

/// Typed manager/command errors. Serialized for the frontend as
/// `{kind, message}` exactly like `TraceError`.
#[derive(Debug)]
pub enum MikrotikManagerError {
    /// That profile already backs a running session — the UI can point at
    /// the offending device. Serializes with kind `already-running` so the
    /// frontend error contract stays unchanged from the single-slot days.
    AlreadyRunningForProfile(i64),
    NoActiveSession,
    /// The 8-session concurrency cap is full.
    TooManySessions,
    ProfileNotFound(i64),
    SessionNotFound(i64),
    ProfileInUse(i64),
    NotStored,
    Unauthorized,
    Api(MikrotikError),
    Db(DbError),
}

impl MikrotikManagerError {
    fn kind(&self) -> &'static str {
        match self {
            Self::AlreadyRunningForProfile(_) => "already-running",
            Self::NoActiveSession => "no-active-session",
            Self::TooManySessions => "too-many-sessions",
            Self::ProfileNotFound(_) => "profile-not-found",
            Self::SessionNotFound(_) => "session-not-found",
            Self::ProfileInUse(_) => "profile-in-use",
            Self::NotStored => "not-stored",
            Self::Unauthorized => "unauthorized",
            Self::Api(_) => "api",
            Self::Db(_) => "db",
        }
    }
}

impl fmt::Display for MikrotikManagerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunningForProfile(id) => {
                write!(f, "profile {id} is already being monitored")
            }
            Self::NoActiveSession => f.write_str("no mikrotik session is running with that id"),
            Self::TooManySessions => {
                f.write_str("too many concurrent mikrotik sessions — disconnect one first")
            }
            Self::ProfileNotFound(id) => write!(f, "no mikrotik profile with id {id}"),
            Self::SessionNotFound(id) => write!(f, "no mikrotik session with id {id}"),
            Self::ProfileInUse(id) => {
                write!(f, "mikrotik profile {id} backs the active session")
            }
            Self::NotStored => f.write_str("password not stored — re-enter"),
            Self::Unauthorized => f.write_str("authentication failed (401)"),
            Self::Api(err) => write!(f, "{err}"),
            Self::Db(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for MikrotikManagerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Api(err) => Some(err),
            Self::Db(err) => Some(err),
            _ => None,
        }
    }
}

impl Serialize for MikrotikManagerError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("MikrotikManagerError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<DbError> for MikrotikManagerError {
    fn from(err: DbError) -> Self {
        Self::Db(err)
    }
}

impl From<MikrotikError> for MikrotikManagerError {
    fn from(err: MikrotikError) -> Self {
        match err {
            MikrotikError::Unauthorized => Self::Unauthorized,
            other => Self::Api(other),
        }
    }
}

impl From<SecretError> for MikrotikManagerError {
    fn from(err: SecretError) -> Self {
        match err {
            SecretError::NotStored => Self::NotStored,
            SecretError::Keyring(message) => Self::Api(MikrotikError::Connect(message)),
        }
    }
}

/// Integration point owned by todo 6: the version/firmware probe. The runtime
/// invokes it once per session start; the implementation MUST spawn its own
/// cancellable task and never gate or block the 5s polling loop.
pub type RunVersionProbe = Arc<dyn Fn(VersionProbeArgs) + Send + Sync>;

#[derive(Clone)]
pub struct VersionProbeArgs {
    pub session_id: i64,
    pub profile_id: i64,
    pub db: Arc<Database>,
    pub manager: crate::mikrotik::manager::MikrotikManager,
    pub api: Arc<dyn MikrotikApi>,
    pub on_status: MikrotikStatusSink,
    pub stop_rx: tokio::sync::watch::Receiver<bool>,
}
