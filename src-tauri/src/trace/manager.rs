use std::sync::Arc;

use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;

use crate::db::{now_rfc3339, Database, NewTrace};
use crate::engine::{resolve_target, Family};

use super::runtime::{run_trace, TraceRunContext};
use super::types::{
    LoadedTraceDto, StartTraceDto, StoppedTraceDto, TraceError, TraceEvent, TraceFactory,
    TraceResolver, TraceStatusSink, TraceSummaryDto,
};

#[derive(Clone)]
pub struct TraceManager {
    db: Arc<Database>,
    factory: TraceFactory,
    resolver: TraceResolver,
    inner: Arc<Mutex<TraceInner>>,
}

struct TraceInner {
    active: Option<ActiveTrace>,
}

struct ActiveTrace {
    trace_id: i64,
    stop_tx: watch::Sender<bool>,
    join_handle: JoinHandle<Result<StoppedTraceDto, TraceError>>,
}

impl TraceManager {
    pub fn new(db: Database, factory: TraceFactory, resolver: TraceResolver) -> Self {
        Self {
            db: Arc::new(db),
            factory,
            resolver,
            inner: Arc::new(Mutex::new(TraceInner { active: None })),
        }
    }

    pub async fn start<E>(
        &self,
        target: &str,
        family: &str,
        on_event: E,
        on_status: TraceStatusSink,
    ) -> Result<StartTraceDto, TraceError>
    where
        E: Fn(TraceEvent) + Send + Sync + 'static,
    {
        let family = parse_family(family)?;
        let resolved = resolve_target(target, family)
            .await
            .map_err(TraceError::Resolve)?;
        let mut inner = self.inner.lock().await;
        if inner.active.is_some() {
            return Err(TraceError::AlreadyRunning);
        }

        let stream = match (self.factory)(resolved.selected).await {
            Ok(stream) => stream,
            Err(err) => {
                on_status(super::types::TraceStatusEvent::Error {
                    message: format!("{err:?}"),
                });
                return Err(err.into());
            }
        };

        let trace_id = self
            .db
            .create_trace(&NewTrace {
                target_input: target.to_owned(),
                resolved_ip: resolved.selected.to_string(),
                family: family_name(family).to_owned(),
                engine: "traceroute".to_owned(),
                max_hops: 30,
                started_at: now_rfc3339(),
            })
            .await?;
        let (stop_tx, stop_rx) = watch::channel(false);
        let manager = self.clone();
        let db = Arc::clone(&self.db);
        let resolver = Arc::clone(&self.resolver);
        let on_event: Arc<dyn Fn(TraceEvent) + Send + Sync> = Arc::new(on_event);
        let on_status = on_status.clone();
        let join_handle = tokio::spawn(async move {
            run_trace(
                stream,
                TraceRunContext {
                    manager,
                    db,
                    trace_id,
                    resolved_ip: resolved.selected,
                    resolver,
                    on_event,
                    on_status,
                    stop_rx,
                },
            )
            .await
        });
        inner.active = Some(ActiveTrace {
            trace_id,
            stop_tx,
            join_handle,
        });
        Ok(StartTraceDto {
            trace_id,
            engine: "traceroute".to_owned(),
            resolved_ip: resolved.selected.to_string(),
            answers: resolved
                .answers
                .into_iter()
                .map(|ip| ip.to_string())
                .collect(),
        })
    }

    pub async fn stop(&self) -> Result<StoppedTraceDto, TraceError> {
        let active = {
            self.inner
                .lock()
                .await
                .active
                .take()
                .ok_or(TraceError::NoActiveTrace)?
        };
        let _ = active.stop_tx.send(true);
        active.join_handle.await.map_err(|err| {
            TraceError::Stream(crate::engine::TraceEngineError::Spawn(format!(
                "trace task panicked: {err}"
            )))
        })?
    }

    pub async fn list_traces(&self) -> Result<Vec<TraceSummaryDto>, TraceError> {
        Ok(self
            .db
            .list_traces()
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn load_trace(&self, id: i64) -> Result<LoadedTraceDto, TraceError> {
        let trace = self
            .db
            .list_traces()
            .await?
            .into_iter()
            .find(|trace| trace.id == id)
            .ok_or(TraceError::TraceNotFound(id))?;
        let hops = self
            .db
            .load_trace_hops(id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(LoadedTraceDto {
            trace: trace.into(),
            hops,
        })
    }

    pub async fn delete_trace(&self, id: i64) -> Result<(), TraceError> {
        self.db.delete_trace(id).await?;
        Ok(())
    }

    pub(crate) async fn clear_active(&self, trace_id: i64) {
        let mut inner = self.inner.lock().await;
        if inner
            .active
            .as_ref()
            .is_some_and(|active| active.trace_id == trace_id)
        {
            inner.active.take();
        }
    }
}

fn parse_family(input: &str) -> Result<Family, TraceError> {
    match input {
        "auto" => Ok(Family::Auto),
        "v4" => Ok(Family::V4),
        "v6" => Ok(Family::V6),
        other => Err(TraceError::InvalidFamily(other.to_owned())),
    }
}

fn family_name(family: Family) -> &'static str {
    match family {
        Family::Auto => "auto",
        Family::V4 => "v4",
        Family::V6 => "v6",
    }
}
