//! MTU discovery types: probe outcomes, search tuning, search actions and
//! results, and the serialized DTO seam for the Tauri layer.
//!
//! This module is the CONTRACT between `mtu::search` (the pure search
//! controller) and `mtu::engine` (the per-probe I/O engines). Do not add
//! I/O, sockets, or async here.

use std::fmt;
use std::time::Duration;

use serde::Serialize;

/// IPv4 header (20) + ICMP echo header (8) overhead: IP MTU = payload + 28.
pub const IPV4_ICMP_OVERHEAD: u32 = 28;
/// Default lower MTU bound (IPv4 minimum reassembly buffer).
pub const DEFAULT_FLOOR_MTU: u32 = 576;
/// Default upper MTU bound (jumbo Ethernet).
pub const DEFAULT_CEILING_MTU: u32 = 9000;
/// Largest ceiling the search accepts (jumbo-frames variants up to 10G
/// Ethernet's 10240-byte frame payload).
pub const MAX_CEILING_MTU: u32 = 10240;
/// Payload used to verify reachability before any size probing.
pub const BASELINE_PAYLOAD: usize = 56;
/// How many probes per size are tolerated before a Timeout is believed.
pub const DEFAULT_CONFIRM_PROBES: u8 = 2;

/// Outcome of one probe at one payload size, produced by an engine.
#[derive(Clone, Debug, PartialEq)]
pub enum ProbeOutcome {
    /// Echo reply received; carries the round-trip time.
    Ok { rtt: Duration },
    /// The packet was too big for the path (ICMP Frag Needed with DF, or the
    /// OS refused to send). `hint_mtu` is the RFC 1191 next-hop MTU when the
    /// engine could extract it.
    TooBig { hint_mtu: Option<u32> },
    /// No reply within the timeout. NOT evidence of "too big".
    Timeout,
    /// Engine/transport failure. Ends the run; engines should map transient
    /// conditions to `Timeout` instead of ending the search.
    Error(String),
}

impl ProbeOutcome {
    pub fn is_ok(&self) -> bool {
        matches!(self, ProbeOutcome::Ok { .. })
    }
}

/// Serialized mirror of [`ProbeOutcome`] for Tauri channels and history.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "outcome",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ProbeOutcomeDto {
    Ok { rtt_ms: f64 },
    TooBig { hint_mtu: Option<u32> },
    Timeout,
    Error { message: String },
}

impl From<ProbeOutcome> for ProbeOutcomeDto {
    fn from(outcome: ProbeOutcome) -> Self {
        match outcome {
            ProbeOutcome::Ok { rtt } => ProbeOutcomeDto::Ok {
                rtt_ms: rtt.as_secs_f64() * 1000.0,
            },
            ProbeOutcome::TooBig { hint_mtu } => ProbeOutcomeDto::TooBig { hint_mtu },
            ProbeOutcome::Timeout => ProbeOutcomeDto::Timeout,
            ProbeOutcome::Error(message) => ProbeOutcomeDto::Error { message },
        }
    }
}

/// Search tuning for one MTU discovery run.
#[derive(Clone, Debug, PartialEq)]
pub struct MtuConfig {
    /// Lower MTU bound; probing never goes below this.
    pub floor_mtu: u32,
    /// Upper MTU bound; probing never goes above this.
    pub ceiling_mtu: u32,
    /// Probe attempts per size before a `Timeout` is believed.
    pub confirm_probes: u8,
    /// Payload size for the reachability baseline.
    pub baseline_payload: usize,
}

impl Default for MtuConfig {
    fn default() -> Self {
        Self {
            floor_mtu: DEFAULT_FLOOR_MTU,
            ceiling_mtu: DEFAULT_CEILING_MTU,
            confirm_probes: DEFAULT_CONFIRM_PROBES,
            baseline_payload: BASELINE_PAYLOAD,
        }
    }
}

impl MtuConfig {
    /// Largest legal payload (MTU floor minus headers).
    pub fn floor_payload(&self) -> usize {
        self.floor_mtu.saturating_sub(IPV4_ICMP_OVERHEAD) as usize
    }

    /// Largest legal payload (MTU ceiling minus headers).
    pub fn ceiling_payload(&self) -> usize {
        self.ceiling_mtu.saturating_sub(IPV4_ICMP_OVERHEAD) as usize
    }
}

/// What the search controller wants next. The runtime feeds each `Probe`
/// outcome back via `SearchController::step`.
#[derive(Clone, Debug, PartialEq)]
pub enum SearchAction {
    /// Send one probe with this payload; IP MTU is `mtu_size`.
    Probe {
        seq: u64,
        payload_size: usize,
        mtu_size: u32,
    },
    /// The search is finished; no further probes.
    Done(SearchResult),
}

/// Why a run ended without an exact MTU.
#[derive(Clone, Debug, PartialEq)]
pub enum ResultKind {
    /// Adjacent payload sizes bracket the MTU exactly: largest payload that
    /// passed + 28.
    Exact { mtu: u32 },
    /// Path MTU is at least `mtu`, but the search could not pin it down.
    LowerBound { mtu: u32, reason: LowerBoundReason },
    /// The target did not answer even the small baseline probes.
    Unreachable,
    /// The engine failed; carries the verbatim engine message.
    Failed { message: String },
}

/// Reason for a [`ResultKind::LowerBound`].
#[derive(Clone, Debug, PartialEq)]
pub enum LowerBoundReason {
    /// Probes above `mtu` persistently timed out (no Frag Needed replies);
    /// ICMP filtering or a black hole is suspected.
    TimeoutAbove { tried_mtu: u32 },
    /// Every size up to the configured ceiling passed; the path MTU is at
    /// least the ceiling.
    CeilingReached,
}

/// Final result of one search run.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchResult {
    pub kind: ResultKind,
    pub probes_sent: u64,
}

/// Probe method for a run: classic ICMP DF probing, or the TCP PLPMTUD
/// fallback for paths where ICMP is filtered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MtuMethod {
    Icmp,
    Tcp,
}

impl MtuMethod {
    pub fn parse(input: &str) -> Option<Self> {
        match input {
            "icmp" => Some(MtuMethod::Icmp),
            "tcp" => Some(MtuMethod::Tcp),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            MtuMethod::Icmp => "icmp",
            MtuMethod::Tcp => "tcp",
        }
    }
}

/// Streaming event for one probe attempt or its outcome.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MtuProbeEvent {
    Attempt {
        seq: u64,
        payload_size: usize,
        mtu_size: u32,
    },
    Outcome {
        seq: u64,
        outcome: ProbeOutcomeDto,
    },
}

/// Lifecycle event for one MTU discovery run.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "event",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum MtuStatusEvent {
    Completed {
        run_id: i64,
        result: ResultKindDto,
        probes_sent: u64,
    },
    Cancelled {
        run_id: i64,
        probes_sent: u64,
    },
    Error {
        message: String,
    },
}

/// Serialized form of [`ResultKind`] for channels and history.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum ResultKindDto {
    Exact {
        mtu: u32,
    },
    LowerBound {
        mtu: u32,
        reason: LowerBoundReasonDto,
    },
    Unreachable,
    Failed {
        message: String,
    },
}

/// Serialized form of [`LowerBoundReason`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(
    tag = "reason",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum LowerBoundReasonDto {
    TimeoutAbove { tried_mtu: u32 },
    CeilingReached,
}

impl From<&ResultKind> for ResultKindDto {
    fn from(kind: &ResultKind) -> Self {
        match kind {
            ResultKind::Exact { mtu } => ResultKindDto::Exact { mtu: *mtu },
            ResultKind::LowerBound { mtu, reason } => ResultKindDto::LowerBound {
                mtu: *mtu,
                reason: match reason {
                    LowerBoundReason::TimeoutAbove { tried_mtu } => {
                        LowerBoundReasonDto::TimeoutAbove {
                            tried_mtu: *tried_mtu,
                        }
                    }
                    LowerBoundReason::CeilingReached => LowerBoundReasonDto::CeilingReached,
                },
            },
            ResultKind::Unreachable => ResultKindDto::Unreachable,
            ResultKind::Failed { message } => ResultKindDto::Failed {
                message: message.clone(),
            },
        }
    }
}

/// Start-of-run acknowledgement returned by the start command.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartMtuDto {
    pub run_id: i64,
    pub method: String,
    pub resolved_ip: String,
    pub answers: Vec<String>,
}

/// Acknowledgement for a stopped run.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoppedMtuDto {
    pub run_id: i64,
    pub probes_sent: u64,
}

/// One persisted probe attempt, as loaded from history.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MtuProbeDto {
    pub seq: u64,
    pub payload_size: usize,
    pub mtu_size: u32,
    pub outcome: ProbeOutcomeDto,
    pub at: String,
}

/// Summary row of a persisted run.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MtuRunSummaryDto {
    pub id: i64,
    pub target_input: String,
    pub resolved_ip: String,
    pub method: String,
    pub result: ResultKindDto,
    pub probes_sent: u64,
    pub started_at: String,
    pub ended_at: Option<String>,
}

/// Full run: summary plus every probe, in seq order.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedMtuRunDto {
    pub run: MtuRunSummaryDto,
    pub probes: Vec<MtuProbeDto>,
}

/// Errors surfaced by the MTU manager and commands, mirroring `TraceError`.
#[derive(Debug)]
pub enum MtuError {
    AlreadyRunning,
    NoActiveRun,
    InvalidMethod(String),
    RunNotFound(i64),
    Resolve(crate::engine::EngineError),
    Engine(crate::engine::EngineError),
    Db(crate::db::DbError),
}

impl MtuError {
    fn kind(&self) -> &'static str {
        match self {
            Self::AlreadyRunning => "already-running",
            Self::NoActiveRun => "no-active-run",
            Self::InvalidMethod(_) => "invalid-method",
            Self::RunNotFound(_) => "run-not-found",
            Self::Resolve(_) => "resolve",
            Self::Engine(_) => "engine",
            Self::Db(_) => "db",
        }
    }
}

impl fmt::Display for MtuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => f.write_str("an MTU discovery run is already active"),
            Self::NoActiveRun => f.write_str("no MTU discovery run is active"),
            Self::InvalidMethod(input) => {
                write!(f, "invalid method {input:?}; expected icmp|tcp")
            }
            Self::RunNotFound(id) => write!(f, "no MTU run with id {id}"),
            Self::Resolve(err) | Self::Engine(err) => write!(f, "{err}"),
            Self::Db(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for MtuError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Resolve(err) | Self::Engine(err) => Some(err),
            Self::Db(err) => Some(err),
            _ => None,
        }
    }
}

impl Serialize for MtuError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;

        let mut state = serializer.serialize_struct("MtuError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<crate::db::DbError> for MtuError {
    fn from(err: crate::db::DbError) -> Self {
        Self::Db(err)
    }
}
impl From<crate::engine::EngineError> for MtuError {
    fn from(err: crate::engine::EngineError) -> Self {
        Self::Engine(err)
    }
}
