//! MTU probe engines: per-probe I/O producing [`ProbeOutcome`].
//! See `.omo/plans/mtu-discovery.md` milestone 1.

use std::collections::VecDeque;
use std::net::IpAddr;

#[cfg(any(unix, test))]
use std::time::Duration;

use super::tcp_probe::TcpMtuProber;
use super::types::ProbeOutcome;

#[cfg(windows)]
use crate::engine::{WinIcmpPinger, PING_TIMEOUT_MS};

#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
use tokio::process::Command;

#[cfg(unix)]
use crate::engine::{EngineError, EngineWarning, PING_TIMEOUT_MS};

#[cfg(all(test, not(unix)))]
#[derive(Clone, Debug, PartialEq)]
pub struct EngineWarning {
    pub seq: u64,
    pub raw_line: String,
    pub message: String,
}

const IP_PACKET_TOO_BIG: i32 = 11009;
const IP_REQ_TIMED_OUT: i32 = 11010;
#[cfg(unix)]
const WATCHDOG_EXTRA_MS: u64 = 1000;
#[cfg(unix)]
const DISCOVERY_ORDER: [&str; 3] = ["/bin/ping", "/sbin/ping", "/usr/bin/ping"];

/// Per-probe MTU engine. Payload size varies on every call.
pub enum MtuProbeEngine {
    #[cfg(windows)]
    WinIcmpDf(IpAddr),
    #[cfg(unix)]
    OsPingDf(OsPingDfEngine),
    /// Scripted outcomes for tests, including `tests/*.rs` integration tests
    /// (which build the lib without `cfg(test)`, so this is not test-gated).
    #[doc(hidden)]
    Mock(VecDeque<ProbeOutcome>),
    /// TCP PLPMTUD fallback (RFC 4821 style). Construction itself never fails
    /// on unsupported platforms; `connect` reports `Unavailable` there.
    TcpProbe(TcpMtuProber),
}

#[cfg(unix)]
pub struct OsPingDfEngine {
    program: PathBuf,
    target: IpAddr,
    warnings: Vec<EngineWarning>,
}

#[cfg(unix)]
impl OsPingDfEngine {
    pub fn new(target: IpAddr) -> Result<Self, EngineError> {
        for candidate in DISCOVERY_ORDER {
            let path = PathBuf::from(candidate);
            if path.is_file() {
                return Ok(Self::with_program(path, target));
            }
        }
        Err(EngineError::Unavailable(format!(
            "no ping binary found in {}",
            DISCOVERY_ORDER.join(", ")
        )))
    }

    pub fn with_program(program: PathBuf, target: IpAddr) -> Self {
        Self {
            program,
            target,
            warnings: Vec::new(),
        }
    }

    pub fn take_warnings(&mut self) -> Vec<EngineWarning> {
        std::mem::take(&mut self.warnings)
    }

    async fn probe(&mut self, payload_size: usize) -> ProbeOutcome {
        let IpAddr::V4(_) = self.target else {
            return ProbeOutcome::Error("IPv6 MTU discovery is not supported yet".to_owned());
        };
        let watchdog = Duration::from_millis(PING_TIMEOUT_MS + WATCHDOG_EXTRA_MS);
        let spawned = Command::new(&self.program)
            .args(ping_df_argv(self.target, payload_size))
            .env("LC_ALL", "C")
            .kill_on_drop(true)
            .output();
        let output = match tokio::time::timeout(watchdog, spawned).await {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => {
                return ProbeOutcome::Error(format!(
                    "failed to spawn {}: {err}",
                    self.program.display()
                ));
            }
            Err(_elapsed) => {
                return ProbeOutcome::Error(format!(
                    "{} exceeded the watchdog ({} ms)",
                    self.program.display(),
                    watchdog.as_millis()
                ));
            }
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}\n{stderr}");
        let (outcome, warning) = classify_osping_output(&combined, output.status.success());
        if let Some(warning) = warning {
            self.warnings.push(warning);
        }
        outcome
    }
}

impl MtuProbeEngine {
    /// Per-probe payload size; IP MTU = payload + 28.
    pub async fn probe(&mut self, payload_size: usize) -> ProbeOutcome {
        match self {
            #[cfg(windows)]
            MtuProbeEngine::WinIcmpDf(target) => match target {
                IpAddr::V4(v4) => {
                    let payload = vec![0x61; payload_size.clamp(1, 65_507)];
                    match WinIcmpPinger::echo_v4_df(
                        *v4,
                        payload,
                        u32::try_from(PING_TIMEOUT_MS).unwrap_or(u32::MAX),
                    )
                    .await
                    {
                        Ok(rtt) => ProbeOutcome::Ok { rtt },
                        Err(err) => match err.raw_os_error() {
                            Some(IP_PACKET_TOO_BIG) => ProbeOutcome::TooBig { hint_mtu: None },
                            Some(IP_REQ_TIMED_OUT) => ProbeOutcome::Timeout,
                            Some(_) | None => ProbeOutcome::Error(err.to_string()),
                        },
                    }
                }
                IpAddr::V6(_) => {
                    ProbeOutcome::Error("IPv6 MTU discovery is not supported yet".to_owned())
                }
            },
            #[cfg(unix)]
            MtuProbeEngine::OsPingDf(engine) => engine.probe(payload_size).await,
            MtuProbeEngine::Mock(script) => script.pop_front().unwrap_or(ProbeOutcome::Timeout),
            MtuProbeEngine::TcpProbe(prober) => prober.probe(payload_size).await,
        }
    }
}

#[cfg(unix)]
fn ping_df_argv(target: IpAddr, payload_size: usize) -> Vec<String> {
    #[cfg(target_os = "macos")]
    let wait = PING_TIMEOUT_MS.to_string();
    #[cfg(not(target_os = "macos"))]
    let wait = PING_TIMEOUT_MS.div_ceil(1000).to_string();

    let mut args = vec![
        "-c".to_owned(),
        "1".to_owned(),
        "-W".to_owned(),
        wait,
        "-s".to_owned(),
        payload_size.to_string(),
    ];
    #[cfg(target_os = "macos")]
    args.push("-D".to_owned());
    #[cfg(target_os = "linux")]
    {
        args.push("-M".to_owned());
        args.push("do".to_owned());
    }
    args.push(target.to_string());
    args
}

#[cfg(any(unix, test))]
fn classify_osping_output(
    stdout: &str,
    exit_success: bool,
) -> (ProbeOutcome, Option<EngineWarning>) {
    if stdout.contains("Frag needed and DF set") {
        return (
            ProbeOutcome::TooBig {
                hint_mtu: parse_mtu_hint(stdout),
            },
            None,
        );
    }
    if stdout.contains("Message too long") {
        return (ProbeOutcome::TooBig { hint_mtu: None }, None);
    }
    for line in stdout.lines() {
        if let Some(token) = extract_time_token(line) {
            match token.parse::<f64>() {
                Ok(ms) => {
                    return (
                        ProbeOutcome::Ok {
                            rtt: Duration::from_secs_f64(ms / 1000.0),
                        },
                        None,
                    );
                }
                Err(_) => return (ProbeOutcome::Timeout, None),
            }
        }
    }
    if exit_success {
        return (
            ProbeOutcome::Timeout,
            Some(EngineWarning {
                seq: 0,
                raw_line: stdout.trim().to_owned(),
                message: "ping exited 0 without a time= token; probe recorded as loss".to_owned(),
            }),
        );
    }
    (ProbeOutcome::Timeout, None)
}

#[cfg(any(unix, test))]
fn parse_mtu_hint(stdout: &str) -> Option<u32> {
    let marker = "mtu";
    let pos = stdout.find(marker)?;
    let rest = &stdout[pos + marker.len()..];
    let digits = rest
        .trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '=')
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    digits.parse::<u32>().ok()
}

#[cfg(any(unix, test))]
fn extract_time_token(line: &str) -> Option<&str> {
    let pos = line.find("time=")?;
    let rest = &line[pos + "time=".len()..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    // Given an OS ping success line with an English time= token,
    // When the MTU classifier interprets the completed process,
    // Then the probe outcome is Ok with the parsed RTT.
    #[test]
    fn classify_osping_output_returns_ok_when_time_token_is_present() {
        let stdout = "64 bytes from 127.0.0.1: icmp_seq=1 ttl=64 time=12.3 ms";

        let (outcome, warning) = classify_osping_output(stdout, true);

        assert_eq!(
            outcome,
            ProbeOutcome::Ok {
                rtt: Duration::from_secs_f64(0.0123)
            }
        );
        assert_eq!(warning, None);
    }

    // Given Linux iputils reports Frag Needed with an MTU hint,
    // When the MTU classifier interprets the completed process,
    // Then the probe outcome is TooBig with the parsed hint.
    #[test]
    fn classify_osping_output_returns_too_big_with_hint_when_linux_reports_frag_needed() {
        let stdout = "Frag needed and DF set (mtu = 1462)";

        let (outcome, warning) = classify_osping_output(stdout, false);

        assert_eq!(
            outcome,
            ProbeOutcome::TooBig {
                hint_mtu: Some(1462)
            }
        );
        assert_eq!(warning, None);
    }

    // Given macOS reports Message too long without an MTU hint,
    // When the MTU classifier interprets the completed process,
    // Then the probe outcome is TooBig without a hint.
    #[test]
    fn classify_osping_output_returns_too_big_without_hint_when_macos_reports_message_too_long() {
        let stdout = "ping: sendto: Message too long";

        let (outcome, warning) = classify_osping_output(stdout, false);

        assert_eq!(outcome, ProbeOutcome::TooBig { hint_mtu: None });
        assert_eq!(warning, None);
    }

    // Given ping exits successfully without a time= token,
    // When the MTU classifier interprets the completed process,
    // Then the probe is Timeout and a warning is retained.
    #[test]
    fn classify_osping_output_warns_when_exit_zero_has_no_time_token() {
        let stdout = "garbled success output";

        let (outcome, warning) = classify_osping_output(stdout, true);

        assert_eq!(outcome, ProbeOutcome::Timeout);
        assert!(warning.is_some());
    }

    // Given ping exits non-zero without useful MTU or RTT tokens,
    // When the MTU classifier interprets the completed process,
    // Then the probe is a plain Timeout without warning.
    #[test]
    fn classify_osping_output_times_out_without_warning_when_nonzero_has_no_tokens() {
        let stdout = "100% packet loss";

        let (outcome, warning) = classify_osping_output(stdout, false);

        assert_eq!(outcome, ProbeOutcome::Timeout);
        assert_eq!(warning, None);
    }

    // Given Frag Needed lines vary whitespace around the mtu value,
    // When the MTU classifier interprets each fixture,
    // Then the same numeric hint is parsed.
    #[test]
    fn classify_osping_output_parses_mtu_hint_whitespace_variants() {
        for stdout in [
            "Frag needed and DF set (mtu = 1500)",
            "Frag needed and DF set (mtu=1500)",
            "Frag needed and DF set (mtu =    1500)",
        ] {
            let (outcome, warning) = classify_osping_output(stdout, false);
            assert_eq!(
                outcome,
                ProbeOutcome::TooBig {
                    hint_mtu: Some(1500)
                }
            );
            assert_eq!(warning, None);
        }
    }

    // Given a scripted MTU mock engine,
    // When probes are requested in order,
    // Then outcomes replay in order and exhaustion yields Timeout.
    #[tokio::test]
    async fn mock_engine_replays_script_then_times_out_when_exhausted() {
        let script = VecDeque::from([
            ProbeOutcome::Ok {
                rtt: Duration::from_millis(1),
            },
            ProbeOutcome::TooBig { hint_mtu: None },
        ]);
        let mut engine = MtuProbeEngine::Mock(script);

        assert_eq!(
            engine.probe(1472).await,
            ProbeOutcome::Ok {
                rtt: Duration::from_millis(1)
            }
        );
        assert_eq!(
            engine.probe(8972).await,
            ProbeOutcome::TooBig { hint_mtu: None }
        );
        assert_eq!(engine.probe(8972).await, ProbeOutcome::Timeout);
    }
}
