//! MTU run runtime: drives the search controller against a probe engine,
//! streams events, and persists probes. See `.omo/plans/mtu-discovery.md`
//! milestone 2.

use std::sync::Arc;

use tokio::sync::watch;

use crate::db::{now_rfc3339, Database, NewMtuProbe};

use super::engine::MtuProbeEngine;
use super::manager::MtuManager;
use super::search::SearchController;
use super::types::{
    LowerBoundReason, MtuConfig, MtuError, MtuProbeEvent, MtuStatusEvent, ProbeOutcome,
    ProbeOutcomeDto, ResultKind, ResultKindDto, SearchAction, SearchResult, StoppedMtuDto,
};

pub struct MtuRunContext {
    pub manager: MtuManager,
    pub db: Arc<Database>,
    pub run_id: i64,
    pub engine: MtuProbeEngine,
    pub config: MtuConfig,
    pub on_event: Arc<dyn Fn(MtuProbeEvent) + Send + Sync>,
    pub on_status: Arc<dyn Fn(MtuStatusEvent) + Send + Sync>,
    pub stop_rx: watch::Receiver<bool>,
}

pub async fn run_mtu(ctx: MtuRunContext) -> Result<StoppedMtuDto, MtuError> {
    let MtuRunContext {
        manager,
        db,
        run_id,
        mut engine,
        config,
        on_event,
        on_status,
        stop_rx,
    } = ctx;
    let mut controller = SearchController::new(config);
    let mut action = controller.initial_action();
    let mut probes_sent = 0u64;

    loop {
        match action {
            SearchAction::Probe {
                seq,
                payload_size,
                mtu_size,
            } => {
                if *stop_rx.borrow() {
                    return cancel_run(run_id, probes_sent, &manager, &db, &on_status).await;
                }
                (on_event)(MtuProbeEvent::Attempt {
                    seq,
                    payload_size,
                    mtu_size,
                });
                let outcome = engine.probe(payload_size).await;
                probes_sent = probes_sent.max(seq);
                let outcome_dto = ProbeOutcomeDto::from(outcome.clone());
                (on_event)(MtuProbeEvent::Outcome {
                    seq,
                    outcome: outcome_dto,
                });
                let probe = probe_row(seq, payload_size, mtu_size, &outcome);
                let _ = db.add_mtu_probe(run_id, &probe).await;
                action = controller.step(outcome);
            }
            SearchAction::Done(result) => {
                let dto = finish_run(run_id, result, &manager, &db, &on_status).await?;
                return Ok(dto);
            }
        }
    }
}

async fn cancel_run(
    run_id: i64,
    probes_sent: u64,
    manager: &MtuManager,
    db: &Database,
    on_status: &Arc<dyn Fn(MtuStatusEvent) + Send + Sync>,
) -> Result<StoppedMtuDto, MtuError> {
    let ended_at = now_rfc3339();
    db.finish_mtu_run(
        run_id,
        "cancelled",
        None,
        None,
        i64::try_from(probes_sent).unwrap_or(i64::MAX),
        &ended_at,
    )
    .await?;
    on_status(MtuStatusEvent::Cancelled {
        run_id,
        probes_sent,
    });
    manager.clear_active(run_id).await;
    Ok(StoppedMtuDto {
        run_id,
        probes_sent,
    })
}

async fn finish_run(
    run_id: i64,
    result: SearchResult,
    manager: &MtuManager,
    db: &Database,
    on_status: &Arc<dyn Fn(MtuStatusEvent) + Send + Sync>,
) -> Result<StoppedMtuDto, MtuError> {
    let ended_at = now_rfc3339();
    let (result_kind, result_mtu, detail) = result_kind_to_row(&result.kind);
    db.finish_mtu_run(
        run_id,
        result_kind,
        result_mtu,
        detail.as_deref(),
        i64::try_from(result.probes_sent).unwrap_or(i64::MAX),
        &ended_at,
    )
    .await?;
    on_status(MtuStatusEvent::Completed {
        run_id,
        result: ResultKindDto::from(&result.kind),
        probes_sent: result.probes_sent,
    });
    manager.clear_active(run_id).await;
    Ok(StoppedMtuDto {
        run_id,
        probes_sent: result.probes_sent,
    })
}

fn result_kind_to_row(kind: &ResultKind) -> (&'static str, Option<i64>, Option<String>) {
    match kind {
        ResultKind::Exact { mtu } => ("exact", Some(i64::from(*mtu)), None),
        ResultKind::LowerBound { mtu, reason } => {
            let detail = match reason {
                LowerBoundReason::TimeoutAbove { tried_mtu } => {
                    format!("timeout-above:{tried_mtu}")
                }
                LowerBoundReason::CeilingReached => "ceiling".to_owned(),
            };
            ("lower-bound", Some(i64::from(*mtu)), Some(detail))
        }
        ResultKind::Unreachable => ("unreachable", None, None),
        ResultKind::Failed { message } => ("failed", None, Some(message.clone())),
    }
}

fn probe_row(seq: u64, payload_size: usize, mtu_size: u32, outcome: &ProbeOutcome) -> NewMtuProbe {
    let (outcome_name, rtt_ms, hint_mtu, message) = match outcome {
        ProbeOutcome::Ok { rtt } => ("ok", Some(rtt.as_secs_f64() * 1000.0), None, None),
        ProbeOutcome::TooBig { hint_mtu } => ("too-big", None, hint_mtu.map(i64::from), None),
        ProbeOutcome::Timeout => ("timeout", None, None, None),
        ProbeOutcome::Error(message) => ("error", None, None, Some(message.clone())),
    };
    NewMtuProbe {
        seq: i64::try_from(seq).unwrap_or(i64::MAX),
        payload_size: i64::try_from(payload_size).unwrap_or(i64::MAX),
        mtu_size: i64::from(mtu_size),
        outcome: outcome_name.to_owned(),
        rtt_ms,
        hint_mtu,
        message,
        at: now_rfc3339(),
    }
}
