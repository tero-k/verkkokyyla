use serde::{Deserialize, Serialize};

use crate::dns::client::{DnsProtocol, ResolverEndpointDto};
use crate::dns::error::DnsError;
use crate::dns::query::{query_once, QueryOpts, QueryResultDto, RecordTypeSpec};

/// Result of a resolver integrity comparison between a primary resolver and an
/// authoritative reference endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReportDto {
    pub checked_name: String,
    pub checked_type: String,
    pub primary_rcode: String,
    pub reference_rcode: String,
    pub answers_match: bool,
    pub filtering_ok: bool,
    pub open_resolver: bool,
    pub notes: Vec<String>,
}

/// Result of an AXFR zone-transfer attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneTransferDto {
    pub zone: String,
    pub allowed: bool,
    pub record_count: usize,
    pub serial: Option<u32>,
    pub notes: Vec<String>,
}

/// Compare a resolver's answer for `name`/`rtype` against an authoritative
/// reference endpoint, plus a lightweight open-resolver probe.
pub async fn resolver_integrity_check(
    primary: &ResolverEndpointDto,
    reference: &ResolverEndpointDto,
    name: &str,
    rtype: RecordTypeSpec,
) -> Result<IntegrityReportDto, DnsError> {
    let primary_result = query_once(primary, name, rtype, QueryOpts::default()).await?;
    let reference_opts = QueryOpts {
        rd: false,
        ..QueryOpts::default()
    };
    let reference_result = query_once(reference, name, rtype, reference_opts).await?;

    let primary_answers = answer_set(&primary_result);
    let reference_answers = answer_set(&reference_result);
    let answers_match = primary_answers == reference_answers;
    let filtering_ok = answers_match && primary_result.rcode == reference_result.rcode;

    // Probe for open resolver behaviour using a short timeout so isolated
    // fixtures do not stall.
    let mut notes = Vec::new();
    let open_resolver = match probe_open_resolver(primary).await {
        Ok(open) => open,
        Err(e) => {
            notes.push(format!("open resolver probe failed: {e}"));
            false
        }
    };

    Ok(IntegrityReportDto {
        checked_name: name.trim_end_matches('.').to_string(),
        checked_type: rtype.to_string(),
        primary_rcode: primary_result.rcode.clone(),
        reference_rcode: reference_result.rcode.clone(),
        answers_match,
        filtering_ok,
        open_resolver,
        notes,
    })
}

/// Attempt an AXFR for `zone` over TCP against `endpoint`.
///
/// The caller must opt in by supplying a token equal to the zone being
/// transferred.  Without that token the check returns `allowed: false` without
/// any network traffic.
pub async fn zone_transfer_check(
    endpoint: &ResolverEndpointDto,
    zone: &str,
    opt_in_token: &str,
) -> Result<ZoneTransferDto, DnsError> {
    let zone = normalize_domain(zone);
    let token = normalize_domain(opt_in_token);
    if token != zone {
        return Ok(ZoneTransferDto {
            zone: zone.trim_end_matches('.').to_string(),
            allowed: false,
            record_count: 0,
            serial: None,
            notes: vec!["opt-in token does not match zone".to_string()],
        });
    }

    if endpoint.protocol != DnsProtocol::Tcp {
        return Err(DnsError::UnsupportedTransport(
            "AXFR requires TCP".to_string(),
        ));
    }

    let result = query_once(endpoint, &zone, RecordTypeSpec::Axfr, QueryOpts::default()).await;

    match result {
        Ok(res) => {
            let serial = parse_soa_serial(&res);
            Ok(ZoneTransferDto {
                zone: zone.trim_end_matches('.').to_string(),
                allowed: true,
                record_count: res.answers.len(),
                serial,
                notes: Vec::new(),
            })
        }
        Err(e) => Ok(ZoneTransferDto {
            zone: zone.trim_end_matches('.').to_string(),
            allowed: false,
            record_count: 0,
            serial: None,
            notes: vec![format!("AXFR failed: {e}")],
        }),
    }
}

fn answer_set(result: &QueryResultDto) -> Vec<String> {
    let mut answers: Vec<String> = result
        .answers
        .iter()
        .map(|a| a.data.trim().to_lowercase())
        .collect();
    answers.sort();
    answers
}

async fn probe_open_resolver(endpoint: &ResolverEndpointDto) -> Result<bool, DnsError> {
    let opts = QueryOpts {
        timeout: std::time::Duration::from_millis(500),
        ..QueryOpts::default()
    };
    match query_once(endpoint, "example.com.", RecordTypeSpec::A, opts).await {
        Ok(res) => Ok(res.rcode == "noerror" && !res.answers.is_empty()),
        Err(DnsError::Timeout) => Ok(false),
        Err(e) => Err(e),
    }
}

fn normalize_domain(name: &str) -> String {
    let name = name.trim().to_lowercase();
    if name.ends_with('.') {
        name
    } else {
        format!("{name}.")
    }
}

fn parse_soa_serial(result: &QueryResultDto) -> Option<u32> {
    result
        .answers
        .iter()
        .find(|a| {
            // SOA RData text always starts with the primary NS name.
            a.data.split_whitespace().count() >= 7
        })
        .and_then(|a| {
            let parts: Vec<&str> = a.data.split_whitespace().collect();
            parts.get(2).and_then(|s| s.parse::<u32>().ok())
        })
}
