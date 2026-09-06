use std::fmt;
use std::net::IpAddr;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use serde::Serialize;

use crate::db::{DbError, TraceHopRow, TraceSummary};
use crate::engine::trace_parse::RawHop;
use crate::engine::{EngineError, TraceEngineError};

pub trait TraceStream: Send {
    fn next<'a>(&'a mut self) -> BoxFuture<'a, Result<Option<RawHop>, TraceEngineError>>;
}

pub type TraceFactoryFuture = BoxFuture<'static, Result<Box<dyn TraceStream>, TraceEngineError>>;
pub type TraceFactory = Arc<dyn Fn(IpAddr) -> TraceFactoryFuture + Send + Sync>;
pub type TraceResolverFuture = BoxFuture<'static, Option<String>>;
pub type TraceResolver = Arc<dyn Fn(IpAddr) -> TraceResolverFuture + Send + Sync>;
pub type TraceStatusSink = Arc<dyn Fn(TraceStatusEvent) + Send + Sync>;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum TraceEvent {
    Hop {
        hop: u32,
        address: Option<String>,
        rtt1_ms: Option<f64>,
        rtt2_ms: Option<f64>,
        rtt3_ms: Option<f64>,
        annotation: Option<String>,
        at: String,
    },
    Hostname {
        hop: u32,
        address: String,
        hostname: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum TraceStatusEvent {
    Completed {
        trace_id: i64,
        hop_count: u64,
        reached_target: bool,
    },
    Cancelled {
        trace_id: i64,
        hop_count: u64,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartTraceDto {
    pub trace_id: i64,
    pub engine: String,
    pub resolved_ip: String,
    pub answers: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoppedTraceDto {
    pub trace_id: i64,
    pub hop_count: u64,
    pub ended_at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceSummaryDto {
    pub id: i64,
    pub target_input: String,
    pub resolved_ip: String,
    pub family: String,
    pub engine: String,
    pub max_hops: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub reached_target: bool,
    pub hop_count: i64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceHopDto {
    pub hop: i64,
    pub address: Option<String>,
    pub hostname: Option<String>,
    pub rtt1_ms: Option<f64>,
    pub rtt2_ms: Option<f64>,
    pub rtt3_ms: Option<f64>,
    pub annotation: Option<String>,
    pub at: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedTraceDto {
    pub trace: TraceSummaryDto,
    pub hops: Vec<TraceHopDto>,
}

#[derive(Debug)]
pub enum TraceError {
    AlreadyRunning,
    NoActiveTrace,
    InvalidFamily(String),
    TraceNotFound(i64),
    Resolve(EngineError),
    Stream(TraceEngineError),
    Db(DbError),
}

impl TraceError {
    fn kind(&self) -> &'static str {
        match self {
            Self::AlreadyRunning => "already-running",
            Self::NoActiveTrace => "no-active-trace",
            Self::InvalidFamily(_) => "invalid-family",
            Self::TraceNotFound(_) => "trace-not-found",
            Self::Resolve(_) => "resolve",
            Self::Stream(_) => "stream",
            Self::Db(_) => "db",
        }
    }
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => f.write_str("a trace is already running"),
            Self::NoActiveTrace => f.write_str("no trace is running"),
            Self::InvalidFamily(input) => {
                write!(f, "invalid family {input:?}; expected auto|v4|v6")
            }
            Self::TraceNotFound(id) => write!(f, "no trace with id {id}"),
            Self::Resolve(err) => write!(f, "{err}"),
            Self::Stream(err) => write!(f, "{err:?}"),
            Self::Db(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for TraceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resolve(err) => Some(err),
            Self::Db(err) => Some(err),
            _ => None,
        }
    }
}

impl Serialize for TraceError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("TraceError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<DbError> for TraceError {
    fn from(err: DbError) -> Self {
        Self::Db(err)
    }
}
impl From<TraceEngineError> for TraceError {
    fn from(err: TraceEngineError) -> Self {
        Self::Stream(err)
    }
}

impl From<TraceSummary> for TraceSummaryDto {
    fn from(row: TraceSummary) -> Self {
        Self {
            id: row.id,
            target_input: row.target_input,
            resolved_ip: row.resolved_ip,
            family: row.family,
            engine: row.engine,
            max_hops: row.max_hops,
            started_at: row.started_at,
            ended_at: row.ended_at,
            status: row.status,
            reached_target: row.reached_target,
            hop_count: row.hop_count,
        }
    }
}

impl From<TraceHopRow> for TraceHopDto {
    fn from(row: TraceHopRow) -> Self {
        Self {
            hop: row.hop,
            address: row.address,
            hostname: row.hostname,
            rtt1_ms: row.rtt1_ms,
            rtt2_ms: row.rtt2_ms,
            rtt3_ms: row.rtt3_ms,
            annotation: row.annotation,
            at: row.at,
        }
    }
}
