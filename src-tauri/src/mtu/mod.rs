//! Path MTU discovery: probe engines, a pure search controller, the session
//! manager with SQLite history, and the Tauri command surface.
//! See `.omo/plans/mtu-discovery.md`.

pub mod engine;
pub mod manager;
pub mod runtime;
pub mod search;
pub mod tcp_probe;
pub mod types;

pub use types::{
    LoadedMtuRunDto, LowerBoundReasonDto, MtuConfig, MtuError, MtuMethod, MtuProbeDto,
    MtuProbeEvent, MtuRunSummaryDto, MtuStatusEvent, ProbeOutcome, ProbeOutcomeDto, ResultKind,
    ResultKindDto, SearchAction, SearchResult, StartMtuDto, StoppedMtuDto,
};
