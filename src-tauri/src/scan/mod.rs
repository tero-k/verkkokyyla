pub mod arp;
pub mod cidr;
pub mod discovery;
pub mod interfaces;
pub mod manager;
pub mod oui;
pub mod ports;
pub mod rdns;

use std::fmt;

use serde::Serialize;

pub use interfaces::InterfaceDto;
pub use manager::ScanManager;

use crate::db::{DbError, ScanHostRow, ScanSummary};
use crate::scan::ports::OpenPort;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ScanEvent {
    Host {
        ip: String,
        mac: Option<String>,
        vendor: Option<String>,
        hostname: Option<String>,
        found_by: String,
        open_ports: Vec<OpenPort>,
        at: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ScanStatusEvent {
    Engine { engine: String, tcp_fallback: bool },
    Progress { done: u64, total: u64 },
    Stopped { scan_id: i64, host_count: u64 },
    Completed { scan_id: i64, host_count: u64 },
    Error { message: String },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartScanDto {
    pub scan_id: i64,
    pub interface_name: String,
    pub cidr: String,
    pub tcp_fallback: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoppedScanDto {
    pub scan_id: i64,
    pub host_count: u64,
    pub ended_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanSummaryDto {
    pub id: i64,
    pub interface_name: String,
    pub cidr: String,
    pub tcp_fallback: bool,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub host_count: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanHostDto {
    pub ip: String,
    pub mac: Option<String>,
    pub vendor: Option<String>,
    pub hostname: Option<String>,
    pub found_by: String,
    pub open_ports: Vec<OpenPort>,
    pub at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedScanDto {
    pub scan: ScanSummaryDto,
    pub hosts: Vec<ScanHostDto>,
}

#[derive(Debug)]
pub enum ScanError {
    AlreadyRunning,
    NoActiveScan,
    ScanNotFound(i64),
    Cidr(cidr::CidrError),
    Arp(arp::ArpError),
    Interface(interfaces::InterfaceError),
    Db(DbError),
}

impl ScanError {
    fn kind(&self) -> &'static str {
        match self {
            Self::AlreadyRunning => "already-running",
            Self::NoActiveScan => "no-active-scan",
            Self::ScanNotFound(_) => "scan-not-found",
            Self::Cidr(_) => "cidr",
            Self::Arp(_) => "arp",
            Self::Interface(_) => "interface",
            Self::Db(_) => "db",
        }
    }
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => f.write_str("a scan is already running"),
            Self::NoActiveScan => f.write_str("no scan is running"),
            Self::ScanNotFound(id) => write!(f, "no scan with id {id}"),
            Self::Cidr(err) => write!(f, "{err}"),
            Self::Arp(err) => write!(f, "{err}"),
            Self::Interface(err) => write!(f, "{err}"),
            Self::Db(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ScanError {}

impl Serialize for ScanError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ScanError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<cidr::CidrError> for ScanError {
    fn from(err: cidr::CidrError) -> Self {
        Self::Cidr(err)
    }
}
impl From<arp::ArpError> for ScanError {
    fn from(err: arp::ArpError) -> Self {
        Self::Arp(err)
    }
}
impl From<interfaces::InterfaceError> for ScanError {
    fn from(err: interfaces::InterfaceError) -> Self {
        Self::Interface(err)
    }
}
impl From<DbError> for ScanError {
    fn from(err: DbError) -> Self {
        Self::Db(err)
    }
}

impl From<ScanSummary> for ScanSummaryDto {
    fn from(row: ScanSummary) -> Self {
        Self {
            id: row.id,
            interface_name: row.interface_name,
            cidr: row.cidr,
            tcp_fallback: row.tcp_fallback,
            started_at: row.started_at,
            ended_at: row.ended_at,
            status: row.status,
            host_count: row.host_count,
        }
    }
}

impl From<ScanHostRow> for ScanHostDto {
    fn from(row: ScanHostRow) -> Self {
        Self {
            ip: row.ip,
            mac: row.mac,
            vendor: row.vendor,
            hostname: row.hostname,
            found_by: row.found_by,
            open_ports: serde_json::from_str(&row.open_ports).unwrap_or_default(),
            at: row.at,
        }
    }
}
