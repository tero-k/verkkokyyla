//! POSIX OS-ping fallback adapter: spawns the system `ping` binary via
//! `tokio::process::Command` with an absolute resolved path, a fixed argv
//! array, and child env `LC_ALL=C` forcing English output. The parser is
//! pinned to the English `time=<float> ms` token — localized output is NEVER
//! parsed heuristically; a parse failure records the probe as loss and
//! retains the raw line in an [`EngineWarning`].

use std::net::IpAddr;
use std::path::PathBuf;
use std::time::Duration;

use tokio::process::Command;

use super::{EngineError, ProbeResult, PING_TIMEOUT_MS};

/// Absolute-path discovery order for the ping binary.
const DISCOVERY_ORDER: [&str; 3] = ["/bin/ping", "/sbin/ping", "/usr/bin/ping"];

/// Backstop in case the spawned ping ignores its own timeout flag.
const WATCHDOG_EXTRA_MS: u64 = 1000;

/// A retained warning: the OS-ping output for probe `seq` could not be
/// parsed (garbled or localized output). The probe itself is recorded as
/// loss; the session layer (todo 7) surfaces these as warning events.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineWarning {
    pub seq: u64,
    pub raw_line: String,
    pub message: String,
}

/// POSIX fallback engine spawning the OS ping binary per probe.
pub struct OsPinger {
    program: PathBuf,
    target: IpAddr,
    payload_size: usize,
    dont_fragment: bool,
    warnings: Vec<EngineWarning>,
}

impl OsPinger {
    /// Discover the ping binary in [`DISCOVERY_ORDER`] and bind the target.
    pub fn new(target: IpAddr, payload_size: usize, dont_fragment: bool) -> Result<Self, EngineError> {
        for candidate in DISCOVERY_ORDER {
            let path = PathBuf::from(candidate);
            if path.is_file() {
                return Ok(Self::with_program(path, target, payload_size, dont_fragment));
            }
        }
        Err(EngineError::Unavailable(format!(
            "no ping binary found in {}",
            DISCOVERY_ORDER.join(", ")
        )))
    }

    /// Bind an explicit binary path — the test seam for fake ping fixtures.
    pub fn with_program(program: PathBuf, target: IpAddr, payload_size: usize, dont_fragment: bool) -> Self {
        Self {
            program,
            target,
            payload_size: payload_size.clamp(1, 65_507),
            dont_fragment,
            warnings: Vec::new(),
        }
    }

    /// Drain the retained parse-failure warnings.
    pub fn take_warnings(&mut self) -> Vec<EngineWarning> {
        std::mem::take(&mut self.warnings)
    }

    /// Run `ping -c 1 -W <n> <target>` and parse the English output.
    pub async fn probe(&mut self, seq: u64) -> ProbeResult {
        let watchdog = Duration::from_millis(PING_TIMEOUT_MS + WATCHDOG_EXTRA_MS);
        let spawned = Command::new(&self.program)
            .args(ping_argv(self.target, self.payload_size, self.dont_fragment))
            .env("LC_ALL", "C")
            .kill_on_drop(true)
            .output();
        let output = match tokio::time::timeout(watchdog, spawned).await {
            Ok(Ok(output)) => output,
            Ok(Err(err)) => {
                return ProbeResult::Error(format!(
                    "failed to spawn {}: {err}",
                    self.program.display()
                ));
            }
            Err(_elapsed) => {
                return ProbeResult::Error(format!(
                    "{} exceeded the watchdog ({} ms)",
                    self.program.display(),
                    watchdog.as_millis()
                ));
            }
        };
        self.interpret(seq, &output)
    }

    /// Map one finished ping invocation to a [`ProbeResult`].
    fn interpret(&mut self, seq: u64, output: &std::process::Output) -> ProbeResult {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Finding a `time=` token always returns from inside the loop, so
        // completing the loop means the output carried no English token.
        for line in stdout.lines() {
            if let Some(token) = extract_time_token(line) {
                match token.parse::<f64>() {
                    Ok(ms) => return ProbeResult::Rtt(Duration::from_secs_f64(ms / 1000.0)),
                    Err(_) => {
                        self.warnings.push(EngineWarning {
                            seq,
                            raw_line: line.to_owned(),
                            message: "unparseable time= value; probe recorded as loss".to_owned(),
                        });
                        return ProbeResult::Timeout;
                    }
                }
            }
        }
        if output.status.success() {
            // Exit 0 but no English `time=` token: garbled/localized output.
            // Never guess — record loss and keep the raw output.
            self.warnings.push(EngineWarning {
                seq,
                raw_line: stdout.trim().to_owned(),
                message: "ping exited 0 without a time= token; probe recorded as loss".to_owned(),
            });
        }
        ProbeResult::Timeout
    }
}

/// Build the OS `ping` argv. Linux/macOS differ on the wait unit,
/// and `-s <bytes>` sets the ICMP payload size. The DF flag is requested
/// with `-M do` on Linux and `-D` on macOS; on other POSIX targets it is
/// omitted when the binary is not known to support it.
fn ping_argv(target: IpAddr, payload_size: usize, dont_fragment: bool) -> Vec<String> {
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

    if dont_fragment {
        #[cfg(target_os = "macos")]
        args.push("-D".to_owned());
        #[cfg(target_os = "linux")]
        args.push("-M".to_owned());
        #[cfg(target_os = "linux")]
        args.push("do".to_owned());
    }

    args.push(target.to_string());
    args
}

/// Extract the token after `time=` (digits and dots) from one output line.
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
    use std::net::Ipv4Addr;

    fn target() -> IpAddr {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    }

    /// Write an executable fake `ping` shell script emitting `body`.
    fn fake_ping(dir: &std::path::Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join("fake-ping");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write fake ping");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake ping");
        path
    }

    // Given the pinned English fixture `time=12.3 ms`,
    // When the adapter probes through the fake binary,
    // Then the RTT is parsed as 12.3 ms.
    #[tokio::test]
    async fn osping_parses_pinned_english_fixture() {
        let dir = std::env::temp_dir().join(format!("osping-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tempdir");
        let program = fake_ping(
            &dir,
            "echo '64 bytes from 127.0.0.1: icmp_seq=1 ttl=64 time=12.3 ms'",
        );
        let mut pinger = OsPinger::with_program(program, target(), 32, false);

        match pinger.probe(1).await {
            ProbeResult::Rtt(rtt) => {
                assert!((rtt.as_secs_f64() * 1000.0 - 12.3).abs() < 1e-6)
            }
            other => panic!("expected Rtt, got {other:?}"),
        }
        assert!(pinger.take_warnings().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    // Given garbled output containing an unparseable `time=` value,
    // When probed,
    // Then the probe is recorded as loss (Timeout) and the raw line is
    // retained in an EngineWarning — never parsed heuristically.
    #[tokio::test]
    async fn osping_unparseable_time_value_records_loss_and_warning() {
        let dir = std::env::temp_dir().join(format!("osping-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tempdir");
        let program = fake_ping(&dir, "echo '64 bytes from x: time=abc ms'");
        let mut pinger = OsPinger::with_program(program, target(), 32, false);

        assert_eq!(pinger.probe(1).await, ProbeResult::Timeout);
        let warnings = pinger.take_warnings();
        assert_eq!(warnings.len(), 1);
        assert_eq!(warnings[0].seq, 1);
        assert!(warnings[0].raw_line.contains("time=abc"));
        std::fs::remove_dir_all(&dir).ok();
    }

    // Given localized output (German `Zeit=...`) with exit 0 and no `time=`
    // token,
    // When probed,
    // Then the probe is loss + warning; the localized value is NOT parsed.
    #[tokio::test]
    async fn osping_localized_output_is_never_parsed() {
        let dir = std::env::temp_dir().join(format!("osping-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tempdir");
        let program = fake_ping(&dir, "echo '64 Bytes von x: Zeit=12,3 ms'");
        let mut pinger = OsPinger::with_program(program, target(), 32, false);

        assert_eq!(pinger.probe(1).await, ProbeResult::Timeout);
        assert_eq!(pinger.take_warnings().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    // Given a fake ping that reports total loss (exit 1, no time= token),
    // When probed,
    // Then the outcome is a plain Timeout with no warning.
    #[tokio::test]
    async fn osping_loss_fixture_yields_timeout_without_warning() {
        let dir = std::env::temp_dir().join(format!("osping-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tempdir");
        let program = fake_ping(
            &dir,
            "echo 'PING 192.0.2.1: 56 data bytes'; echo 'ping: sendto: No route to host' >&2; exit 1",
        );
        let mut pinger = OsPinger::with_program(program, target(), 32, false);

        assert_eq!(pinger.probe(1).await, ProbeResult::Timeout);
        assert!(pinger.take_warnings().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    // Given a nonexistent binary path,
    // When probed,
    // Then the probe is an Error (no panic).
    #[tokio::test]
    async fn osping_missing_binary_yields_error() {
        let mut pinger =
            OsPinger::with_program(PathBuf::from("/nonexistent/definitely-no-ping"), target());
        assert!(matches!(pinger.probe(1).await, ProbeResult::Error(_)));
    }

    // Given output lines,
    // When extracting the time= token,
    // Then only the digits/dots immediately after `time=` are returned.
    #[test]
    fn osping_extract_time_token_boundaries() {
        assert_eq!(
            extract_time_token("64 bytes from a: icmp_seq=1 ttl=64 time=0.042 ms"),
            Some("0.042")
        );
        assert_eq!(extract_time_token("time=123 ms"), Some("123"));
        assert_eq!(extract_time_token("no token here"), None);
        assert_eq!(extract_time_token("Zeit=12,3 ms"), None);
        assert_eq!(extract_time_token("time="), Some(""));
    }
}
