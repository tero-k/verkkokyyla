use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::dns::client::{DnsProtocol, ResolverEndpointDto};
use crate::dns::error::DnsError;
use crate::dns::query::{query_once, QueryOpts, QueryResultDto, RecordTypeSpec};

/// Result of one EDNS buffer-size probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdnsSweepDto {
    pub size: u16,
    pub rcode: String,
    pub truncated: bool,
    pub response_bytes: usize,
}

/// Result of a truncation → TCP fallback probe.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TcpFallbackDto {
    pub udp_truncated: bool,
    pub tcp_success: bool,
    pub tcp_latency_ms: Option<u64>,
}

/// Result of the same query over several transports.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportResultDto {
    pub protocol: String,
    pub rcode: String,
    pub latency_ms: u64,
    pub answer_hash: String,
}

/// EDNS capability report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdnsSupportDto {
    pub opt_present: bool,
    pub edns_version: u8,
    pub responder: String,
    pub requested_version: u8,
    pub header_rcode: u16,
    pub extended_rcode: u8,
    pub full_rcode: u16,
    pub full_rcode_name: String,
}

/// Query `name` type A over UDP with a sweep of EDNS payload sizes.
pub async fn edns_buffer_sweep(
    endpoint: &ResolverEndpointDto,
    name: &str,
) -> Result<Vec<EdnsSweepDto>, DnsError> {
    let mut out = Vec::with_capacity(4);
    for size in [512u16, 1232, 1400, 4096] {
        let opts = QueryOpts {
            edns_size: Some(size),
            ..QueryOpts::default()
        };
        let result = query_once(endpoint, name, RecordTypeSpec::A, opts).await?;
        out.push(EdnsSweepDto {
            size,
            rcode: result.rcode,
            truncated: result.truncated,
            response_bytes: result.response_bytes,
        });
    }
    Ok(out)
}

/// Probe whether a UDP truncation is followed by a successful TCP retry.
pub async fn tcp_fallback_check(
    udp_endpoint: &ResolverEndpointDto,
    tcp_endpoint: &ResolverEndpointDto,
    name: &str,
) -> Result<TcpFallbackDto, DnsError> {
    let udp_opts = QueryOpts {
        edns_size: Some(512),
        ..QueryOpts::default()
    };
    let udp_result = query_once(udp_endpoint, name, RecordTypeSpec::A, udp_opts).await?;
    let udp_truncated = udp_result.truncated;

    let mut tcp_success = false;
    let mut tcp_latency_ms = None;
    if udp_truncated {
        match query_once(tcp_endpoint, name, RecordTypeSpec::A, QueryOpts::default()).await {
            Ok(r) => {
                tcp_success = true;
                tcp_latency_ms = Some(r.latency_ms);
            }
            Err(_) => {}
        }
    }

    Ok(TcpFallbackDto {
        udp_truncated,
        tcp_success,
        tcp_latency_ms,
    })
}

/// Force the query over TCP. The supplied endpoint must target the TCP listener.
pub async fn forced_tcp_query(
    tcp_endpoint: &ResolverEndpointDto,
    name: &str,
    rtype: RecordTypeSpec,
) -> Result<QueryResultDto, DnsError> {
    query_once(tcp_endpoint, name, rtype, QueryOpts::default()).await
}

/// Run the same query over the supplied transport endpoints. For DoT endpoints
/// pass `tls_pinned_cert` so the client can trust the test fixture certificate.
pub async fn transport_equivalence(
    endpoints: &[ResolverEndpointDto],
    name: &str,
    rtype: RecordTypeSpec,
    tls_pinned_cert: Option<&[u8]>,
) -> Result<Vec<TransportResultDto>, DnsError> {
    let mut out = Vec::with_capacity(endpoints.len());
    for endpoint in endpoints {
        let opts = if endpoint.protocol == DnsProtocol::Tls {
            QueryOpts {
                pinned_root_cert: tls_pinned_cert.map(|c| c.to_vec()),
                ..QueryOpts::default()
            }
        } else {
            QueryOpts::default()
        };

        match query_once(endpoint, name, rtype, opts).await {
            Ok(result) => {
                let hash = answer_hash(&result);
                out.push(TransportResultDto {
                    protocol: endpoint.protocol.to_string(),
                    rcode: result.rcode,
                    latency_ms: result.latency_ms,
                    answer_hash: hash,
                })
            }
            Err(err) => out.push(TransportResultDto {
                protocol: endpoint.protocol.to_string(),
                rcode: format!("error: {}", err.kind()),
                latency_ms: 0,
                answer_hash: String::new(),
            }),
        }
    }
    Ok(out)
}

/// Check EDNS0 support: OPT presence, version 0 handling, and a version-1 BADVERS probe.
pub async fn edns_support_check(
    endpoint: &ResolverEndpointDto,
    name: &str,
) -> Result<EdnsSupportDto, DnsError> {
    let version0_opts = QueryOpts {
        edns_size: Some(1232),
        edns_version: 0,
        ..QueryOpts::default()
    };
    let version0 = query_once(endpoint, name, RecordTypeSpec::A, version0_opts).await?;
    let opt_present = version0.edns_present;

    let requested_version: u8 = 1;
    let badvers_opts = QueryOpts {
        edns_size: Some(1232),
        edns_version: requested_version,
        ..QueryOpts::default()
    };
    let badvers = query_once(endpoint, name, RecordTypeSpec::A, badvers_opts).await?;

    Ok(EdnsSupportDto {
        opt_present,
        edns_version: 0,
        responder: endpoint.address.clone(),
        requested_version,
        header_rcode: badvers.header_rcode,
        extended_rcode: badvers.extended_rcode,
        full_rcode: badvers.full_rcode,
        full_rcode_name: badvers.rcode.clone(),
    })
}

fn answer_hash(result: &QueryResultDto) -> String {
    let mut data: Vec<String> = result.answers.iter().map(|a| a.data.clone()).collect();
    data.sort();
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
