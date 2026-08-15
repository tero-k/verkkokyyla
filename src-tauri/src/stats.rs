//! Incremental ping-session statistics engine.
//!
//! Pure std, no Tauri dependencies. Aggregates are updated in O(1) per probe
//! (`feed`) and are NEVER recomputed from the retained history; the history
//! `Vec` exists solely for later graphing.
//!
//! Algorithms:
//! - Mean / population stddev: Welford's online algorithm (no naive
//!   sum-of-squares, which is numerically unstable).
//! - Jitter: RFC 3550 (appendix A.8) estimator `J += (|D| - J) / 16`, where
//!   `D` is the difference of the transit times of two CONSECUTIVE SUCCESSFUL
//!   RTT samples. Design choice: a lost probe (Timeout or Error) breaks the
//!   `D` chain — the first successful RTT after a loss contributes no `D`
//!   update, since no adjacent successful pair exists across the gap.

use std::time::{Duration, SystemTime};

/// Outcome of a single probe, as mapped by the pinger (todo 4/5).
/// This is the single canonical input type for the stats engine.
#[derive(Clone, Debug, PartialEq)]
pub enum ProbeOutcome {
    /// Successful reply with its round-trip time.
    Rtt(Duration),
    /// No reply within the probe timeout.
    Timeout,
    /// Transport/engine error (counts as loss).
    Error(String),
}

/// One retained probe record (history for graphing; aggregates never read it).
#[derive(Clone, Debug)]
pub struct ProbeRecord {
    /// 1-based sequence number in the session.
    pub seq: u64,
    pub outcome: ProbeOutcome,
    /// Wall-clock time the outcome was recorded.
    pub at: SystemTime,
}

/// Point-in-time view of all aggregates.
///
/// `min_ms`/`avg_ms`/`max_ms`/`stddev_ms`/`jitter_ms` are `None` when they are
/// undefined (no successful RTTs; jitter additionally needs >= 2), so a
/// snapshot can never contain NaN.
#[derive(Clone, Debug, PartialEq)]
pub struct StatsSnapshot {
    /// Total probes fed (successful + lost).
    pub count: u64,
    /// Probes that timed out or errored.
    pub loss_count: u64,
    /// `loss_count / count`; 0.0 when `count == 0`.
    pub loss_fraction: f64,
    pub min_ms: Option<f64>,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<f64>,
    /// Population standard deviation of successful RTTs (Welford, M2 / n).
    pub stddev_ms: Option<f64>,
    /// RFC 3550 jitter estimator over consecutive successful RTTs.
    pub jitter_ms: Option<f64>,
}

/// O(1)-per-probe incremental statistics engine.
#[derive(Debug, Default)]
pub struct StatsEngine {
    history: Vec<ProbeRecord>,
    count: u64,
    loss_count: u64,
    rtt_count: u64,
    min_ms: Option<f64>,
    max_ms: Option<f64>,
    mean_ms: f64,
    m2_ms: f64,
    jitter_ms: Option<f64>,
    last_rtt_ms: Option<f64>,
}

impl StatsEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one probe outcome and update every aggregate incrementally.
    pub fn feed(&mut self, outcome: ProbeOutcome) {
        self.count += 1;
        let seq = self.count;

        match outcome {
            ProbeOutcome::Rtt(rtt) => {
                // as_secs_f64 keeps full precision; no `as` cast needed.
                let ms = rtt.as_secs_f64() * 1000.0;
                self.feed_rtt(ms);
            }
            ProbeOutcome::Timeout | ProbeOutcome::Error(_) => {
                self.loss_count += 1;
                // A loss breaks the RFC 3550 D chain: the next successful
                // RTT starts a new pair instead of diffing across the gap.
                self.last_rtt_ms = None;
            }
        }

        self.history.push(ProbeRecord {
            seq,
            outcome,
            at: SystemTime::now(),
        });
    }

    /// Welford + min/max + RFC 3550 jitter update for one successful RTT.
    fn feed_rtt(&mut self, ms: f64) {
        self.rtt_count += 1;
        // u64 -> f64 has no lossless From impl; `as` is the only conversion.
        let n = self.rtt_count as f64;

        let delta = ms - self.mean_ms;
        self.mean_ms += delta / n;
        let delta2 = ms - self.mean_ms;
        self.m2_ms += delta * delta2;

        self.min_ms = Some(self.min_ms.map_or(ms, |m| m.min(ms)));
        self.max_ms = Some(self.max_ms.map_or(ms, |m| m.max(ms)));

        if let Some(prev) = self.last_rtt_ms {
            // D chain: only between consecutive SUCCESSFUL samples.
            let d = (ms - prev).abs();
            let j = self.jitter_ms.unwrap_or(0.0);
            self.jitter_ms = Some(j + (d - j) / 16.0);
        }
        self.last_rtt_ms = Some(ms);
    }

    /// All aggregates at this point in time; never NaN.
    pub fn snapshot(&self) -> StatsSnapshot {
        let has_rtt = self.rtt_count > 0;
        // u64 -> f64 has no lossless From impl; `as` is the only conversion.
        let loss_fraction = if self.count == 0 {
            0.0
        } else {
            self.loss_count as f64 / self.count as f64
        };

        StatsSnapshot {
            count: self.count,
            loss_count: self.loss_count,
            loss_fraction,
            min_ms: self.min_ms,
            avg_ms: has_rtt.then_some(self.mean_ms),
            max_ms: self.max_ms,
            stddev_ms: has_rtt.then(|| (self.m2_ms / self.rtt_count as f64).sqrt()),
            jitter_ms: self.jitter_ms,
        }
    }

    /// Full per-probe history for graphing. Aggregates are never derived
    /// from this slice.
    pub fn history(&self) -> &[ProbeRecord] {
        &self.history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(value: f64) -> Duration {
        Duration::from_secs_f64(value / 1000.0)
    }

    fn feed_all(engine: &mut StatsEngine, outcomes: &[ProbeOutcome]) {
        for outcome in outcomes {
            engine.feed(outcome.clone());
        }
    }

    // Given a canned RTT series with closed-form stats,
    // When all probes are fed,
    // Then min/avg/max/stddev/jitter match the known values within 1e-9.
    #[test]
    fn canned_rtt_series_matches_known_aggregates() {
        let mut engine = StatsEngine::new();
        // RTTs: 10, 20, 30, 40 ms.
        feed_all(
            &mut engine,
            &[
                ProbeOutcome::Rtt(ms(10.0)),
                ProbeOutcome::Rtt(ms(20.0)),
                ProbeOutcome::Rtt(ms(30.0)),
                ProbeOutcome::Rtt(ms(40.0)),
            ],
        );

        let snap = engine.snapshot();
        assert_eq!(snap.count, 4);
        assert_eq!(snap.loss_count, 0);
        assert!((snap.loss_fraction - 0.0).abs() < 1e-9);

        let min = snap.min_ms.expect("min");
        let avg = snap.avg_ms.expect("avg");
        let max = snap.max_ms.expect("max");
        let stddev = snap.stddev_ms.expect("stddev");
        let jitter = snap.jitter_ms.expect("jitter");

        assert!((min - 10.0).abs() < 1e-9);
        assert!((max - 40.0).abs() < 1e-9);
        assert!((avg - 25.0).abs() < 1e-9);
        // Population stddev of {10,20,30,40}: sqrt(125).
        assert!((stddev - 125.0_f64.sqrt()).abs() < 1e-9);
        // RFC 3550 over D=10,10,10:
        // J1 = 10/16 = 0.625
        // J2 = 0.625 + (10 - 0.625)/16 = 1.2109375
        // J3 = 1.2109375 + (10 - 1.2109375)/16 = 1.76025390625
        assert!((jitter - 1.76025390625).abs() < 1e-9);
    }

    // Given 10 probes with 2 losses (one Timeout, one Error),
    // When fed,
    // Then the loss fraction is exactly 0.2 and aggregates cover only the 8
    // successful RTTs.
    #[test]
    fn two_losses_out_of_ten_probes_yield_point_two_loss_fraction() {
        let mut engine = StatsEngine::new();
        let outcomes = [
            ProbeOutcome::Rtt(ms(10.0)),
            ProbeOutcome::Timeout,
            ProbeOutcome::Rtt(ms(20.0)),
            ProbeOutcome::Rtt(ms(30.0)),
            ProbeOutcome::Rtt(ms(40.0)),
            ProbeOutcome::Error("icmp unreachable".to_owned()),
            ProbeOutcome::Rtt(ms(50.0)),
            ProbeOutcome::Rtt(ms(60.0)),
            ProbeOutcome::Rtt(ms(70.0)),
            ProbeOutcome::Rtt(ms(80.0)),
        ];
        feed_all(&mut engine, &outcomes);

        let snap = engine.snapshot();
        assert_eq!(snap.count, 10);
        assert_eq!(snap.loss_count, 2);
        assert!((snap.loss_fraction - 0.2).abs() < 1e-9);
        assert!((snap.min_ms.expect("min") - 10.0).abs() < 1e-9);
        assert!((snap.max_ms.expect("max") - 80.0).abs() < 1e-9);
        assert!((snap.avg_ms.expect("avg") - 45.0).abs() < 1e-9);
    }

    // Given RTTs separated by a loss,
    // When computing jitter,
    // Then the D chain skips the lost probe (no D update across the gap).
    #[test]
    fn jitter_chain_skips_lost_probes() {
        let mut engine = StatsEngine::new();
        feed_all(
            &mut engine,
            &[
                ProbeOutcome::Rtt(ms(10.0)),
                ProbeOutcome::Timeout,
                ProbeOutcome::Rtt(ms(20.0)),
                ProbeOutcome::Rtt(ms(35.0)),
            ],
        );

        let snap = engine.snapshot();
        // Only one valid D: |35 - 20| = 15 => J = 15/16 = 0.9375.
        // The 10 -> 20 pair is NOT used because a loss sits between them.
        assert!((snap.jitter_ms.expect("jitter") - 0.9375).abs() < 1e-9);
    }

    // Given a first successful RTT,
    // When a snapshot is taken,
    // Then jitter is None (no D pair exists yet).
    #[test]
    fn jitter_is_none_with_fewer_than_two_successful_rtts() {
        let mut engine = StatsEngine::new();
        engine.feed(ProbeOutcome::Rtt(ms(42.0)));
        assert_eq!(engine.snapshot().jitter_ms, None);
    }

    // Given 2000 probes,
    // When fed one by one,
    // Then aggregates equal the hand-computed values (proving the O(1)
    // incremental path is correct and independent of any cap) and the full
    // history is retained for graphing.
    #[test]
    fn two_thousand_probes_match_hand_computed_aggregates() {
        const N: u64 = 2000;
        let mut engine = StatsEngine::new();
        for i in 1..=N {
            // RTT = i ms exactly.
            engine.feed(ProbeOutcome::Rtt(Duration::from_millis(i)));
        }

        let snap = engine.snapshot();
        assert_eq!(snap.count, N);
        assert_eq!(snap.loss_count, 0);
        assert!((snap.loss_fraction - 0.0).abs() < 1e-9);

        // Hand-computed closed forms for RTTs 1..=2000 ms.
        let n = f64::from(u32::try_from(N).expect("fits"));
        let expected_min = 1.0;
        let expected_max = 2000.0;
        let expected_avg = (n + 1.0) / 2.0;
        // Population variance of 1..=n: (n^2 - 1) / 12.
        let expected_stddev = ((n * n - 1.0) / 12.0).sqrt();
        // Jitter over 1999 updates of D = 1: J = 1 - (15/16)^1999.
        let expected_jitter = 1.0 - (15.0_f64 / 16.0).powi(i32::try_from(N - 1).expect("fits"));

        assert!((snap.min_ms.expect("min") - expected_min).abs() < 1e-9);
        assert!((snap.max_ms.expect("max") - expected_max).abs() < 1e-9);
        assert!((snap.avg_ms.expect("avg") - expected_avg).abs() < 1e-9);
        assert!((snap.stddev_ms.expect("stddev") - expected_stddev).abs() < 1e-6);
        assert!((snap.jitter_ms.expect("jitter") - expected_jitter).abs() < 1e-9);

        // History is fully retained (graphing) even though aggregates never
        // touch it.
        let history = engine.history();
        assert_eq!(history.len(), usize::try_from(N).expect("fits"));
        assert_eq!(history.first().map(|r| r.seq), Some(1));
        assert_eq!(history.last().map(|r| r.seq), Some(N));
        assert_eq!(history[1999].outcome, ProbeOutcome::Rtt(Duration::from_millis(2000)));
    }

    // Given an all-lost session,
    // When a snapshot is taken,
    // Then every RTT aggregate is None, loss is 100%, and no float is NaN.
    #[test]
    fn all_lost_session_yields_no_nan_and_full_loss() {
        let mut engine = StatsEngine::new();
        feed_all(
            &mut engine,
            &[
                ProbeOutcome::Timeout,
                ProbeOutcome::Timeout,
                ProbeOutcome::Error("socket error".to_owned()),
                ProbeOutcome::Timeout,
                ProbeOutcome::Error("icmp unreachable".to_owned()),
            ],
        );

        let snap = engine.snapshot();
        assert_eq!(snap.count, 5);
        assert_eq!(snap.loss_count, 5);
        assert!((snap.loss_fraction - 1.0).abs() < 1e-9);
        assert!(!snap.loss_fraction.is_nan());

        // Every RTT aggregate is None; no NaN anywhere among produced floats.
        let nullable = [snap.min_ms, snap.avg_ms, snap.max_ms, snap.stddev_ms, snap.jitter_ms];
        assert!(nullable.iter().all(Option::is_none));
        assert!(nullable.into_iter().flatten().all(|v| !v.is_nan()));
    }

    // Given a fresh engine,
    // When a snapshot is taken before any probe,
    // Then it is empty, loss fraction is 0 (not NaN from 0/0), and all RTT
    // aggregates are None.
    #[test]
    fn empty_engine_snapshot_has_no_nan() {
        let snap = StatsEngine::new().snapshot();
        assert_eq!(snap.count, 0);
        assert_eq!(snap.loss_count, 0);
        assert_eq!(snap.loss_fraction, 0.0);
        assert!(!snap.loss_fraction.is_nan());
        let nullable = [snap.min_ms, snap.avg_ms, snap.max_ms, snap.stddev_ms, snap.jitter_ms];
        assert!(nullable.iter().all(Option::is_none));
    }
}
