//! Test-only engine feeding scripted outcomes (no network).

use std::collections::VecDeque;

use super::ProbeResult;

/// Engine that replays a scripted sequence of [`ProbeResult`]s. Once the
/// script is exhausted every further probe yields `Timeout`.
pub struct MockPinger {
    script: VecDeque<ProbeResult>,
}

impl MockPinger {
    pub fn new(script: Vec<ProbeResult>) -> Self {
        Self {
            script: script.into(),
        }
    }

    pub fn probe(&mut self, _seq: u64) -> ProbeResult {
        self.script.pop_front().unwrap_or(ProbeResult::Timeout)
    }
}
