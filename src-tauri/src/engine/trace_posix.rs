use std::net::IpAddr;
use std::path::PathBuf;

use super::trace_os::{
    absolute_fallbacks, search_path_dirs, RawHopStream, TraceEngineError, TraceLimits, TraceOs,
};
use super::trace_parse::parse_traceroute_line;

pub struct TracePosix {
    inner: TraceOs,
}

impl TracePosix {
    pub fn new(target: IpAddr) -> Result<Self, TraceEngineError> {
        let program = TraceOs::resolve_program(
            "traceroute",
            &search_path_dirs(),
            &absolute_fallbacks(&[
                "/usr/sbin/traceroute",
                "/usr/bin/traceroute",
                "/sbin/traceroute",
                "/bin/traceroute",
            ]),
        )?;
        Ok(Self {
            inner: TraceOs::with_program(
                program,
                vec![
                    "-n".to_owned(),
                    "-m".to_owned(),
                    "30".to_owned(),
                    "-w".to_owned(),
                    "1".to_owned(),
                    "-q".to_owned(),
                    "1".to_owned(),
                ],
                target,
                parse_traceroute_line,
            )
            .with_env("LC_ALL", "C"),
        })
    }

    pub fn with_program(program: PathBuf, prefix_args: Vec<String>, target: IpAddr) -> Self {
        Self {
            inner: TraceOs::with_program(program, prefix_args, target, parse_traceroute_line)
                .with_env("LC_ALL", "C"),
        }
    }

    pub fn with_limits(mut self, limits: TraceLimits) -> Self {
        self.inner = self.inner.with_limits(limits);
        self
    }

    pub fn start(self) -> Result<RawHopStream, TraceEngineError> {
        self.inner.start()
    }
}
