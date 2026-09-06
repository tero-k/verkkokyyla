//! Typed errors for the MikroTik REST client and defensive parser.
//!
//! Error classification follows the plan's rules: a 401 is always an
//! authentication failure (never a version signal), a REST 404 on
//! `/rest/system/resource` means the RouterOS version predates REST support,
//! and no-such-command bodies (400/406) are detectable so callers can treat
//! unsupported endpoints as "not applicable" rather than fatal.

use thiserror::Error;

/// Errors produced by the MikroTik REST client and its defensive parser.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum MikrotikError {
    /// TCP connect failed, the connection was refused, or the connection
    /// dropped before the response completed.
    #[error("connection failed: {0}")]
    Connect(String),

    /// Connect or request timeout (RouterOS REST server ceiling is 60s).
    #[error("timeout: {0}")]
    Timeout(String),

    /// HTTP 401 — always an authentication failure, never a version signal.
    #[error("authentication failed (401): check username and password")]
    Unauthorized,

    /// HTTP 403 — authenticated but the REST user lacks permission.
    #[error("forbidden (403): REST user lacks permission")]
    Forbidden,

    /// TLS handshake or certificate validation failure.
    #[error("TLS error: {0}")]
    Tls(String),

    /// Any other non-success HTTP status from the router.
    #[error("router API error {status}: {message}")]
    Api { status: u16, message: String },

    /// Response body could not be decoded as the expected JSON shape
    /// (includes HTML error pages served in place of REST payloads).
    #[error("parse error: {0}")]
    Parse(String),

    /// `delete_file` target name was not present in `/rest/file`.
    #[error("file not found on router: {0}")]
    FileNotFound(String),

    /// REST 404 on `/rest/system/resource` — RouterOS too old for REST.
    /// The message names the required service, split by transport scheme.
    #[error("RouterOS version too old for REST: {0}")]
    UnsupportedVersion(String),
}

impl MikrotikError {
    /// RouterOS answers unsupported endpoints (e.g. `/system/health` on some
    /// boards, `/system/routerboard` on CHR/x86) with 400/406 and a
    /// "no such command" body — sometimes with a "(remove)" suffix, in mixed
    /// case, or as a JSON body carrying only a `message` field. Callers use
    /// this to map such responses to a "not supported" UI state.
    pub fn is_no_such_command(&self) -> bool {
        match self {
            MikrotikError::Api { status, message } => {
                matches!(*status, 400 | 406) && message.to_lowercase().contains("no such command")
            }
            _ => false,
        }
    }

    /// True when the HTTP status/body pair looks like a RouterOS
    /// no-such-command rejection (checked before an error is constructed).
    pub fn looks_like_no_such_command(status: u16, body: &str) -> bool {
        matches!(status, 400 | 406) && body.to_lowercase().contains("no such command")
    }
}

/// Classify a reqwest transport error into a typed variant (pattern:
/// `bench.rs::classify_reqwest_error` — walk the source chain for TLS hints).
pub(crate) fn classify_transport(err: &reqwest::Error) -> MikrotikError {
    use std::error::Error as _;

    let message = err.to_string();

    if err.is_timeout() {
        return MikrotikError::Timeout(message);
    }

    let mut source = err.source();
    let mut tls_hint = message.to_lowercase().contains("certificate")
        || message.to_lowercase().contains("tls");
    while let Some(s) = source {
        let text = s.to_string().to_lowercase();
        if text.contains("certificate") || text.contains("tls") {
            tls_hint = true;
        }
        source = s.source();
    }

    if tls_hint {
        MikrotikError::Tls(message)
    } else {
        MikrotikError::Connect(message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mikrotik_error_no_such_command_plain_body() {
        let err = MikrotikError::Api {
            status: 400,
            message: "no such command or directory (remove)".to_owned(),
        };
        assert!(err.is_no_such_command());
    }

    #[test]
    fn mikrotik_error_no_such_command_mixed_case() {
        let err = MikrotikError::Api {
            status: 406,
            message: "No Such Command Or Directory (remove)".to_owned(),
        };
        assert!(err.is_no_such_command());
    }

    #[test]
    fn mikrotik_error_no_such_command_message_only_json() {
        let body = r#"{"message":"no such command or directory (remove)"}"#;
        assert!(MikrotikError::looks_like_no_such_command(400, body));
    }

    #[test]
    fn mikrotik_error_no_such_command_false_for_other_statuses() {
        let err = MikrotikError::Api {
            status: 500,
            message: "no such command".to_owned(),
        };
        assert!(!err.is_no_such_command());
        assert!(!MikrotikError::looks_like_no_such_command(
            404,
            "no such command"
        ));
    }

    #[test]
    fn mikrotik_error_no_such_command_false_for_other_messages() {
        let err = MikrotikError::Api {
            status: 400,
            message: "bad request".to_owned(),
        };
        assert!(!err.is_no_such_command());
    }

    #[test]
    fn mikrotik_error_display_never_empty() {
        let variants = vec![
            MikrotikError::Connect("refused".to_owned()).to_string(),
            MikrotikError::Timeout("deadline".to_owned()).to_string(),
            MikrotikError::Unauthorized.to_string(),
            MikrotikError::Forbidden.to_string(),
            MikrotikError::Tls("bad cert".to_owned()).to_string(),
            MikrotikError::Api {
                status: 500,
                message: "boom".to_owned(),
            }
            .to_string(),
            MikrotikError::Parse("junk".to_owned()).to_string(),
            MikrotikError::FileNotFound("x.backup".to_owned()).to_string(),
            MikrotikError::UnsupportedVersion("need v7".to_owned()).to_string(),
        ];
        for text in variants {
            assert!(!text.is_empty());
        }
    }
}
