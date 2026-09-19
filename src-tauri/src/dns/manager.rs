use std::str::FromStr;
use std::sync::Arc;

use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio::time::Instant;

use crate::db::{now_rfc3339, Database, DnsRunSummary, DnsRunTargetRow, LoadedDnsRun, NewDnsRun};
use crate::dns::bench::load::run_load;
use crate::dns::bench::profiles::BenchmarkProfile;
use crate::dns::bench::scheduler::SampleCell;
use crate::dns::bench::stats::MetricsDto;
use crate::dns::client::ResolverEndpointDto;
use crate::dns::diagnostics::{run_diagnostics, DnsDiagnosticsDto};
use crate::dns::email::{email_security_report, EmailSecurityReportDto};
use crate::dns::error::DnsError;
use crate::dns::query::{query_once, QueryOpts, QueryResultDto, RecordTypeSpec};

/// Error payload streamed back to callers when a single record-type query fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsErrorKindDto {
    pub kind: String,
    pub message: String,
}

/// One completed (or failed) record-type lookup inside a multi-type query.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupEventDto {
    pub query_name: String,
    pub record_type: String,
    pub result: Result<QueryResultDto, DnsErrorKindDto>,
}

/// Final summary returned after all record types have been processed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LookupSummaryDto {
    pub name: String,
    pub resolver: String,
    pub completed: usize,
    pub failed: usize,
    pub elapsed_ms: u64,
}

/// Persisted summary of a DNS benchmark or diagnostics run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsRunSummaryDto {
    pub id: i64,
    pub target_input: String,
    pub kind: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub target_count: i64,
}

/// Loaded DNS run with per-target metrics.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedDnsRunDto {
    pub id: i64,
    pub target_input: String,
    pub kind: String,
    pub config_json: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub targets: Vec<DnsRunTargetDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsRunTargetDto {
    pub target: String,
    pub protocol: Option<String>,
    pub metrics_json: String,
}

/// Result returned after a benchmark run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkRunDto {
    pub run_id: i64,
    pub profile_name: String,
    pub endpoint_name: String,
    pub status: String,
    pub elapsed_ms: u64,
    pub metrics: MetricsDto,
}

/// Stream a multi-record-type DNS lookup against `endpoint`.
///
/// Each parsed record type is issued through `query_once` with up to 8 concurrent
/// queries. Unknown record type strings produce a failed event and the command
/// continues. The callback `on_result` is invoked once per completed query.
pub async fn run_dns_lookup<F>(
    name: &str,
    record_types: Vec<String>,
    endpoint: ResolverEndpointDto,
    mut on_result: F,
) -> Result<LookupSummaryDto, DnsError>
where
    F: FnMut(LookupEventDto) + Send,
{
    if record_types.is_empty() {
        return Err(DnsError::InvalidInput(
            "no record types provided".to_owned(),
        ));
    }

    let start = Instant::now();
    let resolver_display = format!("{} ({})", endpoint.name, endpoint.protocol);
    let mut completed = 0usize;
    let mut failed = 0usize;

    let mut tasks = Vec::with_capacity(record_types.len());
    for rt in record_types {
        match RecordTypeSpec::from_str(&rt) {
            Ok(spec) => tasks.push((rt, spec)),
            Err(err) => {
                on_result(LookupEventDto {
                    query_name: name.to_owned(),
                    record_type: rt,
                    result: Err(DnsErrorKindDto {
                        kind: err.kind().to_owned(),
                        message: err.to_string(),
                    }),
                });
                failed += 1;
            }
        }
    }

    let semaphore = Arc::new(Semaphore::new(8));
    let mut in_flight = FuturesUnordered::new();
    for (rt, spec) in tasks {
        let sem = Arc::clone(&semaphore);
        let endpoint = endpoint.clone();
        let name = name.to_owned();
        in_flight.push(async move {
            let _permit = sem.acquire().await.expect("semaphore never closed");
            let result = query_once(
                &endpoint,
                &name,
                spec,
                QueryOpts {
                    dnssec_ok: true,
                    ..QueryOpts::default()
                },
            )
            .await;
            (rt, result)
        });
    }

    while let Some((record_type, result)) = in_flight.next().await {
        match result {
            Ok(query_result) => {
                let event_record_type = query_result.record_type.clone();
                on_result(LookupEventDto {
                    query_name: query_result.query_name.clone(),
                    record_type: event_record_type,
                    result: Ok(query_result),
                });
                completed += 1;
            }
            Err(err) => {
                on_result(LookupEventDto {
                    query_name: name.to_owned(),
                    record_type,
                    result: Err(DnsErrorKindDto {
                        kind: err.kind().to_owned(),
                        message: err.to_string(),
                    }),
                });
                failed += 1;
            }
        }
    }

    let elapsed_ms = start.elapsed().as_millis() as u64;

    Ok(LookupSummaryDto {
        name: name.to_owned(),
        resolver: resolver_display,
        completed,
        failed,
        elapsed_ms,
    })
}

/// Manager that owns DNS persistence and high-level orchestration.
pub struct DnsManager {
    db: Arc<Database>,
}

impl DnsManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Run a streamed multi-record lookup and persist it as a history run.
    ///
    /// Events are forwarded to `on_result` as they stream in and also collected
    /// so the finished run can be stored with one target row per record type.
    /// Persistence failures are logged-and-ignored: a history write must never
    /// fail the lookup itself (mirrors `dns_diagnostics` behaviour).
    pub async fn run_lookup<F>(
        &self,
        name: &str,
        record_types: Vec<String>,
        endpoint: ResolverEndpointDto,
        mut on_result: F,
    ) -> Result<LookupSummaryDto, DnsError>
    where
        F: FnMut(LookupEventDto) + Send,
    {
        let started_at = now_rfc3339();
        let collected = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&collected);
        let summary = run_dns_lookup(name, record_types, endpoint.clone(), move |event| {
            sink.lock()
                .expect("lookup event sink poisoned")
                .push(event.clone());
            on_result(event);
        })
        .await?;

        let events = std::mem::take(&mut *collected.lock().expect("lookup event sink poisoned"));
        let _ = self
            .persist_lookup(&endpoint, &summary, started_at, &events)
            .await;
        Ok(summary)
    }

    /// Persist a completed lookup run: one row per queried record type.
    async fn persist_lookup(
        &self,
        endpoint: &ResolverEndpointDto,
        summary: &LookupSummaryDto,
        started_at: String,
        events: &[LookupEventDto],
    ) -> Result<i64, DnsError> {
        let config_json = serde_json::json!({
            "name": summary.name,
            "resolver": summary.resolver,
            "recordTypes": events.iter().map(|e| e.record_type.clone()).collect::<Vec<_>>(),
            "completed": summary.completed,
            "failed": summary.failed,
            "elapsedMs": summary.elapsed_ms,
        })
        .to_string();

        let run_id = self
            .db
            .create_dns_run(&NewDnsRun {
                target_input: format!("{} via {}", summary.name, endpoint.name),
                kind: "lookup".to_string(),
                config_json,
                started_at,
            })
            .await
            .map_err(|e| DnsError::Io(e.to_string()))?;

        let status = if summary.failed > 0 {
            "partial"
        } else {
            "completed"
        };
        let targets: Vec<DnsRunTargetRow> = events
            .iter()
            .map(|event| DnsRunTargetRow {
                run_id,
                target: event.record_type.clone(),
                protocol: Some(endpoint.protocol.to_string()),
                metrics_json: serde_json::to_string(&event.result).unwrap_or_default(),
            })
            .collect();

        self.db
            .finish_dns_run(run_id, &now_rfc3339(), status, &targets)
            .await
            .map_err(|e| DnsError::Io(e.to_string()))?;
        Ok(run_id)
    }

    /// Run a consolidated diagnostics report for `domain` against `endpoint`.
    pub async fn run_diagnostics(
        &self,
        endpoint: ResolverEndpointDto,
        domain: &str,
    ) -> Result<DnsDiagnosticsDto, DnsError> {
        run_diagnostics(&endpoint, domain).await
    }

    /// Run an email-security report for `domain` against `endpoint`.
    pub async fn run_email_check(
        &self,
        endpoint: ResolverEndpointDto,
        domain: &str,
        dkim_selectors: Vec<String>,
    ) -> Result<EmailSecurityReportDto, DnsError> {
        email_security_report(&endpoint, domain, &dkim_selectors).await
    }

    /// Run a benchmark profile against an endpoint and persist the result.
    pub async fn run_benchmark<F>(
        &self,
        endpoint: ResolverEndpointDto,
        profile: BenchmarkProfile,
        on_cell: F,
    ) -> Result<BenchmarkRunDto, DnsError>
    where
        F: FnMut(SampleCell) + Send,
    {
        let target_input = format!("{} via {}", endpoint.name, endpoint.protocol);
        let config_json = serde_json::to_string(&profile).unwrap_or_default();

        let run_id = self
            .db
            .create_dns_run(&NewDnsRun {
                target_input: target_input.clone(),
                kind: "benchmark".to_string(),
                config_json: config_json.clone(),
                started_at: now_rfc3339(),
            })
            .await
            .map_err(|e| DnsError::Io(e.to_string()))?;

        let result = run_load(endpoint.clone(), profile.clone(), on_cell).await;

        match result {
            Ok(load) => {
                let metrics_json = serde_json::to_string(&load.metrics).unwrap_or_default();
                let status = "completed".to_string();
                let ended_at = now_rfc3339();
                self.db
                    .finish_dns_run(
                        run_id,
                        &ended_at,
                        &status,
                        &[DnsRunTargetRow {
                            run_id,
                            target: endpoint.name.clone(),
                            protocol: Some(endpoint.protocol.to_string()),
                            metrics_json,
                        }],
                    )
                    .await
                    .map_err(|e| DnsError::Io(e.to_string()))?;

                Ok(BenchmarkRunDto {
                    run_id,
                    profile_name: load.profile_name,
                    endpoint_name: load.endpoint_name,
                    status,
                    elapsed_ms: load.elapsed.as_millis() as u64,
                    metrics: load.metrics,
                })
            }
            Err(e) => {
                let status = format!("error: {}", e.kind());
                self.db
                    .finish_dns_run(run_id, &now_rfc3339(), &status, &[])
                    .await
                    .map_err(|inner| DnsError::Io(inner.to_string()))?;
                Err(e)
            }
        }
    }

    /// Persist a completed diagnostics run.
    pub async fn persist_diagnostics(
        &self,
        endpoint_name: &str,
        domain: &str,
        report: &DnsDiagnosticsDto,
    ) -> Result<i64, DnsError> {
        let config_json = serde_json::to_string(report).unwrap_or_default();
        let run_id = self
            .db
            .create_dns_run(&NewDnsRun {
                target_input: format!("{domain} via {endpoint_name}"),
                kind: "diagnostics".to_string(),
                config_json,
                started_at: now_rfc3339(),
            })
            .await
            .map_err(|e| DnsError::Io(e.to_string()))?;

        self.db
            .finish_dns_run(run_id, &now_rfc3339(), "completed", &[])
            .await
            .map_err(|e| DnsError::Io(e.to_string()))?;
        Ok(run_id)
    }

    pub async fn list_runs(&self) -> Result<Vec<DnsRunSummaryDto>, DnsError> {
        let rows = self
            .db
            .list_dns_runs()
            .await
            .map_err(|e| DnsError::Io(e.to_string()))?;
        Ok(rows.into_iter().map(summary_to_dto).collect())
    }

    pub async fn load_run(&self, id: i64) -> Result<LoadedDnsRunDto, DnsError> {
        let loaded = self
            .db
            .load_dns_run(id)
            .await
            .map_err(|e| DnsError::Io(e.to_string()))?;
        Ok(loaded_to_dto(loaded))
    }

    pub async fn delete_run(&self, id: i64) -> Result<(), DnsError> {
        self.db
            .delete_dns_run(id)
            .await
            .map_err(|e| DnsError::Io(e.to_string()))
    }
}

fn summary_to_dto(summary: DnsRunSummary) -> DnsRunSummaryDto {
    DnsRunSummaryDto {
        id: summary.id,
        target_input: summary.target_input,
        kind: summary.kind,
        started_at: summary.started_at,
        ended_at: summary.ended_at,
        status: summary.status,
        target_count: summary.target_count,
    }
}

fn loaded_to_dto(loaded: LoadedDnsRun) -> LoadedDnsRunDto {
    LoadedDnsRunDto {
        id: loaded.run.id,
        target_input: loaded.run.target_input,
        kind: loaded.run.kind,
        config_json: loaded.run.config_json,
        started_at: loaded.run.started_at,
        ended_at: loaded.run.ended_at,
        status: loaded.run.status,
        targets: loaded
            .targets
            .into_iter()
            .map(|t| DnsRunTargetDto {
                target: t.target,
                protocol: t.protocol,
                metrics_json: t.metrics_json,
            })
            .collect(),
    }
}
