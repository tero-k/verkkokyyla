//! Pure search controller: state machine mapping probe outcomes to the next
//! probe (or the final result). No I/O - see `.omo/plans/mtu-discovery.md`.

use super::types::{
    LowerBoundReason, MtuConfig, ProbeOutcome, ResultKind, SearchAction, SearchResult,
    IPV4_ICMP_OVERHEAD,
};

/// Deterministic Path MTU search controller.
pub struct SearchController {
    config: MtuConfig,
    phase: Phase,
    next_seq: u64,
    probes_sent: u64,
}

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    Baseline(u64),
    Bracket(BracketState),
    Refine(RefineState),
    Done(SearchResult),
}

#[derive(Clone, Debug, PartialEq)]
struct BracketState {
    current_mtu: u32,
    attempts_sent: u64,
    largest_ok_mtu: u32,
}

#[derive(Clone, Debug, PartialEq)]
struct RefineState {
    low_payload: usize,
    high_payload: usize,
    current_payload: usize,
    attempts_sent: u64,
}

impl SearchController {
    pub fn new(config: MtuConfig) -> Self {
        Self {
            config,
            phase: Phase::Baseline(1),
            next_seq: 2,
            probes_sent: 1,
        }
    }

    pub fn initial_action(&self) -> SearchAction {
        SearchAction::Probe {
            seq: 1,
            payload_size: self.config.baseline_payload,
            mtu_size: payload_to_mtu(self.config.baseline_payload),
        }
    }

    pub fn step(&mut self, outcome: ProbeOutcome) -> SearchAction {
        let phase = self.phase.clone();
        match phase {
            Phase::Done(result) => SearchAction::Done(result),
            Phase::Baseline(attempts_sent) => self.step_baseline(attempts_sent, outcome),
            Phase::Bracket(state) => self.step_bracket(state, outcome),
            Phase::Refine(state) => self.step_refine(state, outcome),
        }
    }

    fn step_baseline(&mut self, attempts_sent: u64, outcome: ProbeOutcome) -> SearchAction {
        match outcome {
            ProbeOutcome::Ok { rtt: _ } => self.start_bracket(),
            ProbeOutcome::TooBig { hint_mtu } => self.finish(ResultKind::Failed {
                message: baseline_rejected_message(hint_mtu),
            }),
            ProbeOutcome::Timeout => {
                if attempts_sent < self.confirm_attempts() {
                    self.emit_probe(
                        self.config.baseline_payload,
                        Phase::Baseline(attempts_sent + 1),
                    )
                } else {
                    self.finish(ResultKind::Unreachable)
                }
            }
            ProbeOutcome::Error(message) => self.finish(ResultKind::Failed { message }),
        }
    }

    fn step_bracket(&mut self, state: BracketState, outcome: ProbeOutcome) -> SearchAction {
        match outcome {
            ProbeOutcome::Ok { rtt: _ } => {
                if state.current_mtu == self.config.ceiling_mtu {
                    self.finish(ResultKind::LowerBound {
                        mtu: self.config.ceiling_mtu,
                        reason: LowerBoundReason::CeilingReached,
                    })
                } else {
                    let next_mtu = state
                        .current_mtu
                        .saturating_mul(2)
                        .min(self.config.ceiling_mtu);
                    self.emit_bracket_probe(next_mtu, state.current_mtu)
                }
            }
            ProbeOutcome::TooBig { hint_mtu } => {
                let high_payload = self.mtu_to_payload(state.current_mtu);
                self.emit_refine_probe(
                    self.mtu_to_payload(state.largest_ok_mtu),
                    high_payload,
                    hint_mtu,
                )
            }
            ProbeOutcome::Timeout => {
                if state.attempts_sent < self.confirm_attempts() {
                    self.emit_probe(
                        self.mtu_to_payload(state.current_mtu),
                        Phase::Bracket(BracketState {
                            attempts_sent: state.attempts_sent + 1,
                            ..state
                        }),
                    )
                } else {
                    self.finish(ResultKind::LowerBound {
                        mtu: state.largest_ok_mtu,
                        reason: LowerBoundReason::TimeoutAbove {
                            tried_mtu: state.current_mtu,
                        },
                    })
                }
            }
            ProbeOutcome::Error(message) => self.finish(ResultKind::Failed { message }),
        }
    }

    fn step_refine(&mut self, state: RefineState, outcome: ProbeOutcome) -> SearchAction {
        match outcome {
            ProbeOutcome::Ok { rtt: _ } => {
                self.continue_refine(state.current_payload, state.high_payload, None)
            }
            ProbeOutcome::TooBig { hint_mtu } => {
                let high = state.high_payload.min(state.current_payload);
                self.continue_refine(state.low_payload, high, hint_mtu)
            }
            ProbeOutcome::Timeout => {
                if state.attempts_sent < self.confirm_attempts() {
                    self.emit_probe(
                        state.current_payload,
                        Phase::Refine(RefineState {
                            attempts_sent: state.attempts_sent + 1,
                            ..state
                        }),
                    )
                } else {
                    self.finish(ResultKind::LowerBound {
                        mtu: payload_to_mtu(state.low_payload),
                        reason: LowerBoundReason::TimeoutAbove {
                            tried_mtu: payload_to_mtu(state.current_payload),
                        },
                    })
                }
            }
            ProbeOutcome::Error(message) => self.finish(ResultKind::Failed { message }),
        }
    }

    fn start_bracket(&mut self) -> SearchAction {
        let mtu = 1500u32.clamp(self.config.floor_mtu, self.config.ceiling_mtu);
        self.emit_bracket_probe(mtu, self.config.floor_mtu)
    }

    fn emit_bracket_probe(&mut self, current_mtu: u32, largest_ok_mtu: u32) -> SearchAction {
        self.emit_probe(
            self.mtu_to_payload(current_mtu),
            Phase::Bracket(BracketState {
                current_mtu,
                attempts_sent: 1,
                largest_ok_mtu,
            }),
        )
    }

    fn continue_refine(
        &mut self,
        low_payload: usize,
        high_payload: usize,
        hint_mtu: Option<u32>,
    ) -> SearchAction {
        if high_payload == low_payload + 1 {
            self.finish(ResultKind::Exact {
                mtu: payload_to_mtu(low_payload),
            })
        } else {
            self.emit_refine_probe(low_payload, high_payload, hint_mtu)
        }
    }

    fn emit_refine_probe(
        &mut self,
        low_payload: usize,
        high_payload: usize,
        hint_mtu: Option<u32>,
    ) -> SearchAction {
        let current_payload = self.next_refine_payload(low_payload, high_payload, hint_mtu);
        self.emit_probe(
            current_payload,
            Phase::Refine(RefineState {
                low_payload,
                high_payload,
                current_payload,
                attempts_sent: 1,
            }),
        )
    }

    fn next_refine_payload(
        &self,
        low_payload: usize,
        high_payload: usize,
        hint_mtu: Option<u32>,
    ) -> usize {
        let hinted_payload = hint_mtu.map(|mtu| self.hint_to_payload(mtu));
        match hinted_payload {
            Some(payload) if low_payload < payload && payload < high_payload => payload,
            Some(_) | None => low_payload + ((high_payload - low_payload) / 2),
        }
    }

    fn hint_to_payload(&self, hint_mtu: u32) -> usize {
        let lower = self.config.floor_mtu.saturating_add(1);
        self.mtu_to_payload(hint_mtu.clamp(lower, self.config.ceiling_mtu))
    }

    fn mtu_to_payload(&self, mtu: u32) -> usize {
        let clamped = mtu.clamp(self.config.floor_mtu, self.config.ceiling_mtu);
        usize::try_from(clamped.saturating_sub(IPV4_ICMP_OVERHEAD))
            .unwrap_or(usize::MAX)
            .clamp(self.config.floor_payload(), self.config.ceiling_payload())
    }

    fn emit_probe(&mut self, payload_size: usize, phase: Phase) -> SearchAction {
        let action = SearchAction::Probe {
            seq: self.next_seq,
            payload_size,
            mtu_size: payload_to_mtu(payload_size),
        };
        self.next_seq += 1;
        self.probes_sent += 1;
        self.phase = phase;
        action
    }

    fn finish(&mut self, kind: ResultKind) -> SearchAction {
        let result = SearchResult {
            kind,
            probes_sent: self.probes_sent,
        };
        self.phase = Phase::Done(result.clone());
        SearchAction::Done(result)
    }

    fn confirm_attempts(&self) -> u64 {
        u64::from(self.config.confirm_probes.max(1))
    }
}

fn payload_to_mtu(payload_size: usize) -> u32 {
    u32::try_from(payload_size)
        .unwrap_or(u32::MAX)
        .saturating_add(IPV4_ICMP_OVERHEAD)
}

fn baseline_rejected_message(hint_mtu: Option<u32>) -> String {
    match hint_mtu {
        Some(mtu) => format!("baseline probe rejected: too big (hint MTU {mtu})"),
        None => "baseline probe rejected: too big".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::mtu::types::{BASELINE_PAYLOAD, MAX_CEILING_MTU};

    use super::*;

    #[derive(Debug)]
    struct ProbeRecord {
        seq: u64,
    }

    fn ok() -> ProbeOutcome {
        ProbeOutcome::Ok {
            rtt: Duration::from_millis(1),
        }
    }

    fn link_outcome(link_mtu: u32, payload_size: usize) -> ProbeOutcome {
        if payload_to_mtu(payload_size) > link_mtu {
            ProbeOutcome::TooBig { hint_mtu: None }
        } else {
            ok()
        }
    }

    fn run_to_done<F>(config: MtuConfig, mut outcome_for: F) -> (SearchResult, Vec<ProbeRecord>)
    where
        F: FnMut(usize) -> ProbeOutcome,
    {
        let mut controller = SearchController::new(config);
        let mut action = controller.initial_action();
        let mut probes = Vec::new();

        loop {
            match action {
                SearchAction::Probe {
                    seq,
                    payload_size,
                    mtu_size: _,
                } => {
                    probes.push(ProbeRecord { seq });
                    action = controller.step(outcome_for(payload_size));
                }
                SearchAction::Done(result) => return (result, probes),
            }
        }
    }

    fn assert_exact_link_mtu(link_mtu: u32) -> u64 {
        let (result, probes) = run_to_done(MtuConfig::default(), |payload| {
            link_outcome(link_mtu, payload)
        });

        assert_eq!(result.kind, ResultKind::Exact { mtu: link_mtu });
        assert!(result.probes_sent <= 24);
        assert_eq!(result.probes_sent, probes.len() as u64);
        result.probes_sent
    }

    // Given a simulated link with MTU 600,
    // When the controller is driven to completion,
    // Then it converges to the exact MTU within the probe budget.
    #[test]
    fn finds_exact_mtu_when_link_mtu_is_600() {
        assert_exact_link_mtu(600);
    }

    // Given a simulated link with MTU 1420,
    // When the controller is driven to completion,
    // Then it converges to the exact MTU within the probe budget.
    #[test]
    fn finds_exact_mtu_when_link_mtu_is_1420() {
        assert_exact_link_mtu(1420);
    }

    // Given a simulated link with MTU 1492,
    // When the controller is driven to completion,
    // Then it converges to the exact MTU within the probe budget.
    #[test]
    fn finds_exact_mtu_when_link_mtu_is_1492() {
        assert_exact_link_mtu(1492);
    }

    // Given a simulated link with MTU 1500,
    // When the controller is driven to completion,
    // Then it converges to the exact MTU within the probe budget.
    #[test]
    fn finds_exact_mtu_when_link_mtu_is_1500() {
        assert_exact_link_mtu(1500);
    }

    // Given a path that reports an RFC 1191 hint on the first TooBig,
    // When the same 1420-byte MTU link is searched,
    // Then the exact result is reached with fewer probes than without a hint.
    #[test]
    fn uses_rfc1191_hint_to_shortcut_refinement() {
        let without_hint_probes = assert_exact_link_mtu(1420);
        let mut first_too_big = true;

        let (result, probes) = run_to_done(MtuConfig::default(), |payload| {
            if payload_to_mtu(payload) > 1420 && first_too_big {
                first_too_big = false;
                ProbeOutcome::TooBig {
                    hint_mtu: Some(1420),
                }
            } else {
                link_outcome(1420, payload)
            }
        });

        assert_eq!(result.kind, ResultKind::Exact { mtu: 1420 });
        assert!(result.probes_sent < without_hint_probes);
        assert_eq!(result.probes_sent, probes.len() as u64);
    }

    // Given a link that accepts every configured probe size,
    // When the controller reaches the configured ceiling,
    // Then it reports a lower bound at the ceiling.
    #[test]
    fn reports_ceiling_reached_when_every_probe_passes() {
        let (result, _probes) = run_to_done(MtuConfig::default(), |_payload| ok());

        assert_eq!(
            result.kind,
            ResultKind::LowerBound {
                mtu: 9000,
                reason: LowerBoundReason::CeilingReached,
            }
        );
    }

    // Given a ceiling of 10240 and a link that accepts everything,
    // When the controller exhausts the search,
    // Then it reports the largest ceiling, not the clamped 9000 default.
    #[test]
    fn reports_10240_ceiling_when_every_probe_passes() {
        let config = MtuConfig {
            ceiling_mtu: MAX_CEILING_MTU,
            ..MtuConfig::default()
        };
        let (result, _probes) = run_to_done(config, |_payload| ok());

        assert_eq!(
            result.kind,
            ResultKind::LowerBound {
                mtu: 10240,
                reason: LowerBoundReason::CeilingReached,
            }
        );
    }

    // Given a target that never answers even the baseline probe,
    // When the controller exhausts baseline confirmation attempts,
    // Then it reports unreachable after exactly confirm_probes probes.
    #[test]
    fn reports_unreachable_when_baseline_times_out_repeatedly() {
        let config = MtuConfig::default();
        let (result, _probes) = run_to_done(config.clone(), |_payload| ProbeOutcome::Timeout);

        assert_eq!(result.kind, ResultKind::Unreachable);
        assert_eq!(result.probes_sent, u64::from(config.confirm_probes));
    }

    // Given the tiny baseline probe is explicitly rejected as too big,
    // When the outcome is fed into the controller,
    // Then the run fails as an unexpected baseline rejection.
    #[test]
    fn fails_when_baseline_probe_is_rejected_as_too_big() {
        let (result, _probes) = run_to_done(MtuConfig::default(), |_payload| {
            ProbeOutcome::TooBig { hint_mtu: Some(40) }
        });

        assert_eq!(
            result.kind,
            ResultKind::Failed {
                message: "baseline probe rejected: too big (hint MTU 40)".to_owned(),
            }
        );
    }

    // Given probes above the configured lower bound disappear without TooBig,
    // When timeouts persist at the first bracket size,
    // Then the controller reports only the known lower bound.
    #[test]
    fn treats_black_hole_timeouts_as_lower_bound_not_too_big() {
        let config = MtuConfig {
            floor_mtu: 1420,
            ..MtuConfig::default()
        };

        let (result, _probes) = run_to_done(config, |payload| {
            if payload_to_mtu(payload) > 1420 {
                ProbeOutcome::Timeout
            } else {
                ok()
            }
        });

        assert_eq!(
            result.kind,
            ResultKind::LowerBound {
                mtu: 1420,
                reason: LowerBoundReason::TimeoutAbove { tried_mtu: 1500 },
            }
        );
    }

    // Given a probe engine error after the baseline succeeds,
    // When the error is fed back into the controller,
    // Then the run fails with the engine message.
    #[test]
    fn fails_with_engine_error_message_mid_run() {
        let mut call_count = 0u8;
        let (result, _probes) = run_to_done(MtuConfig::default(), |_payload| {
            call_count += 1;
            match call_count {
                1 => ok(),
                2 => ProbeOutcome::Error("socket closed".to_owned()),
                _ => ok(),
            }
        });

        assert_eq!(
            result.kind,
            ResultKind::Failed {
                message: "socket closed".to_owned(),
            }
        );
    }

    // Given a completed run,
    // When step is called again with any outcome,
    // Then it returns the identical Done result without panicking.
    #[test]
    fn returns_same_done_after_completion() {
        let mut controller = SearchController::new(MtuConfig::default());

        let first_done = controller.step(ProbeOutcome::Error("boom".to_owned()));
        let second_done = controller.step(ok());

        assert_eq!(second_done, first_done);
    }

    // Given a normal MTU search,
    // When each probe is emitted across all phases,
    // Then sequence numbers are strictly increasing from one.
    #[test]
    fn emits_strictly_increasing_sequence_numbers() {
        let (_result, probes) =
            run_to_done(MtuConfig::default(), |payload| link_outcome(1492, payload));

        for (index, probe) in probes.iter().enumerate() {
            assert_eq!(probe.seq, index as u64 + 1);
        }
    }

    // Given a newly constructed controller,
    // When initial_action is requested,
    // Then the baseline probe uses the frozen baseline payload and seq 1.
    #[test]
    fn initial_action_is_the_baseline_probe() {
        let controller = SearchController::new(MtuConfig::default());

        assert_eq!(
            controller.initial_action(),
            SearchAction::Probe {
                seq: 1,
                payload_size: BASELINE_PAYLOAD,
                mtu_size: payload_to_mtu(BASELINE_PAYLOAD),
            }
        );
    }
}
