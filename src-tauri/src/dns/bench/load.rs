use std::time::Duration;

use rand::{distributions::Distribution, SeedableRng};
use rand::rngs::StdRng;

use crate::dns::bench::mixes::QueryMix;
use crate::dns::bench::profiles::BenchmarkProfile;
use crate::dns::bench::scheduler::{BenchScheduler, SchedulerConfig, SampleCell};
use crate::dns::bench::stats::{aggregate_with_elapsed, MetricsDto};
use crate::dns::client::ResolverEndpointDto;
use crate::dns::error::DnsError;
use crate::dns::query::QueryOpts;

/// Result of a single benchmark run.
#[derive(Debug, Clone)]
pub struct LoadRunResult {
    pub profile_name: String,
    pub endpoint_name: String,
    pub elapsed: Duration,
    pub cell: SampleCell,
    pub metrics: MetricsDto,
}

/// Run `profile` against `endpoint`.
///
/// This is a pragmatic implementation: it maps the profile onto one scheduler
/// cell using the profile's base query name.  Cache-busting and weighted mixes
/// are applied by generating a single representative query name for the cell;
/// per-query mix variation is not yet supported by the cell scheduler.
pub async fn run_load<F>(
    endpoint: ResolverEndpointDto,
    profile: BenchmarkProfile,
    mut on_cell: F,
) -> Result<LoadRunResult, DnsError>
where
    F: FnMut(SampleCell) + Send,
{
    let total_queries = profile.total_queries();
    let query_name = representative_query_name(&profile);

    let config = SchedulerConfig {
        endpoint,
        query_name,
        record_type: profile.record_type,
        concurrency: profile.concurrency,
        timeout: Duration::from_secs(2),
        sample_cap: total_queries.max(1).min(100_000),
    };

    let scheduler = BenchScheduler::new(config);
    let start = tokio::time::Instant::now();
    let cell = scheduler.run(total_queries, |cell| on_cell(cell.clone())).await?;
    let elapsed = start.elapsed();

    let mut metrics = aggregate_with_elapsed(&cell.samples, elapsed);
    metrics.target = cell.target.clone();

    Ok(LoadRunResult {
        profile_name: profile.name,
        endpoint_name: cell.target.clone(),
        elapsed,
        cell,
        metrics,
    })
}

fn representative_query_name(profile: &BenchmarkProfile) -> String {
    if profile.cache_bust {
        return profile.query_name.clone();
    }
    match &profile.mix {
        QueryMix::PopularWeighted => profile.query_name.clone(),
        QueryMix::UniqueLabels { base } => {
            let mut rng = StdRng::from_entropy();
            let label: String = rand::distributions::Uniform::new_inclusive(b'a', b'z')
                .sample_iter(&mut rng)
                .take(12)
                .map(|b| b as char)
                .collect();
            format!("{label}.{base}")
        }
        QueryMix::Mixed { .. } => profile.query_name.clone(),
    }
}

pub fn query_opts_for_profile(_profile: &BenchmarkProfile) -> QueryOpts {
    QueryOpts {
        timeout: Duration::from_secs(2),
        ..QueryOpts::default()
    }
}

#[allow(dead_code)]
pub fn rate_limit_interval(qps_limit: usize) -> Option<Duration> {
    if qps_limit == 0 {
        None
    } else {
        Some(Duration::from_secs_f64(1.0 / qps_limit.max(1) as f64))
    }
}
