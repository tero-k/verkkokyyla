use std::time::Duration;

use serde::{Deserialize, Serialize};

const MIN_PERCENTILE_SAMPLES: usize = 1_000;
const SCORE_LATENCY_CAP_MS: f64 = 500.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SampleOutcome {
    Ok,
    Timeout,
    Servfail,
    Refused,
    Nxdomain,
    NoData,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sample {
    pub latency_ms: f64,
    pub outcome: SampleOutcome,
    pub cold_conn: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsDto {
    pub target: String,
    pub count: usize,
    pub min: Option<f64>,
    pub median: Option<f64>,
    pub mean: Option<f64>,
    pub max: Option<f64>,
    pub p90: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
    pub success_rate: f64,
    pub timeout_rate: f64,
    pub servfail_rate: f64,
    pub refused_rate: f64,
    pub nxdomain_rate: f64,
    pub completed_qps: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreDto {
    pub target: String,
    pub score: f64,
}

/// Percentile over sorted raw query samples using linear interpolation.
///
/// This deliberately follows the flamethrower metrics.cpp averaged-averages
/// warning: percentiles must be computed from raw per-query latencies, never
/// from averaged buckets or pre-aggregated cells.
pub fn percentile(sorted_samples: &[Sample], p: f64) -> f64 {
    match sorted_samples.len() {
        0 => 0.0,
        1 => sorted_samples[0].latency_ms,
        len => {
            let rank = (p.clamp(0.0, 100.0) / 100.0) * (len - 1) as f64;
            let lower = rank.floor() as usize;
            let upper = rank.ceil() as usize;
            let fraction = rank - lower as f64;
            let low = sorted_samples[lower].latency_ms;
            let high = sorted_samples[upper].latency_ms;
            low + (high - low) * fraction
        }
    }
}

pub fn aggregate(samples: &[Sample]) -> MetricsDto {
    aggregate_with_elapsed(samples, Duration::ZERO)
}

pub fn aggregate_with_elapsed(samples: &[Sample], elapsed: Duration) -> MetricsDto {
    let count = samples.len();
    if count == 0 {
        return MetricsDto::default();
    }

    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| left.latency_ms.total_cmp(&right.latency_ms));

    let count_f64 = count as f64;
    let mean = samples.iter().map(|sample| sample.latency_ms).sum::<f64>() / count_f64;
    let completed_qps = if elapsed.is_zero() {
        0.0
    } else {
        count_f64 / elapsed.as_secs_f64()
    };
    let (p90, p95, p99) = if count >= MIN_PERCENTILE_SAMPLES {
        (
            Some(percentile(&sorted, 90.0)),
            Some(percentile(&sorted, 95.0)),
            Some(percentile(&sorted, 99.0)),
        )
    } else {
        (None, None, None)
    };

    MetricsDto {
        target: String::new(),
        count,
        min: Some(sorted[0].latency_ms),
        median: Some(percentile(&sorted, 50.0)),
        mean: Some(mean),
        max: Some(sorted[count - 1].latency_ms),
        p90,
        p95,
        p99,
        success_rate: rate(samples, SampleOutcome::Ok),
        timeout_rate: rate(samples, SampleOutcome::Timeout),
        servfail_rate: rate(samples, SampleOutcome::Servfail),
        refused_rate: rate(samples, SampleOutcome::Refused),
        nxdomain_rate: rate(samples, SampleOutcome::Nxdomain),
        completed_qps,
    }
}

/// Reliability score for each cell.
///
/// `success_rate` is documented as the fraction of samples with
/// `SampleOutcome::Ok`. NXDOMAIN is still reported as a DNS response rate, but
/// it is not counted as a successful benchmark answer in this score.
pub fn reliability_score(cells: &[MetricsDto]) -> Vec<ScoreDto> {
    cells
        .iter()
        .map(|cell| {
            let p95 = cell.p95.or(cell.max).unwrap_or(0.0);
            let median = cell.median.unwrap_or(0.0);
            let spread = (p95 - median) / median.max(1.0);
            let score = 50.0 * cell.success_rate
                + 25.0 * (1.0 - p95.min(SCORE_LATENCY_CAP_MS) / SCORE_LATENCY_CAP_MS)
                + 15.0 * (1.0 - cell.timeout_rate)
                + 10.0 * (1.0 - spread.min(1.0));

            ScoreDto {
                target: cell.target.clone(),
                score,
            }
        })
        .collect()
}

fn rate(samples: &[Sample], outcome: SampleOutcome) -> f64 {
    samples
        .iter()
        .filter(|sample| sample.outcome == outcome)
        .count() as f64
        / samples.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f64 = 1e-9;

    fn sample(latency_ms: f64, outcome: SampleOutcome) -> Sample {
        Sample {
            latency_ms,
            outcome,
            cold_conn: false,
        }
    }

    fn ok_series(count: usize) -> Vec<Sample> {
        (1..=count)
            .map(|latency| sample(latency as f64, SampleOutcome::Ok))
            .collect()
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "{actual} != {expected}"
        );
    }

    #[test]
    fn percentile_interpolates_between_raw_samples() {
        let sorted = [
            sample(10.0, SampleOutcome::Ok),
            sample(20.0, SampleOutcome::Ok),
            sample(40.0, SampleOutcome::Ok),
            sample(80.0, SampleOutcome::Ok),
        ];

        let result = percentile(&sorted, 25.0);

        assert_close(result, 17.5);
    }

    #[test]
    fn aggregate_returns_zeroed_metrics_when_empty() {
        let metrics = aggregate(&[]);

        assert_eq!(metrics.count, 0);
        assert_eq!(metrics.median, None);
        assert_close(metrics.timeout_rate, 0.0);
        assert_close(metrics.completed_qps, 0.0);
    }

    #[test]
    fn aggregate_computes_min_median_mean_and_max_from_unsorted_raw_samples() {
        let metrics = aggregate(&[
            sample(40.0, SampleOutcome::Ok),
            sample(10.0, SampleOutcome::Ok),
            sample(30.0, SampleOutcome::Ok),
            sample(20.0, SampleOutcome::Ok),
        ]);

        assert_eq!(metrics.min, Some(10.0));
        assert_eq!(metrics.max, Some(40.0));
        assert_eq!(metrics.median, Some(25.0));
        assert_eq!(metrics.mean, Some(25.0));
    }

    #[test]
    fn aggregate_computes_outcome_rates() {
        let metrics = aggregate(&[
            sample(10.0, SampleOutcome::Ok),
            sample(20.0, SampleOutcome::Timeout),
            sample(30.0, SampleOutcome::Servfail),
            sample(40.0, SampleOutcome::Refused),
            sample(50.0, SampleOutcome::Nxdomain),
        ]);

        assert_close(metrics.timeout_rate, 0.2);
        assert_close(metrics.servfail_rate, 0.2);
        assert_close(metrics.refused_rate, 0.2);
        assert_close(metrics.nxdomain_rate, 0.2);
    }

    #[test]
    fn aggregate_all_timeout_input_has_no_nan_and_full_timeout_rate() {
        let metrics = aggregate(&[sample(2_000.0, SampleOutcome::Timeout)]);

        assert_eq!(metrics.count, 1);
        assert_eq!(metrics.median, Some(2_000.0));
        assert_close(metrics.timeout_rate, 1.0);
        assert!(metrics.mean.expect("mean").is_finite());
    }

    #[test]
    fn aggregate_999_samples_keeps_p99_none_and_median_some() {
        let samples = ok_series(999);
        let metrics = aggregate(&samples);

        assert_eq!(metrics.p99, None);
        assert_eq!(metrics.median, Some(500.0));
    }

    #[test]
    fn aggregate_1000_samples_computes_tail_percentiles() {
        let samples = ok_series(1_000);
        let metrics = aggregate(&samples);

        assert_eq!(metrics.p90, Some(900.1));
        assert_eq!(metrics.p95, Some(950.05));
        assert_eq!(metrics.p99, Some(990.01));
    }

    #[test]
    fn aggregate_with_elapsed_computes_completed_qps() {
        let metrics = aggregate_with_elapsed(&ok_series(20), Duration::from_secs(4));

        assert_close(metrics.completed_qps, 5.0);
    }

    #[test]
    fn reliability_score_matches_exact_formula_with_p95() {
        let mut metrics = aggregate(&[
            sample(100.0, SampleOutcome::Ok),
            sample(200.0, SampleOutcome::Ok),
            sample(300.0, SampleOutcome::Timeout),
            sample(400.0, SampleOutcome::Servfail),
        ]);
        metrics.target = "resolver-a".to_owned();
        metrics.p95 = Some(300.0);

        let scores = reliability_score(&[metrics]);

        assert_eq!(scores[0].target, "resolver-a");
        assert_close(scores[0].score, 54.25);
    }

    #[test]
    fn reliability_score_falls_back_to_max_when_p95_is_missing() {
        let metrics = aggregate(&[
            sample(100.0, SampleOutcome::Ok),
            sample(200.0, SampleOutcome::Ok),
        ]);

        let scores = reliability_score(&[metrics]);

        assert_close(scores[0].score, 86.66666666666667);
    }
}
