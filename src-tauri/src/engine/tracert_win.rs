use std::net::IpAddr;
use std::path::PathBuf;

use super::trace_os::{RawHopStream, TraceEngineError, TraceLimits, TraceOs};
use super::trace_parse::parse_tracert_line;

pub struct TracertWin {
    inner: TraceOs,
}

impl TracertWin {
    pub fn new(target: IpAddr) -> Result<Self, TraceEngineError> {
        let search_dirs = super::trace_os::search_path_dirs();
        let program = TraceOs::resolve_program(
            "tracert.exe",
            &search_dirs,
            &super::trace_os::absolute_fallbacks(&["C:\\Windows\\System32\\tracert.exe"]),
        )?;
        Ok(Self {
            inner: TraceOs::with_program(
                program,
                vec![
                    "/d".to_owned(),
                    "/h".to_owned(),
                    "30".to_owned(),
                    "/w".to_owned(),
                    "1000".to_owned(),
                ],
                target,
                parse_tracert_line,
            ),
        })
    }

    pub fn with_program(program: PathBuf, prefix_args: Vec<String>, target: IpAddr) -> Self {
        Self {
            inner: TraceOs::with_program(program, prefix_args, target, parse_tracert_line),
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
