use serde::{Deserialize, Serialize};

/// Lightweight connection-state summary for a resolver endpoint.
///
/// Full connection tracking (established socket counts, queue depths, TLS
/// handshakes) is platform-specific and is intentionally left as a stub.  The
/// field names are stable so the frontend can render a placeholder row.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStateDto {
    pub endpoint_name: String,
    pub active_sockets: u64,
    pub pending_queries: u64,
    pub avg_conn_lifetime_ms: u64,
    pub notes: Vec<String>,
}

/// Return a placeholder connection-state report for `endpoint_name`.
pub fn connection_state(endpoint_name: &str) -> ConnectionStateDto {
    ConnectionStateDto {
        endpoint_name: endpoint_name.to_owned(),
        notes: vec!["Connection-level metrics are not implemented on this platform.".to_string()],
        ..ConnectionStateDto::default()
    }
}
