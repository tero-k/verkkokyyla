use std::net::IpAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use tokio::io::{AsyncBufReadExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};
use tokio::time::timeout;

use super::trace_parse::RawHop;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceEngineError {
    Unavailable(String),
    Spawn(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceLimits {
    pub inactivity: Duration,
    pub ceiling: Duration,
}

impl TraceLimits {
    pub const fn new(inactivity: Duration, ceiling: Duration) -> Self {
        Self {
            inactivity,
            ceiling,
        }
    }
}

pub struct TraceOs {
    program: PathBuf,
    prefix_args: Vec<String>,
    target: IpAddr,
    parser: fn(&str) -> Option<RawHop>,
    envs: Vec<(String, String)>,
    limits: TraceLimits,
}

pub struct RawHopStream {
    child: Child,
    lines: Lines<BufReader<ChildStdout>>,
    parser: fn(&str) -> Option<RawHop>,
    limits: TraceLimits,
    started_at: Instant,
    last_line_at: Instant,
}

impl TraceOs {
    pub fn with_program(
        program: PathBuf,
        prefix_args: Vec<String>,
        target: IpAddr,
        parser: fn(&str) -> Option<RawHop>,
    ) -> Self {
        Self {
            program,
            prefix_args,
            target,
            parser,
            envs: Vec::new(),
            limits: TraceLimits::new(Duration::from_secs(5), Duration::from_secs(120)),
        }
    }

    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.envs.push((key.into(), value.into()));
        self
    }

    pub fn with_limits(mut self, limits: TraceLimits) -> Self {
        self.limits = limits;
        self
    }

    pub fn resolve_program(
        program_name: &str,
        search_dirs: &[PathBuf],
        absolute_fallbacks: &[PathBuf],
    ) -> Result<PathBuf, TraceEngineError> {
        for directory in search_dirs {
            let candidate = directory.join(program_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }

        for fallback in absolute_fallbacks {
            if fallback.is_file() {
                return Ok(fallback.clone());
            }
        }

        Err(TraceEngineError::Unavailable(format!(
            "no traceroute binary found: {program_name}"
        )))
    }

    pub fn start(self) -> Result<RawHopStream, TraceEngineError> {
        let mut command = Command::new(&self.program);
        command.args(&self.prefix_args);
        command.arg(self.target.to_string());
        for (key, value) in self.envs {
            command.env(key, value);
        }
        command.stdout(std::process::Stdio::piped());
        command.kill_on_drop(true);

        #[cfg(windows)]
        {
            // CREATE_NO_WINDOW — prevents a visible command-prompt window when
            // the Tauri app spawns tracert.exe.
            command.creation_flags(0x08000000);
        }

        let mut child = command.spawn().map_err(|err| {
            TraceEngineError::Spawn(format!("failed to spawn {}: {err}", self.program.display()))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            TraceEngineError::Spawn(format!("{} did not provide stdout", self.program.display()))
        })?;

        Ok(RawHopStream {
            child,
            lines: BufReader::new(stdout).lines(),
            parser: self.parser,
            limits: self.limits,
            started_at: Instant::now(),
            last_line_at: Instant::now(),
        })
    }
}

impl RawHopStream {
    pub async fn next(&mut self) -> Result<Option<RawHop>, TraceEngineError> {
        loop {
            let remaining_ceiling = self
                .limits
                .ceiling
                .checked_sub(self.started_at.elapsed())
                .unwrap_or(Duration::ZERO);
            if remaining_ceiling.is_zero() {
                self.child.start_kill().ok();
                return Err(TraceEngineError::Spawn(
                    "trace exceeded the absolute ceiling".to_owned(),
                ));
            }

            let remaining_inactivity = self
                .limits
                .inactivity
                .checked_sub(self.last_line_at.elapsed())
                .unwrap_or(Duration::ZERO);
            let wait_for = remaining_ceiling.min(remaining_inactivity);
            if wait_for.is_zero() {
                self.child.start_kill().ok();
                return Err(TraceEngineError::Spawn(
                    "trace exceeded the watchdog".to_owned(),
                ));
            }

            let line = match timeout(wait_for, self.lines.next_line()).await {
                Ok(Ok(Some(line))) => line,
                Ok(Ok(None)) => return Ok(None),
                Ok(Err(err)) => {
                    self.child.start_kill().ok();
                    return Err(TraceEngineError::Spawn(format!(
                        "failed reading traceroute stdout: {err}"
                    )));
                }
                Err(_) => {
                    self.child.start_kill().ok();
                    if wait_for == remaining_ceiling {
                        return Err(TraceEngineError::Spawn(
                            "trace exceeded the absolute ceiling".to_owned(),
                        ));
                    }
                    return Err(TraceEngineError::Spawn(
                        "trace exceeded the watchdog".to_owned(),
                    ));
                }
            };

            self.last_line_at = Instant::now();
            let line = String::from_utf8_lossy(line.as_bytes()).into_owned();
            if let Some(raw_hop) = (self.parser)(&line) {
                return Ok(Some(raw_hop));
            }
        }
    }

    pub fn child_running(&mut self) -> Result<bool, TraceEngineError> {
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|err| {
                TraceEngineError::Spawn(format!("failed to query traceroute child: {err}"))
            })
    }
}

impl Drop for RawHopStream {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

pub fn search_path_dirs() -> Vec<PathBuf> {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default()
}

pub fn absolute_fallbacks(paths: &[&str]) -> Vec<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}
