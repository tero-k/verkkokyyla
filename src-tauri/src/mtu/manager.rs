//! MTU session manager: owns the single active run, the stop channel, and
//! SQLite persistence. Mirrors `trace/manager.rs`.
//! See `.omo/plans/mtu-discovery.md` milestone 2.

use std::net::IpAddr;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;

use crate::db::{now_rfc3339, Database, MtuProbeRow, MtuRunSummary, NewMtuRun};
use crate::engine::{resolve_target, EngineError, Family};

use super::engine::MtuProbeEngine;
use super::runtime::{run_mtu, MtuRunContext};
use super::types::{
    LoadedMtuRunDto, LowerBoundReasonDto, MtuConfig, MtuError, MtuMethod, MtuProbeDto,
    MtuProbeEvent, MtuRunSummaryDto, MtuStatusEvent, ProbeOutcomeDto, ResultKindDto, StartMtuDto,
    StoppedMtuDto, MAX_CEILING_MTU,
};

pub type MtuFactoryFuture = BoxFuture<'static, Result<MtuProbeEngine, EngineError>>;
pub type MtuFactory = Arc<dyn Fn(MtuMethod, IpAddr, u16) -> MtuFactoryFuture + Send + Sync>;
pub type MtuStatusSink = Arc<dyn Fn(MtuStatusEvent) + Send + Sync>;

#[derive(Clone)]
pub struct MtuManager {
    db: Arc<Database>,
    factory: MtuFactory,
    inner: Arc<Mutex<MtuInner>>,
}

struct MtuInner {
    active: Option<ActiveMtu>,
}

struct ActiveMtu {
    run_id: i64,
    stop_tx: watch::Sender<bool>,
    join_handle: JoinHandle<Result<StoppedMtuDto, MtuError>>,
}

impl MtuManager {
    pub fn new(db: Database, factory: MtuFactory) -> Self {
        Self {
            db: Arc::new(db),
            factory,
            inner: Arc::new(Mutex::new(MtuInner { active: None })),
        }
    }

    pub async fn start<E>(
        &self,
        target: &str,
        method: &str,
        ceiling_mtu: u32,
        port: u16,
        on_event: E,
        on_status: MtuStatusSink,
    ) -> Result<StartMtuDto, MtuError>
    where
        E: Fn(MtuProbeEvent) + Send + Sync + 'static,
    {
        let method =
            MtuMethod::parse(method).ok_or_else(|| MtuError::InvalidMethod(method.to_owned()))?;
        let resolved = resolve_target(target, Family::V4)
            .await
            .map_err(MtuError::Resolve)?;
        let mut inner = self.inner.lock().await;
        if inner.active.is_some() {
            return Err(MtuError::AlreadyRunning);
        }

        let engine = match (self.factory)(method, resolved.selected, port).await {
            Ok(engine) => engine,
            Err(err) => {
                on_status(MtuStatusEvent::Error {
                    message: format!("{err:?}"),
                });
                return Err(MtuError::Engine(err));
            }
        };
        let config = MtuConfig {
            ceiling_mtu: ceiling_mtu.clamp(1500, MAX_CEILING_MTU),
            ..MtuConfig::default()
        };
        let run_id = self
            .db
            .create_mtu_run(&NewMtuRun {
                target_input: target.to_owned(),
                resolved_ip: resolved.selected.to_string(),
                method: method.name().to_owned(),
                floor_mtu: i64::from(config.floor_mtu),
                ceiling_mtu: i64::from(config.ceiling_mtu),
                started_at: now_rfc3339(),
            })
            .await?;
        let (stop_tx, stop_rx) = watch::channel(false);
        let manager = self.clone();
        let db = Arc::clone(&self.db);
        let on_event: Arc<dyn Fn(MtuProbeEvent) + Send + Sync> = Arc::new(on_event);
        let join_handle = tokio::spawn(async move {
            run_mtu(MtuRunContext {
                manager,
                db,
                run_id,
                engine,
                config,
                on_event,
                on_status: on_status.clone(),
                stop_rx,
            })
            .await
        });
        inner.active = Some(ActiveMtu {
            run_id,
            stop_tx,
            join_handle,
        });
        Ok(StartMtuDto {
            run_id,
            method: method.name().to_owned(),
            resolved_ip: resolved.selected.to_string(),
            answers: resolved
                .answers
                .into_iter()
                .map(|answer| answer.to_string())
                .collect(),
        })
    }

    pub async fn stop(&self) -> Result<StoppedMtuDto, MtuError> {
        let active = {
            self.inner
                .lock()
                .await
                .active
                .take()
                .ok_or(MtuError::NoActiveRun)?
        };
        let _ = active.stop_tx.send(true);
        active.join_handle.await.map_err(|err| {
            MtuError::Engine(EngineError::Unavailable(format!(
                "mtu task panicked: {err}"
            )))
        })?
    }

    pub async fn list_runs(&self) -> Result<Vec<MtuRunSummaryDto>, MtuError> {
        Ok(self
            .db
            .list_mtu_runs()
            .await?
            .into_iter()
            .map(mtu_summary_to_dto)
            .collect())
    }

    pub async fn load_run(&self, id: i64) -> Result<LoadedMtuRunDto, MtuError> {
        let run = self
            .db
            .list_mtu_runs()
            .await?
            .into_iter()
            .find(|run| run.id == id)
            .ok_or(MtuError::RunNotFound(id))?;
        let probes = self
            .db
            .load_mtu_probes(id)
            .await?
            .into_iter()
            .map(mtu_probe_to_dto)
            .collect();
        Ok(LoadedMtuRunDto {
            run: mtu_summary_to_dto(run),
            probes,
        })
    }

    pub async fn delete_run(&self, id: i64) -> Result<(), MtuError> {
        self.db.delete_mtu_run(id).await?;
        Ok(())
    }

    pub(crate) async fn clear_active(&self, run_id: i64) {
        let mut inner = self.inner.lock().await;
        if inner
            .active
            .as_ref()
            .is_some_and(|active| active.run_id == run_id)
        {
            inner.active.take();
        }
    }
}

fn mtu_summary_to_dto(row: MtuRunSummary) -> MtuRunSummaryDto {
    MtuRunSummaryDto {
        id: row.id,
        target_input: row.target_input,
        resolved_ip: row.resolved_ip,
        method: row.method,
        result: row_result_to_dto(row.result_kind, row.result_mtu, row.detail),
        probes_sent: u64::try_from(row.probes_sent).unwrap_or(0),
        started_at: row.started_at,
        ended_at: row.ended_at,
    }
}

fn row_result_to_dto(
    result_kind: String,
    result_mtu: Option<i64>,
    detail: Option<String>,
) -> ResultKindDto {
    match result_kind.as_str() {
        "exact" => ResultKindDto::Exact {
            mtu: option_i64_to_u32(result_mtu),
        },
        "lower-bound" => ResultKindDto::LowerBound {
            mtu: option_i64_to_u32(result_mtu),
            reason: lower_bound_reason_from_detail(detail.as_deref()),
        },
        "unreachable" => ResultKindDto::Unreachable,
        "failed" | "cancelled" | "running" => ResultKindDto::Failed {
            message: detail.unwrap_or(result_kind),
        },
        _ => ResultKindDto::Failed {
            message: result_kind,
        },
    }
}

fn lower_bound_reason_from_detail(detail: Option<&str>) -> LowerBoundReasonDto {
    match detail {
        Some("ceiling") => LowerBoundReasonDto::CeilingReached,
        Some(value) if value.starts_with("timeout-above:") => {
            let tried_mtu = value
                .strip_prefix("timeout-above:")
                .and_then(|raw| raw.parse::<u32>().ok())
                .unwrap_or(0);
            LowerBoundReasonDto::TimeoutAbove { tried_mtu }
        }
        Some(_) | None => LowerBoundReasonDto::CeilingReached,
    }
}

fn mtu_probe_to_dto(row: MtuProbeRow) -> MtuProbeDto {
    MtuProbeDto {
        seq: u64::try_from(row.seq).unwrap_or(0),
        payload_size: usize::try_from(row.payload_size).unwrap_or(0),
        mtu_size: u32::try_from(row.mtu_size).unwrap_or(0),
        outcome: row_probe_outcome_to_dto(row.outcome, row.rtt_ms, row.hint_mtu, row.message),
        at: row.at,
    }
}

fn row_probe_outcome_to_dto(
    outcome: String,
    rtt_ms: Option<f64>,
    hint_mtu: Option<i64>,
    message: Option<String>,
) -> ProbeOutcomeDto {
    match outcome.as_str() {
        "ok" => ProbeOutcomeDto::Ok {
            rtt_ms: rtt_ms.unwrap_or(0.0),
        },
        "too-big" => ProbeOutcomeDto::TooBig {
            hint_mtu: hint_mtu.and_then(|mtu| u32::try_from(mtu).ok()),
        },
        "timeout" => ProbeOutcomeDto::Timeout,
        "error" => ProbeOutcomeDto::Error {
            message: message.unwrap_or_else(|| "engine error".to_owned()),
        },
        _ => ProbeOutcomeDto::Error {
            message: "unknown outcome".to_owned(),
        },
    }
}

fn option_i64_to_u32(value: Option<i64>) -> u32 {
    value.and_then(|raw| u32::try_from(raw).ok()).unwrap_or(0)
}
