use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum DnsError {
    #[error("DNS endpoint is invalid: {0}")]
    InvalidEndpoint(String),
    #[error("DNS input is invalid: {0}")]
    InvalidInput(String),
    #[error("DNS query timed out")]
    Timeout,
    #[error("DNS server returned SERVFAIL")]
    Servfail,
    #[error("DNS server refused the query")]
    Refused,
    #[error("confirmation is required for this DNS operation")]
    ConfirmationRequired,
    #[error("load testing is not allowed for this DNS target")]
    LoadTargetNotAllowed,
    #[error("DNS benchmark configuration exceeds the allowed cap")]
    OverCap,
    #[error("no authoritative DNS reference could be found")]
    NoReference,
    #[error("DNS zone transfer ended before completion")]
    TruncatedTransfer,
    #[error("DNS transport is unsupported: {0}")]
    UnsupportedTransport(String),
    #[error("DNS transport failed: {0}")]
    Transport(String),
    #[error("DNS resolution failed: {0}")]
    Resolution(String),
    #[error("DNSSEC validation failed: {0}")]
    Dnssec(String),
    #[error("DNS I/O failed: {0}")]
    Io(String),
    #[error("DNS operation was cancelled")]
    Cancelled,
}

impl DnsError {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::InvalidEndpoint(_) => "invalid-endpoint",
            Self::InvalidInput(_) => "invalid-input",
            Self::Timeout => "timeout",
            Self::Servfail => "servfail",
            Self::Refused => "refused",
            Self::ConfirmationRequired => "confirmation-required",
            Self::LoadTargetNotAllowed => "load-target-not-allowed",
            Self::OverCap => "over-cap",
            Self::NoReference => "no-reference",
            Self::TruncatedTransfer => "truncated-transfer",
            Self::UnsupportedTransport(_) => "unsupported-transport",
            Self::Transport(_) => "transport",
            Self::Resolution(_) => "resolution",
            Self::Dnssec(_) => "dnssec",
            Self::Io(_) => "io",
            Self::Cancelled => "cancelled",
        }
    }
}

impl Serialize for DnsError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("DnsError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}
