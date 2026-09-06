use std::collections::VecDeque;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::dns::bench::stats::{Sample, SampleOutcome};
use crate::dns::client::ResolverEndpointDto;
use crate::dns::error::DnsError;
use crate::dns::query::{query_once, QueryOpts, RecordTypeSpec};

const HARD_CONCURRENCY_CAP: usize = 64;
const DEFAULT_SAMPLE_CAP: usize = 100_000;
const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(500);

/// Configuration for one benchmark scheduler run.
#[derive(Debug, Clone)]
pub struct SchedulerConfig {
    pub endpoint: ResolverEndpointDto,
    pub query_name: String,
    pub record_type: RecordTypeSpec,
    pub concurrency: usize,
    pub timeout: Duration,
    pub sample_cap: usize,
}

impl SchedulerConfig {
    /// Clamp concurrency to the hard cap and echo the effective value.
    pub fn effective(&self) -> SchedulerConfig {
        let mut s = self.clone();
        s.concurrency = s.concurrency.min(HARD_CONCURRENCY_CAP);
        s.sample_cap = s.sample_cap.max(1).min(DEFAULT_SAMPLE_CAP);
        s
    }
}

/// One cell of samples collected for a single (resolver × profile) measurement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SampleCell {
    pub target: String,
    pub samples: Vec<Sample>,
    pub truncated: bool,
}

/// Benchmark scheduler that issues DNS queries with bounded concurrency.
pub struct BenchScheduler {
    config: SchedulerConfig,
    cancel: CancellationToken,
}

impl BenchScheduler {
    pub fn new(config: SchedulerConfig) -> Self {
        Self {
            config: config.effective(),
            cancel: CancellationToken::new(),
        }
    }

    pub fn with_cancel(mut self, token: CancellationToken) -> Self {
        self.cancel = token;
        self
    }

    /// Run `total_queries` queries and return the final cell.
    ///
    /// `on_cell` receives aggregate snapshots at most every `SNAPSHOT_INTERVAL`.
    pub async fn run<F>(self, total_queries: usize, mut on_cell: F) -> Result<SampleCell, DnsError>
    where
        F: FnMut(SampleCell) + Send,
    {
        if total_queries == 0 {
            return Ok(SampleCell {
                target: self.config.endpoint.name.clone(),
                samples: Vec::new(),
                truncated: false,
            });
        }

        let concurrency = self.config.concurrency;
        let sem = Arc::new(Semaphore::new(concurrency));
        let mut remaining = total_queries;
        let mut in_flight = tokio::task::JoinSet::new();
        let mut ring: VecDeque<Sample> = VecDeque::with_capacity(self.config.sample_cap);
        let mut truncated = false;
        let mut last_snapshot = Instant::now();

        let endpoint = self.config.endpoint;
        let query_name = self.config.query_name;
        let record_type = self.config.record_type;
        let timeout = self.config.timeout;

        loop {
            // Start new queries up to concurrency while work remains.
            while remaining > 0 && in_flight.len() < concurrency && !self.cancel.is_cancelled() {
                let sem = Arc::clone(&sem);
                let endpoint = endpoint.clone();
                let name = query_name.clone();
                let rtype = record_type;
                let cancel = self.cancel.clone();
                in_flight.spawn(async move {
                    let _permit = sem.acquire().await.expect("semaphore never closed");
                    if cancel.is_cancelled() {
                        return Sample {
                            latency_ms: 0.0,
                            outcome: SampleOutcome::Timeout,
                            cold_conn: false,
                        };
                    }
                    let opts = QueryOpts {
                        timeout,
                        ..QueryOpts::default()
                    };
                    let start = Instant::now();
                    match query_once(&endpoint, &name, rtype, opts).await {
                        Ok(_) => Sample {
                            latency_ms: start.elapsed().as_secs_f64() * 1000.0,
                            outcome: SampleOutcome::Ok,
                            cold_conn: false,
                        },
                        Err(e) => Sample {
                            latency_ms: start.elapsed().as_secs_f64() * 1000.0,
                            outcome: classify_error(&e),
                            cold_conn: false,
                        },
                    }
                });
                remaining -= 1;
            }

            if in_flight.is_empty() {
                break;
            }

            if let Some(Ok(sample)) = in_flight.join_next().await {
                if ring.len() >= self.config.sample_cap {
                    ring.pop_front();
                    truncated = true;
                }
                ring.push_back(sample);

                if last_snapshot.elapsed() >= SNAPSHOT_INTERVAL {
                    on_cell(SampleCell {
                        target: endpoint.name.clone(),
                        samples: ring.iter().copied().collect(),
                        truncated,
                    });
                    last_snapshot = Instant::now();
                }
            } else {
                break;
            }
        }

        Ok(SampleCell {
            target: endpoint.name,
            samples: ring.iter().copied().collect(),
            truncated,
        })
    }
}

fn classify_error(err: &DnsError) -> SampleOutcome {
    match err.kind() {
        "timeout" => SampleOutcome::Timeout,
        "servfail" => SampleOutcome::Servfail,
        "refused" => SampleOutcome::Refused,
        "nxdomain" => SampleOutcome::Nxdomain,
        _ => SampleOutcome::NoData,
    }
}
