//! Ping engine layer: enum-dispatched engines, DNS resolution, zone-id
//! parsing, and the continuous probe loop.
//!
//! Engine SELECTION policy (privilege probe, fallback choice) is the session
//! layer's job (todo 7); this module only provides the engines. Dispatch is
//! an enum with an inherent `async fn probe` — no async-trait crate, no
//! trait objects.

mod dns;
#[cfg(test)]
mod mock;
mod surge;
mod zone;

pub use dns::{resolve_target, Family, ResolveResult};
#[cfg(test)]
pub use mock::MockPinger;
pub use surge::SurgePinger;
pub use zone::{parse_ipv6_with_scope, ParseError};

use std::fmt;
use std::time::Duration;

use tokio::sync::{mpsc, watch};

/// Delay between the start of consecutive probes.
pub const PING_INTERVAL_MS: u64 = 1000;
/// Per-probe reply timeout.
pub const PING_TIMEOUT_MS: u64 = 1000;
/// ICMP echo payload size in bytes.
pub const PING_PAYLOAD_BYTES: usize = 32;

/// Outcome of a single probe, produced by any engine.
/// (The session layer maps this onto `stats::ProbeOutcome`.)
#[derive(Clone, Debug, PartialEq)]
pub enum ProbeResult {
    /// Successful reply with its round-trip time.
    Rtt(Duration),
    /// No reply within the probe timeout.
    Timeout,
    /// Engine/transport error (counts as loss).
    Error(String),
}

/// Errors produced while resolving a target or creating an engine.
#[derive(Debug)]
pub enum EngineError {
    /// A scoped-IPv6 literal failed to parse.
    Parse(ParseError),
    /// The DNS lookup itself failed.
    Resolve { input: String, source: std::io::Error },
    /// The lookup succeeded but no answer matched the requested family.
    NoAnswer { input: String, family: Family },
    /// Engine socket/client creation failed (e.g. permission denied).
    Socket(std::io::Error),
    /// A required platform facility is unavailable (e.g. no ping binary).
    Unavailable(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Parse(err) => write!(f, "{err}"),
            EngineError::Resolve { input, source } => {
                write!(f, "failed to resolve {input:?}: {source}")
            }
            EngineError::NoAnswer { input, family } => {
                write!(f, "no {family:?} answer for {input:?}")
            }
            EngineError::Socket(err) => write!(f, "failed to create ping socket: {err}"),
            EngineError::Unavailable(what) => write!(f, "engine unavailable: {what}"),
        }
    }
}

impl std::error::Error for EngineError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            EngineError::Parse(err) => Some(err),
            EngineError::Resolve { source, .. } => Some(source),
            EngineError::Socket(err) => Some(err),
            EngineError::NoAnswer { .. } | EngineError::Unavailable(_) => None,
        }
    }
}

/// The ping engines. Enum dispatch only — no trait objects.
pub enum PingEngine {
    Surge(SurgePinger),
    #[cfg(test)]
    Mock(MockPinger),
}

impl PingEngine {
    /// Send one echo with sequence number `seq`.
    pub async fn probe(&mut self, seq: u64) -> ProbeResult {
        match self {
            PingEngine::Surge(pinger) => pinger.probe(seq).await,
            #[cfg(test)]
            PingEngine::Mock(pinger) => pinger.probe(seq),
        }
    }
}

/// One probe emitted by [`run_probe_loop`].
#[derive(Clone, Debug, PartialEq)]
pub struct LoopProbe {
    /// 1-based sequence number within the session.
    pub seq: u64,
    pub result: ProbeResult,
}

/// Continuously probe at [`PING_INTERVAL_MS`] until the `stop` watch channel
/// flips to `true` (or the sink is dropped). Each probe's outcome is sent to
/// `sink`. The session layer (todo 7) owns the watch sender and the sink.
pub async fn run_probe_loop(
    engine: &mut PingEngine,
    stop: &mut watch::Receiver<bool>,
    sink: &mpsc::Sender<LoopProbe>,
) {
    let interval = Duration::from_millis(PING_INTERVAL_MS);
    let mut seq = 0u64;
    loop {
        if *stop.borrow() {
            break;
        }
        seq += 1;
        let result = engine.probe(seq).await;
        if sink.send(LoopProbe { seq, result }).await.is_err() {
            break;
        }
        tokio::select! {
            biased;
            _ = stop.changed() => {
                if *stop.borrow() {
                    break;
                }
            }
            () = tokio::time::sleep(interval) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Given a mock engine with a scripted sequence,
    // When probed through the PingEngine enum,
    // Then outcomes replay in order and an exhausted script yields Timeout.
    #[tokio::test]
    async fn mock_pinger_feeds_scripted_outcomes_in_order() {
        let script = vec![
            ProbeResult::Rtt(Duration::from_millis(3)),
            ProbeResult::Timeout,
            ProbeResult::Error("boom".to_owned()),
        ];
        let mut engine = PingEngine::Mock(MockPinger::new(script));

        assert_eq!(
            engine.probe(1).await,
            ProbeResult::Rtt(Duration::from_millis(3))
        );
        assert_eq!(engine.probe(2).await, ProbeResult::Timeout);
        assert_eq!(
            engine.probe(3).await,
            ProbeResult::Error("boom".to_owned())
        );
        // Script exhausted -> Timeout.
        assert_eq!(engine.probe(4).await, ProbeResult::Timeout);
    }

    // Given a mock engine whose script ends after 3 probes,
    // When the loop runs with a stop watch,
    // Then exactly the 3 scripted probes are emitted and the loop stops
    // promptly once the flag flips.
    #[tokio::test]
    async fn pinger_loop_emits_scripted_results_then_stops() {
        let script = vec![
            ProbeResult::Rtt(Duration::from_millis(1)),
            ProbeResult::Timeout,
            ProbeResult::Rtt(Duration::from_millis(2)),
        ];
        let mut engine = PingEngine::Mock(MockPinger::new(script));
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let (tx, mut rx) = mpsc::channel(16);

        let collector = tokio::spawn(async move {
            let mut got = Vec::new();
            while let Some(probe) = rx.recv().await {
                let done = got.len() == 2;
                got.push(probe);
                if done {
                    stop_tx.send(true).expect("stop send");
                }
            }
            got
        });

        run_probe_loop(&mut engine, &mut stop_rx, &tx).await;
        drop(tx);
        let got = collector.await.expect("collector");

        assert_eq!(got.len(), 3);
        assert_eq!(got[0].seq, 1);
        assert_eq!(got[2].seq, 3);
        assert_eq!(got[0].result, ProbeResult::Rtt(Duration::from_millis(1)));
        assert_eq!(got[1].result, ProbeResult::Timeout);
    }

    // Given the stop flag already set before the loop starts,
    // When the loop runs,
    // Then no probe is emitted at all.
    #[tokio::test]
    async fn pinger_loop_with_stop_preset_emits_nothing() {
        let mut engine = PingEngine::Mock(MockPinger::new(vec![ProbeResult::Timeout]));
        let (_stop_tx, mut stop_rx) = watch::channel(true);
        let (tx, mut rx) = mpsc::channel(16);

        run_probe_loop(&mut engine, &mut stop_rx, &tx).await;
        drop(tx);
        assert!(rx.recv().await.is_none());
    }
}
