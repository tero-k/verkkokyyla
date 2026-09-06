use serde::{Deserialize, Serialize};

/// Result of a single DNS probe, distinguishing a successful response from a
/// transport/lookup failure so callers do not confuse "empty answer" with
/// "could not ask".
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum ProbeResult<T> {
    #[serde(rename = "ok")]
    Ok(ProbeSuccess<T>),
    #[serde(rename = "err")]
    Err(ProbeFailure),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSuccess<T> {
    pub data: T,
    pub server: String,
    pub rcode: String,
    pub aa_flag: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeFailure {
    pub server: String,
    pub error: String,
    pub rcode: Option<String>,
}

impl<T> ProbeResult<T> {
    pub fn ok(&self) -> Option<&ProbeSuccess<T>> {
        match self {
            ProbeResult::Ok(s) => Some(s),
            ProbeResult::Err(_) => None,
        }
    }

    pub fn is_ok(&self) -> bool {
        matches!(self, ProbeResult::Ok(_))
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> ProbeResult<U> {
        match self {
            ProbeResult::Ok(s) => ProbeResult::Ok(ProbeSuccess {
                data: f(s.data),
                server: s.server,
                rcode: s.rcode,
                aa_flag: s.aa_flag,
            }),
            ProbeResult::Err(e) => ProbeResult::Err(e),
        }
    }
}
