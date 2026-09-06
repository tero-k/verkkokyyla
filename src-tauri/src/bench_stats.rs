//! Pure-std statistics helpers for benchmark result aggregation.
//!
//! Percentiles are computed from unrounded f64 samples using linear interpolation.
//! Means and standard deviations use the standard (population) formulas; Welford's
//! algorithm is not required here because samples are already available as f64 and
//! the datasets are small enough for a stable two-pass approach.

/// Summary statistics for a single metric (for example `total_ms` or `ttfb_ms`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct MetricStats {
    pub count: usize,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    /// Population standard deviation.
    pub stddev: f64,
    pub p50: f64,
    pub p90: f64,
    pub p95: f64,
    pub p99: f64,
}

impl MetricStats {
    /// Returns a fully-unspecified value used when no samples exist.
    /// Prefer `summarize` to build from data; this exists for serde defaults.
    pub fn empty() -> Self {
        Self {
            count: 0,
            min: 0.0,
            max: 0.0,
            mean: 0.0,
            stddev: 0.0,
            p50: 0.0,
            p90: 0.0,
            p95: 0.0,
            p99: 0.0,
        }
    }
}

/// Compute the `p`-th percentile (0 <= p <= 1) of a sorted slice using linear
/// interpolation between adjacent ranks.
///
/// Returns `None` for an empty slice.
pub fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    if sorted.len() == 1 {
        return Some(sorted[0]);
    }
    let pos = (sorted.len() - 1) as f64 * p.clamp(0.0, 1.0);
    let lower = pos.floor() as usize;
    let upper = pos.ceil() as usize;
    if lower == upper {
        return Some(sorted[lower]);
    }
    let frac = pos - lower as f64;
    Some(sorted[lower] * (1.0 - frac) + sorted[upper] * frac)
}

/// Summarize a collection of samples. Returns `None` when `samples` is empty.
/// The returned statistics are computed from the raw, unrounded values.
pub fn summarize(samples: &[f64]) -> Option<MetricStats> {
    if samples.is_empty() {
        return None;
    }

    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let count = sorted.len();
    let min = sorted[0];
    let max = sorted[count - 1];

    let mean = sorted.iter().sum::<f64>() / count as f64;

    let variance = if count == 1 {
        0.0
    } else {
        sorted.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / count as f64
    };
    let stddev = variance.sqrt();

    Some(MetricStats {
        count,
        min,
        max,
        mean,
        stddev,
        p50: percentile(&sorted, 0.50).unwrap_or(mean),
        p90: percentile(&sorted, 0.90).unwrap_or(mean),
        p95: percentile(&sorted, 0.95).unwrap_or(mean),
        p99: percentile(&sorted, 0.99).unwrap_or(mean),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_samples_returns_none() {
        assert!(summarize(&[]).is_none());
        assert!(percentile(&[], 0.5).is_none());
    }

    #[test]
    fn single_sample_all_stats_equal() {
        let s = summarize(&[42.0]).unwrap();
        assert_eq!(s.count, 1);
        assert_eq!(s.min, 42.0);
        assert_eq!(s.max, 42.0);
        assert_eq!(s.mean, 42.0);
        assert_eq!(s.stddev, 0.0);
        assert_eq!(s.p50, 42.0);
        assert_eq!(s.p99, 42.0);
    }

    #[test]
    fn simple_median_even_count() {
        let s = summarize(&[1.0, 2.0, 3.0, 4.0]).unwrap();
        assert_eq!(s.p50, 2.5);
    }

    #[test]
    fn percentile_linear_interpolation() {
        let v = [10.0, 20.0, 30.0, 40.0];
        assert_eq!(percentile(&v, 0.0).unwrap(), 10.0);
        assert_eq!(percentile(&v, 0.5).unwrap(), 25.0);
        assert_eq!(percentile(&v, 1.0).unwrap(), 40.0);
        // p90 of 4 samples => position 2.7 => 30 + 0.7*(40-30) = 37.0
        assert_eq!(percentile(&v, 0.90).unwrap(), 37.0);
    }

    #[test]
    fn stddev_is_population() {
        let s = summarize(&[2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0]).unwrap();
        // Population stddev of this classic set is 2.0.
        assert!((s.stddev - 2.0).abs() < 1e-9, "stddev was {}", s.stddev);
    }

    #[test]
    fn does_not_round_input() {
        let s = summarize(&[1.111_111_1, 2.222_222_2, 3.333_333_3]).unwrap();
        assert!((s.p50 - 2.222_222_2).abs() < 1e-6);
    }
}
