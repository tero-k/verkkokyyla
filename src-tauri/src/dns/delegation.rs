use std::collections::BTreeSet;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::dns::client::{DnsProtocol, ResolverEndpointDto};
use crate::dns::error::DnsError;
use crate::dns::probe::{ProbeFailure, ProbeResult, ProbeSuccess};
use crate::dns::query::{query_once, QueryOpts, QueryResultDto, RecordTypeSpec};

/// Result of a delegation/glue diagnostic run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelegationReportDto {
    pub domain: String,
    pub parent_ns: Vec<String>,
    pub parent_ns_error: Option<String>,
    pub child_ns: Vec<String>,
    pub child_ns_error: Option<String>,
    pub ns_consistent: Option<bool>,
    pub glue_records: Vec<String>,
    pub authoritative_servers: Vec<AuthoritativeServerDto>,
    pub authoritative: bool,
    pub ns_serials: Vec<NsSerialDto>,
    pub serial_consistent: Option<bool>,
    pub notes: Vec<String>,
}

/// Evidence collected from one delegated nameserver.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthoritativeServerDto {
    pub name: String,
    pub addresses: Vec<String>,
    pub ns_query: ProbeResult<Vec<String>>,
    pub soa_query: ProbeResult<u32>,
}

/// A per-nameserver SOA serial observation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NsSerialDto {
    pub name: String,
    pub address: String,
    pub serial: Option<u32>,
}

/// Diagnose delegation consistency, glue, and authoritative nameserver state.
///
/// The parent delegation NS set is obtained from the configured resolver. Each
/// delegated nameserver is then queried directly for the zone's NS and SOA
/// records (RD=0). If direct queries yield no usable authoritative response,
/// the configured resolver is queried with RD=0 as a fallback to retrieve the
/// child NS set. Missing evidence is reported explicitly so it cannot be
/// mistaken for a real delegation mismatch.
pub async fn delegation_report(
    endpoint: &ResolverEndpointDto,
    domain: &str,
) -> Result<DelegationReportDto, DnsError> {
    let domain = normalize_domain(domain);

    let parent_probe = probe_parent_ns(endpoint, &domain).await;
    let parent_ns = parent_probe
        .ok()
        .map(|s| s.data.clone())
        .unwrap_or_default();
    let parent_ns_error = match &parent_probe {
        ProbeResult::Ok(_) => None,
        ProbeResult::Err(e) => Some(e.error.clone()),
    };

    // Gather glue from the referral/authoritative response when available.
    let glue_records = gather_glue(endpoint, &domain).await.unwrap_or_default();

    // Probe each delegated nameserver directly.
    let mut servers = Vec::new();
    for ns in &parent_ns {
        let server = probe_authoritative_server(endpoint, &glue_records, ns, &domain).await;
        servers.push(server);
    }

    // Try to derive the child NS set from direct authoritative probes first.
    let child_from_direct = derive_child_ns(&servers);

    // Fallback: ask the configured resolver without recursion. This is useful
    // when direct port-53 queries cannot reach the authoritative servers but
    // the resolver can still return the child zone NS records.
    let child_probe = if let Some(ref records) = child_from_direct {
        Some(ProbeSuccess {
            data: records.clone(),
            server: endpoint.address.clone(),
            rcode: "noerror".to_string(),
            aa_flag: true,
        })
    } else {
        match probe_child_ns_via_resolver(endpoint, &domain).await {
            ProbeResult::Ok(success) => Some(success),
            ProbeResult::Err(_) => None,
        }
    };

    let child_ns = child_probe
        .as_ref()
        .map(|s| s.data.clone())
        .unwrap_or_default();
    let child_ns_error = if child_from_direct.is_some() {
        None
    } else {
        match probe_child_ns_via_resolver(endpoint, &domain).await {
            ProbeResult::Ok(_) => None,
            ProbeResult::Err(e) => Some(e.error.clone()),
        }
    };

    let ns_consistent = match (&parent_probe, &child_probe) {
        (ProbeResult::Ok(p), Some(c)) => Some(sorted_eq(&p.data, &c.data)),
        _ => None,
    };

    let authoritative = servers
        .iter()
        .any(|s| s.soa_query.ok().map(|q| q.aa_flag).unwrap_or(false))
        || child_probe.as_ref().map(|c| c.aa_flag).unwrap_or(false);

    let ns_serials = collect_serials(&servers);
    let serial_consistent = serial_consistency(&ns_serials);

    Ok(DelegationReportDto {
        domain: domain.trim_end_matches('.').to_string(),
        parent_ns,
        parent_ns_error,
        child_ns,
        child_ns_error,
        ns_consistent,
        glue_records,
        authoritative_servers: servers,
        authoritative,
        ns_serials,
        serial_consistent,
        notes: Vec::new(),
    })
}

async fn probe_parent_ns(
    endpoint: &ResolverEndpointDto,
    domain: &str,
) -> ProbeResult<Vec<String>> {
    match query_once(endpoint, domain, RecordTypeSpec::Ns, QueryOpts::default()).await {
        Ok(result) => {
            let rcode = result.rcode.clone();
            let aa_flag = result.aa_flag;
            let records: Vec<String> = result
                .answers
                .iter()
                .map(|a| normalize_ns_name(&a.data))
                .collect();
            if result.rcode == "noerror" || !records.is_empty() {
                ProbeResult::Ok(ProbeSuccess {
                    data: records,
                    server: endpoint.address.clone(),
                    rcode,
                    aa_flag,
                })
            } else {
                ProbeResult::Err(ProbeFailure {
                    server: endpoint.address.clone(),
                    error: format!("parent NS query returned rcode {rcode} with no records"),
                    rcode: Some(rcode),
                })
            }
        }
        Err(err) => ProbeResult::Err(ProbeFailure {
            server: endpoint.address.clone(),
            error: format!("parent NS query failed: {err}"),
            rcode: None,
        }),
    }
}

async fn gather_glue(
    endpoint: &ResolverEndpointDto,
    domain: &str,
) -> Result<Vec<String>, DnsError> {
    // A non-recursive NS query may return glue in the additional section.
    let result = query_once(
        endpoint,
        domain,
        RecordTypeSpec::Ns,
        QueryOpts {
            rd: false,
            ..QueryOpts::default()
        },
    )
    .await?;
    Ok(result
        .additional_glue
        .iter()
        .map(|a| a.data.clone())
        .collect())
}

async fn probe_authoritative_server(
    endpoint: &ResolverEndpointDto,
    glue_records: &[String],
    ns: &str,
    domain: &str,
) -> AuthoritativeServerDto {
    let addresses = resolve_server_addresses(endpoint, glue_records, ns, domain).await;
    let (ns_query, soa_query) = if addresses.is_empty() {
        (
            ProbeResult::Err(ProbeFailure {
                server: ns.to_string(),
                error: "nameserver hostname did not resolve to any addresses".to_string(),
                rcode: None,
            }),
            ProbeResult::Err(ProbeFailure {
                server: ns.to_string(),
                error: "nameserver hostname did not resolve to any addresses".to_string(),
                rcode: None,
            }),
        )
    } else {
        // Use the first resolved address for direct queries. Probing every
        // address for every server is possible but rarely changes the outcome.
        let addr = &addresses[0];
        let ns_opts = QueryOpts {
            rd: false,
            timeout: Duration::from_millis(800),
            ..QueryOpts::default()
        };
        let soa_opts = QueryOpts {
            rd: false,
            timeout: Duration::from_millis(800),
            ..QueryOpts::default()
        };
        let ns_fut = direct_ns_query(ns, addr, domain, ns_opts);
        let soa_fut = direct_soa_query(ns, addr, domain, soa_opts);
        let (ns_q, soa_q) = tokio::join!(ns_fut, soa_fut);
        (ns_q, soa_q)
    };

    AuthoritativeServerDto {
        name: ns.to_string(),
        addresses,
        ns_query,
        soa_query,
    }
}

async fn direct_ns_query(
    name: &str,
    address: &str,
    domain: &str,
    opts: QueryOpts,
) -> ProbeResult<Vec<String>> {
    direct_query(name, address, domain, RecordTypeSpec::Ns, opts, |result| {
        let records: Vec<String> = result
            .answers
            .iter()
            .map(|a| normalize_ns_name(&a.data))
            .collect();
        if records.is_empty() {
            None
        } else {
            Some(records)
        }
    })
    .await
}

async fn direct_soa_query(
    name: &str,
    address: &str,
    domain: &str,
    opts: QueryOpts,
) -> ProbeResult<u32> {
    direct_query(name, address, domain, RecordTypeSpec::Soa, opts, |result| {
        parse_soa_serial(result)
    })
    .await
}

async fn direct_query<T>(
    name: &str,
    address: &str,
    domain: &str,
    rtype: RecordTypeSpec,
    opts: QueryOpts,
    parse: impl FnOnce(&QueryResultDto) -> Option<T>,
) -> ProbeResult<T> {
    let server_endpoint = ResolverEndpointDto {
        name: name.to_string(),
        protocol: DnsProtocol::Udp,
        address: address.to_string(),
    };
    match query_once(&server_endpoint, domain, rtype, opts).await {
        Ok(result) => {
            let rcode = result.rcode.clone();
            let aa_flag = result.aa_flag;
            match parse(&result) {
                Some(data) => ProbeResult::Ok(ProbeSuccess {
                    data,
                    server: address.to_string(),
                    rcode,
                    aa_flag,
                }),
                None => ProbeResult::Err(ProbeFailure {
                    server: address.to_string(),
                    error: "response contained no usable records".to_string(),
                    rcode: Some(rcode),
                }),
            }
        }
        Err(err) => ProbeResult::Err(ProbeFailure {
            server: address.to_string(),
            error: err.to_string(),
            rcode: None,
        }),
    }
}

async fn probe_child_ns_via_resolver(
    endpoint: &ResolverEndpointDto,
    domain: &str,
) -> ProbeResult<Vec<String>> {
    let opts = QueryOpts {
        rd: false,
        ..QueryOpts::default()
    };
    match query_once(endpoint, domain, RecordTypeSpec::Ns, opts).await {
        Ok(result) => {
            let rcode = result.rcode.clone();
            let aa_flag = result.aa_flag;
            let records: Vec<String> = result
                .answers
                .iter()
                .map(|a| normalize_ns_name(&a.data))
                .collect();
            ProbeResult::Ok(ProbeSuccess {
                data: records,
                server: endpoint.address.clone(),
                rcode,
                aa_flag,
            })
        }
        Err(err) => ProbeResult::Err(ProbeFailure {
            server: endpoint.address.clone(),
            error: format!("authoritative NS query failed: {err}"),
            rcode: None,
        }),
    }
}

async fn resolve_server_addresses(
    endpoint: &ResolverEndpointDto,
    glue_records: &[String],
    ns: &str,
    domain: &str,
) -> Vec<String> {
    let zone = domain.trim_end_matches('.');
    let in_bailiwick = ns == zone || ns.ends_with(&format!(".{zone}"));

    if in_bailiwick {
        let glue: Vec<String> = glue_records
            .iter()
            .filter(|a| a.parse::<std::net::IpAddr>().is_ok())
            .cloned()
            .collect();
        if !glue.is_empty() {
            return glue;
        }
    }

    let mut addrs = Vec::new();
    for rtype in [RecordTypeSpec::A, RecordTypeSpec::Aaaa] {
        if let Ok(result) = query_once(endpoint, ns, rtype, QueryOpts::default()).await {
            for answer in &result.answers {
                if answer.data.parse::<std::net::IpAddr>().is_ok() {
                    addrs.push(answer.data.clone());
                }
            }
        }
    }
    addrs
}

fn derive_child_ns(servers: &[AuthoritativeServerDto]) -> Option<Vec<String>> {
    let mut union: BTreeSet<String> = BTreeSet::new();
    let mut had_authoritative = false;
    for server in servers {
        if let Some(success) = server.ns_query.ok() {
            if success.aa_flag {
                had_authoritative = true;
                union.extend(success.data.iter().cloned());
            }
        }
    }
    if had_authoritative && !union.is_empty() {
        Some(union.into_iter().collect())
    } else {
        None
    }
}

fn collect_serials(servers: &[AuthoritativeServerDto]) -> Vec<NsSerialDto> {
    servers
        .iter()
        .map(|server| {
            let (address, serial) = match &server.soa_query {
                ProbeResult::Ok(success) if success.aa_flag => (
                    success.server.clone(),
                    Some(success.data),
                ),
                _ => (
                    server.addresses.first().cloned().unwrap_or_default(),
                    None,
                ),
            };
            NsSerialDto {
                name: server.name.clone(),
                address,
                serial,
            }
        })
        .collect()
}

fn serial_consistency(serials: &[NsSerialDto]) -> Option<bool> {
    let observed: Vec<u32> = serials
        .iter()
        .filter_map(|s| s.serial)
        .collect();
    if observed.len() < 2 {
        return None;
    }
    let unique: BTreeSet<u32> = observed.into_iter().collect();
    Some(unique.len() == 1)
}

fn normalize_domain(name: &str) -> String {
    let name = name.trim().to_lowercase();
    if name.is_empty() || name.ends_with('.') {
        name
    } else {
        format!("{name}.")
    }
}

fn normalize_ns_name(name: &str) -> String {
    name.trim_end_matches('.').to_lowercase()
}

fn sorted_eq(a: &[String], b: &[String]) -> bool {
    let mut a: Vec<String> = a.to_vec();
    let mut b: Vec<String> = b.to_vec();
    a.sort();
    b.sort();
    a == b
}

fn parse_soa_serial(result: &QueryResultDto) -> Option<u32> {
    result
        .answers
        .first()
        .and_then(|a| {
            let parts: Vec<&str> = a.data.split_whitespace().collect();
            parts.get(2).and_then(|s| s.parse::<u32>().ok())
        })
}
