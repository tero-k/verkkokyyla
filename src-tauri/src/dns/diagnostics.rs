use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::dns::client::ResolverEndpointDto;
use crate::dns::delegation::{delegation_report, DelegationReportDto};
use crate::dns::dnssec::{dnssec_report, DnssecReportDto};
use crate::dns::error::DnsError;
use crate::dns::mx::{mx_report, MxReportDto};
use crate::dns::names::{classify_name, wildcard_check, NameExistenceDto, WildcardCheckDto};
use crate::dns::transport::{edns_support_check, EdnsSupportDto};

/// Severity of a single diagnostic finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticStatus {
    Pass,
    Info,
    Inconclusive,
    Warning,
    Error,
}

impl DiagnosticStatus {
    pub fn rank(self) -> u8 {
        match self {
            DiagnosticStatus::Inconclusive => 0,
            DiagnosticStatus::Pass => 1,
            DiagnosticStatus::Info => 2,
            DiagnosticStatus::Warning => 3,
            DiagnosticStatus::Error => 4,
        }
    }
}

/// A named piece of evidence shown to the user.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvidence {
    pub label: String,
    pub value: String,
}

/// One user-facing diagnostic finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticResultDto {
    pub id: String,
    pub title: String,
    pub status: DiagnosticStatus,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<String>,
    pub evidence: Vec<DiagnosticEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub technical_details: Option<serde_json::Value>,
}

/// Inventory of records found for the queried name. Purely informational.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordInventoryItem {
    pub record_type: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

/// Overall diagnostics report.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnsDiagnosticsDto {
    pub domain: String,
    pub duration_ms: u64,
    pub overall_status: DiagnosticStatus,
    pub summary: String,
    pub results: Vec<DiagnosticResultDto>,
    pub inventory: Vec<RecordInventoryItem>,
    pub technical_details: serde_json::Value,
}

/// Run a full diagnostics sweep for `domain` and turn raw DNS evidence into
/// user-facing findings.
pub async fn run_diagnostics(
    endpoint: &ResolverEndpointDto,
    domain: &str,
) -> Result<DnsDiagnosticsDto, DnsError> {
    let start = tokio::time::Instant::now();
    let domain = normalize_domain(domain);

    // Collect raw evidence in parallel where possible.
    let edns_fut = edns_support_check(endpoint, &domain);
    let existence_fut = classify_name(endpoint, &domain);
    let wildcard_fut = wildcard_check(endpoint, &domain);
    let dnssec_fut = dnssec_report(endpoint, &domain);
    let delegation_fut = delegation_report(endpoint, &domain);
    let mx_fut = mx_report(endpoint, &domain);

    let (
        edns,
        existence,
        wildcard,
        dnssec,
        delegation,
        mx,
    ) = tokio::join!(
        edns_fut,
        existence_fut,
        wildcard_fut,
        dnssec_fut,
        delegation_fut,
        mx_fut,
    );

    let mut results: Vec<DiagnosticResultDto> = Vec::with_capacity(8);
    let mut technical: HashMap<String, serde_json::Value> = HashMap::new();

    let inventory = build_inventory(&existence);

    // Addressing
    results.push(diagnose_addressing(&existence));

    // Nameserver delegation
    let delegation_report = delegation.as_ref().map_err(clone_dns_error)?;
    results.push(diagnose_delegation(delegation_report));

    // SOA consistency
    results.push(diagnose_soa_consistency(delegation_report));

    // Authoritative nameservers
    results.push(diagnose_authoritative_ns(delegation_report));

    // DNSSEC
    let dnssec_report = dnssec.as_ref().map_err(clone_dns_error)?;
    results.push(diagnose_dnssec(dnssec_report));

    // EDNS
    let edns_report = edns.as_ref().map_err(clone_dns_error)?;
    results.push(diagnose_edns(edns_report));

    // Wildcard
    let wildcard_report = wildcard.as_ref().map_err(clone_dns_error)?;
    results.push(diagnose_wildcard(wildcard_report));

    // MX
    let mx_report = mx.as_ref().map_err(clone_dns_error)?;
    results.push(diagnose_mx(mx_report));

    // Collect technical details.
    technical.insert("edns".to_string(), serde_json::to_value(edns).unwrap_or_default());
    technical.insert(
        "nameExistence".to_string(),
        serde_json::to_value(existence).unwrap_or_default(),
    );
    technical.insert("wildcard".to_string(), serde_json::to_value(wildcard).unwrap_or_default());
    technical.insert("dnssec".to_string(), serde_json::to_value(dnssec).unwrap_or_default());
    technical.insert(
        "delegation".to_string(),
        serde_json::to_value(delegation).unwrap_or_default(),
    );
    technical.insert("mx".to_string(), serde_json::to_value(mx).unwrap_or_default());

    let overall = results
        .iter()
        .map(|r| r.status)
        .max_by_key(|s| s.rank())
        .unwrap_or(DiagnosticStatus::Info);

    let summary = build_summary(&results);

    Ok(DnsDiagnosticsDto {
        domain: domain.trim_end_matches('.').to_string(),
        duration_ms: start.elapsed().as_millis() as u64,
        overall_status: overall,
        summary,
        results,
        inventory,
        technical_details: serde_json::to_value(technical).unwrap_or_default(),
    })
}

fn normalize_domain(domain: &str) -> String {
    let name = domain.trim().to_lowercase();
    if name.ends_with('.') || name.is_empty() {
        name
    } else {
        format!("{name}.")
    }
}

fn clone_dns_error(err: &DnsError) -> DnsError {
    DnsError::Io(err.to_string())
}

fn build_inventory(existence: &Result<NameExistenceDto, DnsError>) -> Vec<RecordInventoryItem> {
    let mut inventory = Vec::new();
    if let Ok(e) = existence {
        inventory.push(record_inventory("A", e.a));
        inventory.push(record_inventory("AAAA", e.aaaa));
        inventory.push(record_inventory("MX", e.mx));
        inventory.push(record_inventory("TXT", e.txt));
        inventory.push(record_inventory("CNAME", e.cname));
    }
    inventory
}

fn record_inventory(record_type: &str, present: bool) -> RecordInventoryItem {
    RecordInventoryItem {
        record_type: record_type.to_string(),
        status: if present {
            "Present".to_string()
        } else {
            "Not configured".to_string()
        },
        count: None,
    }
}

fn diagnose_addressing(existence: &Result<NameExistenceDto, DnsError>) -> DiagnosticResultDto {
    match existence {
        Ok(e) => {
            let v4 = if e.a { "IPv4 is configured." } else { "IPv4 is not configured." };
            let v6 = if e.aaaa {
                "IPv6 is configured."
            } else {
                "IPv6 is not configured."
            };
            DiagnosticResultDto {
                id: "addressing".to_string(),
                title: "Addressing".to_string(),
                status: DiagnosticStatus::Info,
                summary: format!("{v4} {v6}"),
                impact: None,
                recommendation: None,
                evidence: vec![
                    DiagnosticEvidence {
                        label: "A record".to_string(),
                        value: if e.a { "Present" } else { "Not found" }.to_string(),
                    },
                    DiagnosticEvidence {
                        label: "AAAA record".to_string(),
                        value: if e.aaaa { "Present" } else { "Not found" }.to_string(),
                    },
                ],
                technical_details: None,
            }
        }
        Err(err) => error_result("addressing", "Addressing", format!("Could not query address records: {err}")),
    }
}

fn diagnose_delegation(report: &DelegationReportDto) -> DiagnosticResultDto {
    let parent_known = report.parent_ns_error.is_none() && !report.parent_ns.is_empty();
    let child_known = report.child_ns_error.is_none() && !report.child_ns.is_empty();

    if !parent_known {
        return DiagnosticResultDto {
            id: "delegation".to_string(),
            title: "Nameserver delegation".to_string(),
            status: DiagnosticStatus::Inconclusive,
            summary: "Parent delegation could not be retrieved.".to_string(),
            impact: None,
            recommendation: None,
            evidence: parent_error_evidence(report),
            technical_details: None,
        };
    }

    if !child_known {
        return DiagnosticResultDto {
            id: "delegation".to_string(),
            title: "Nameserver delegation".to_string(),
            status: DiagnosticStatus::Inconclusive,
            summary: "The parent delegation was retrieved, but the authoritative-zone NS records could not be collected.".to_string(),
            impact: None,
            recommendation: None,
            evidence: parent_and_child_error_evidence(report),
            technical_details: None,
        };
    }

    let parent: std::collections::BTreeSet<_> = report.parent_ns.iter().cloned().collect();
    let child: std::collections::BTreeSet<_> = report.child_ns.iter().cloned().collect();

    if parent == child {
        return DiagnosticResultDto {
            id: "delegation".to_string(),
            title: "Nameserver delegation".to_string(),
            status: DiagnosticStatus::Pass,
            summary: "The parent delegation matches the authoritative NS records.".to_string(),
            impact: None,
            recommendation: None,
            evidence: report
                .child_ns
                .iter()
                .map(|ns| DiagnosticEvidence {
                    label: "Nameserver".to_string(),
                    value: ns.clone(),
                })
                .collect(),
            technical_details: None,
        };
    }

    let only_parent: Vec<_> = parent.difference(&child).cloned().collect();
    let only_child: Vec<_> = child.difference(&parent).cloned().collect();

    let mut evidence = Vec::new();
    for ns in &report.parent_ns {
        let marker = if only_parent.contains(ns) { " (parent only)" } else { "" };
        evidence.push(DiagnosticEvidence {
            label: "Parent delegation".to_string(),
            value: format!("{ns}{marker}"),
        });
    }
    for ns in &report.child_ns {
        let marker = if only_child.contains(ns) { " (authoritative zone only)" } else { "" };
        evidence.push(DiagnosticEvidence {
            label: "Authoritative zone".to_string(),
            value: format!("{ns}{marker}"),
        });
    }

    DiagnosticResultDto {
        id: "delegation".to_string(),
        title: "Nameserver delegation".to_string(),
        status: DiagnosticStatus::Warning,
        summary: "The parent delegation and authoritative zone publish different nameserver sets.".to_string(),
        impact: Some("Resolvers may contact different authoritative servers depending on which delegation data they use.".to_string()),
        recommendation: Some("Compare the nameservers configured at the registrar with the NS records inside the DNS zone and make them consistent.".to_string()),
        evidence,
        technical_details: None,
    }
}

fn parent_error_evidence(report: &DelegationReportDto) -> Vec<DiagnosticEvidence> {
    if let Some(err) = &report.parent_ns_error {
        vec![DiagnosticEvidence {
            label: "Parent NS query".to_string(),
            value: err.clone(),
        }]
    } else {
        Vec::new()
    }
}

fn parent_and_child_error_evidence(report: &DelegationReportDto) -> Vec<DiagnosticEvidence> {
    let mut evidence = parent_error_evidence(report);
    if let Some(err) = &report.child_ns_error {
        evidence.push(DiagnosticEvidence {
            label: "Authoritative NS query".to_string(),
            value: err.clone(),
        });
    }
    evidence
}

fn diagnose_soa_consistency(report: &DelegationReportDto) -> DiagnosticResultDto {
    let valid: Vec<_> = report
        .ns_serials
        .iter()
        .filter(|s| s.serial.is_some())
        .collect();

    let evidence: Vec<_> = report
        .ns_serials
        .iter()
        .map(|s| DiagnosticEvidence {
            label: s.name.clone(),
            value: format!(
                "{}: {}",
                if s.address.is_empty() { "resolver" } else { &s.address },
                s.serial.map(|n| n.to_string()).unwrap_or_else(|| "unavailable".to_string())
            ),
        })
        .collect();

    if valid.is_empty() {
        return DiagnosticResultDto {
            id: "soa-consistency".to_string(),
            title: "SOA consistency".to_string(),
            status: DiagnosticStatus::Inconclusive,
            summary: "No authoritative SOA responses were available, so serial consistency could not be verified.".to_string(),
            impact: None,
            recommendation: None,
            evidence,
            technical_details: None,
        };
    }

    if valid.len() == 1 {
        return DiagnosticResultDto {
            id: "soa-consistency".to_string(),
            title: "SOA consistency".to_string(),
            status: DiagnosticStatus::Inconclusive,
            summary: "Only one authoritative SOA response was collected, so serial consistency could not be compared.".to_string(),
            impact: None,
            recommendation: None,
            evidence,
            technical_details: None,
        };
    }

    let serials: std::collections::BTreeSet<_> = valid.iter().filter_map(|s| s.serial).collect();
    if serials.len() == 1 {
        let serial = serials.into_iter().next().unwrap();
        return DiagnosticResultDto {
            id: "soa-consistency".to_string(),
            title: "SOA consistency".to_string(),
            status: DiagnosticStatus::Pass,
            summary: format!("All checked authoritative nameservers report the same SOA serial ({serial})."),
            impact: None,
            recommendation: None,
            evidence,
            technical_details: None,
        };
    }

    DiagnosticResultDto {
        id: "soa-consistency".to_string(),
        title: "SOA consistency".to_string(),
        status: DiagnosticStatus::Warning,
        summary: "Checked authoritative nameservers return different SOA serial numbers.".to_string(),
        impact: Some("This can indicate an incomplete or delayed zone transfer.".to_string()),
        recommendation: Some("Check that all authoritative nameservers have received the latest zone update.".to_string()),
        evidence,
        technical_details: None,
    }
}

fn diagnose_authoritative_ns(report: &DelegationReportDto) -> DiagnosticResultDto {
    if report.authoritative_servers.is_empty() {
        return DiagnosticResultDto {
            id: "authoritative-ns".to_string(),
            title: "Authoritative nameservers".to_string(),
            status: DiagnosticStatus::Inconclusive,
            summary: "No delegated nameservers were discovered, so reachability could not be checked.".to_string(),
            impact: None,
            recommendation: None,
            evidence: Vec::new(),
            technical_details: None,
        };
    }

    let attempted = report.authoritative_servers.len();
    let successful = report
        .authoritative_servers
        .iter()
        .filter(|s| s.soa_query.is_ok() || s.ns_query.is_ok())
        .count();

    let child_known = report.child_ns_error.is_none() && !report.child_ns.is_empty();

    if successful == 0 && child_known {
        let evidence: Vec<_> = report
            .authoritative_servers
            .iter()
            .map(|s| DiagnosticEvidence {
                label: s.name.clone(),
                value: server_failure_summary(s),
            })
            .collect();
        return DiagnosticResultDto {
            id: "authoritative-ns".to_string(),
            title: "Authoritative nameservers".to_string(),
            status: DiagnosticStatus::Inconclusive,
            summary: format!("Direct queries to the {attempted} delegated nameservers did not succeed; reachability was verified through the configured resolver."),
            impact: None,
            recommendation: None,
            evidence,
            technical_details: None,
        };
    }

    if successful == 0 {
        let evidence: Vec<_> = report
            .authoritative_servers
            .iter()
            .map(|s| DiagnosticEvidence {
                label: s.name.clone(),
                value: server_failure_summary(s),
            })
            .collect();
        return DiagnosticResultDto {
            id: "authoritative-ns".to_string(),
            title: "Authoritative nameservers".to_string(),
            status: DiagnosticStatus::Error,
            summary: format!("None of the {attempted} delegated authoritative nameservers responded to direct DNS queries."),
            impact: Some("Queries that use these nameservers may time out or fail.".to_string()),
            recommendation: Some("Verify the nameserver addresses and that they are reachable on UDP/TCP port 53.".to_string()),
            evidence,
            technical_details: None,
        };
    }

    if report.authoritative {
        let evidence: Vec<_> = report
            .authoritative_servers
            .iter()
            .map(|s| DiagnosticEvidence {
                label: s.name.clone(),
                value: format!(
                    "{} / SOA {}",
                    query_status(&s.ns_query),
                    query_status(&s.soa_query)
                ),
            })
            .collect();
        return DiagnosticResultDto {
            id: "authoritative-ns".to_string(),
            title: "Authoritative nameservers".to_string(),
            status: DiagnosticStatus::Pass,
            summary: format!("{successful} of {attempted} delegated nameservers answered direct queries, and the zone returned an authoritative response."),
            impact: None,
            recommendation: None,
            evidence,
            technical_details: None,
        };
    }

    let evidence: Vec<_> = report
        .authoritative_servers
        .iter()
        .map(|s| DiagnosticEvidence {
            label: s.name.clone(),
            value: format!(
                "{} / SOA {} {}",
                query_status(&s.ns_query),
                query_status(&s.soa_query),
                if s.soa_query
                    .ok()
                    .map(|q| !q.aa_flag)
                    .unwrap_or(false)
                {
                    "(not authoritative)"
                } else {
                    ""
                }
            ),
        })
        .collect();

    DiagnosticResultDto {
        id: "authoritative-ns".to_string(),
        title: "Authoritative nameservers".to_string(),
        status: DiagnosticStatus::Warning,
        summary: "Delegated nameservers answered, but none returned an authoritative response for the zone.".to_string(),
        impact: Some("This may indicate a lame delegation or a referral loop.".to_string()),
        recommendation: Some("Verify that the configured nameservers are authoritative for the zone.".to_string()),
        evidence,
        technical_details: None,
    }
}

fn query_status<T>(probe: &crate::dns::probe::ProbeResult<T>) -> String {
    match probe {
        crate::dns::probe::ProbeResult::Ok(_) => "ok".to_string(),
        crate::dns::probe::ProbeResult::Err(e) => e.error.clone(),
    }
}

fn server_failure_summary(server: &crate::dns::delegation::AuthoritativeServerDto) -> String {
    match (&server.ns_query, &server.soa_query) {
        (crate::dns::probe::ProbeResult::Err(e), _) => e.error.clone(),
        (_, crate::dns::probe::ProbeResult::Err(e)) => e.error.clone(),
        _ => "no response".to_string(),
    }
}

fn diagnose_dnssec(report: &DnssecReportDto) -> DiagnosticResultDto {
    if report.validates {
        return DiagnosticResultDto {
            id: "dnssec".to_string(),
            title: "DNSSEC".to_string(),
            status: DiagnosticStatus::Pass,
            summary: "DNSSEC is enabled and the chain of trust validates successfully.".to_string(),
            impact: None,
            recommendation: None,
            evidence: Vec::new(),
            technical_details: None,
        };
    }

    if !report.has_ds && !report.has_dnskey {
        return DiagnosticResultDto {
            id: "dnssec".to_string(),
            title: "DNSSEC".to_string(),
            status: DiagnosticStatus::Info,
            summary: "DNSSEC is not enabled.".to_string(),
            impact: Some("This is a valid DNS configuration, but responses are not protected by DNSSEC validation.".to_string()),
            recommendation: None,
            evidence: Vec::new(),
            technical_details: None,
        };
    }

    if report.has_ds && !report.has_dnskey {
        return DiagnosticResultDto {
            id: "dnssec".to_string(),
            title: "DNSSEC".to_string(),
            status: DiagnosticStatus::Error,
            summary: "DNSSEC is configured but the zone does not publish a DNSKEY record.".to_string(),
            impact: Some("The parent zone has a DS record, but resolvers cannot find the matching DNSKEY, so validation fails.".to_string()),
            recommendation: Some("Publish the correct DNSKEY record in the zone, or remove the DS record at the registrar if DNSSEC is not intended.".to_string()),
            evidence: Vec::new(),
            technical_details: None,
        };
    }

    if report.has_ds && report.has_dnskey {
        return DiagnosticResultDto {
            id: "dnssec".to_string(),
            title: "DNSSEC".to_string(),
            status: DiagnosticStatus::Error,
            summary: "DNSSEC is configured but validation fails.".to_string(),
            impact: Some("The DS record at the parent and/or the signatures in the zone do not form a valid chain of trust.".to_string()),
            recommendation: Some("Check that the DNSKEY matches the DS record and that signatures have not expired.".to_string()),
            evidence: report.notes.iter().map(|n| DiagnosticEvidence { label: "Observation".to_string(), value: n.clone() }).collect(),
            technical_details: None,
        };
    }

    // has_dnskey but no DS
    DiagnosticResultDto {
        id: "dnssec".to_string(),
        title: "DNSSEC".to_string(),
        status: DiagnosticStatus::Warning,
        summary: "The zone publishes a DNSKEY but the parent zone has no DS record.".to_string(),
        impact: Some("Resolvers cannot validate the zone because the chain of trust is not established at the parent.".to_string()),
        recommendation: Some("Add the matching DS record at the registrar to enable DNSSEC validation.".to_string()),
        evidence: Vec::new(),
        technical_details: None,
    }
}

fn diagnose_edns(report: &EdnsSupportDto) -> DiagnosticResultDto {
    if !report.opt_present {
        return DiagnosticResultDto {
            id: "edns".to_string(),
            title: "EDNS compatibility".to_string(),
            status: DiagnosticStatus::Info,
            summary: "The server did not include an OPT record in the response.".to_string(),
            impact: Some("EDNS extensions such as larger UDP payloads and DNSSEC OK may not be supported.".to_string()),
            recommendation: None,
            evidence: vec![DiagnosticEvidence {
                label: "Responder".to_string(),
                value: report.responder.clone(),
            }],
            technical_details: None,
        };
    }

    let is_badvertest = report.full_rcode == 16;

    if is_badvertest {
        return DiagnosticResultDto {
            id: "edns".to_string(),
            title: "EDNS unsupported-version handling".to_string(),
            status: DiagnosticStatus::Pass,
            summary: "The server correctly returned BADVERS (RCODE 16) for an unsupported EDNS version.".to_string(),
            impact: None,
            recommendation: None,
            evidence: edns_evidence(report),
            technical_details: None,
        };
    }

    DiagnosticResultDto {
        id: "edns".to_string(),
        title: "EDNS unsupported-version handling".to_string(),
        status: DiagnosticStatus::Info,
        summary: "The server returned an unexpected response to an unsupported EDNS-version probe.".to_string(),
        impact: Some("This does not affect normal DNS resolution; it is a protocol-compliance observation.".to_string()),
        recommendation: None,
        evidence: edns_evidence(report),
        technical_details: None,
    }
}

fn edns_evidence(report: &EdnsSupportDto) -> Vec<DiagnosticEvidence> {
    vec![
        DiagnosticEvidence {
            label: "Responder".to_string(),
            value: report.responder.clone(),
        },
        DiagnosticEvidence {
            label: "Requested EDNS version".to_string(),
            value: report.requested_version.to_string(),
        },
        DiagnosticEvidence {
            label: "Header RCODE".to_string(),
            value: report.header_rcode.to_string(),
        },
        DiagnosticEvidence {
            label: "Extended RCODE".to_string(),
            value: report.extended_rcode.to_string(),
        },
        DiagnosticEvidence {
            label: "Full RCODE".to_string(),
            value: format!("{} ({})", report.full_rcode, report.full_rcode_name),
        },
    ]
}

fn diagnose_wildcard(report: &WildcardCheckDto) -> DiagnosticResultDto {
    if report.wildcard {
        return DiagnosticResultDto {
            id: "wildcard".to_string(),
            title: "Wildcard DNS".to_string(),
            status: DiagnosticStatus::Info,
            summary: "A wildcard DNS record was detected.".to_string(),
            impact: None,
            recommendation: None,
            evidence: vec![DiagnosticEvidence {
                label: "Probe hostname".to_string(),
                value: report.probe.clone(),
            }],
            technical_details: None,
        };
    }

    DiagnosticResultDto {
        id: "wildcard".to_string(),
        title: "Wildcard DNS".to_string(),
        status: DiagnosticStatus::Pass,
        summary: "No wildcard DNS record was detected.".to_string(),
        impact: None,
        recommendation: None,
        evidence: Vec::new(),
        technical_details: None,
    }
}

fn diagnose_mx(report: &MxReportDto) -> DiagnosticResultDto {
    if !report.has_mx {
        return DiagnosticResultDto {
            id: "mx".to_string(),
            title: "Mail exchangers (MX)".to_string(),
            status: DiagnosticStatus::Info,
            summary: "No MX records are configured.".to_string(),
            impact: Some("Mail cannot be delivered to this domain via MX records. This may be intentional if the domain does not receive email.".to_string()),
            recommendation: None,
            evidence: Vec::new(),
            technical_details: None,
        };
    }

    if report.entries.is_empty() {
        return DiagnosticResultDto {
            id: "mx".to_string(),
            title: "Mail exchangers (MX)".to_string(),
            status: DiagnosticStatus::Info,
            summary: "MX records are configured.".to_string(),
            impact: None,
            recommendation: None,
            evidence: Vec::new(),
            technical_details: None,
        };
    }

    let errors: Vec<_> = report.entries.iter().filter(|e| !e.issues.is_empty()).collect();
    if errors.is_empty() {
        let targets: Vec<String> = report.entries.iter().map(|e| e.target.clone()).collect();
        return DiagnosticResultDto {
            id: "mx".to_string(),
            title: "Mail exchangers (MX)".to_string(),
            status: DiagnosticStatus::Pass,
            summary: format!("{} MX record(s) configured and all targets look usable.", report.entries.len()),
            impact: None,
            recommendation: None,
            evidence: vec![DiagnosticEvidence {
                label: "Targets".to_string(),
                value: targets.join(", "),
            }],
            technical_details: None,
        };
    }

    let mut evidence = Vec::new();
    for e in errors {
        evidence.push(DiagnosticEvidence {
            label: format!("{} (priority {})", e.target, e.priority),
            value: e.issues.join("; "),
        });
    }

    DiagnosticResultDto {
        id: "mx".to_string(),
        title: "Mail exchangers (MX)".to_string(),
        status: DiagnosticStatus::Warning,
        summary: "One or more MX records have problems.".to_string(),
        impact: Some("Mail delivery may fail or be unreliable for the affected targets.".to_string()),
        recommendation: Some("Review the MX targets and fix the listed issues.".to_string()),
        evidence,
        technical_details: None,
    }
}

fn error_result(id: &str, title: &str, summary: String) -> DiagnosticResultDto {
    DiagnosticResultDto {
        id: id.to_string(),
        title: title.to_string(),
        status: DiagnosticStatus::Error,
        summary,
        impact: None,
        recommendation: None,
        evidence: Vec::new(),
        technical_details: None,
    }
}

fn build_summary(results: &[DiagnosticResultDto]) -> String {
    let errors = results.iter().filter(|r| r.status == DiagnosticStatus::Error).count();
    let warnings = results.iter().filter(|r| r.status == DiagnosticStatus::Warning).count();
    let inconclusive = results
        .iter()
        .filter(|r| r.status == DiagnosticStatus::Inconclusive)
        .count();
    let passed = results.iter().filter(|r| r.status == DiagnosticStatus::Pass).count();
    let info = results.iter().filter(|r| r.status == DiagnosticStatus::Info).count();

    let mut parts = Vec::new();
    if errors == 1 {
        parts.push("1 error".to_string());
    } else if errors > 1 {
        parts.push(format!("{errors} errors"));
    }
    if warnings == 1 {
        parts.push("1 warning".to_string());
    } else if warnings > 1 {
        parts.push(format!("{warnings} warnings"));
    }
    if inconclusive == 1 {
        parts.push("1 not verified".to_string());
    } else if inconclusive > 1 {
        parts.push(format!("{inconclusive} not verified"));
    }
    if passed > 0 {
        parts.push(format!("{passed} passed"));
    }
    if info > 0 {
        parts.push(format!("{info} informational"));
    }

    if parts.is_empty() {
        "No checks completed.".to_string()
    } else {
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dns::delegation::{AuthoritativeServerDto, NsSerialDto};
    use crate::dns::probe::{ProbeFailure, ProbeSuccess};

    #[test]
    fn diagnostic_result_serializes_empty_evidence_array() {
        let dto = DiagnosticResultDto {
            id: "test".to_string(),
            title: "Test".to_string(),
            status: DiagnosticStatus::Pass,
            summary: "ok".to_string(),
            impact: None,
            recommendation: None,
            evidence: Vec::new(),
            technical_details: None,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(json.contains("\"evidence\":[]"), "expected empty evidence array in JSON: {json}");
    }

    fn delegation_report(
        parent_ns: Vec<String>,
        parent_ns_error: Option<String>,
        child_ns: Vec<String>,
        child_ns_error: Option<String>,
    ) -> DelegationReportDto {
        DelegationReportDto {
            domain: "example.com".to_string(),
            parent_ns,
            parent_ns_error,
            child_ns,
            child_ns_error,
            ns_consistent: None,
            glue_records: Vec::new(),
            authoritative_servers: Vec::new(),
            authoritative: false,
            ns_serials: Vec::new(),
            serial_consistent: None,
            notes: Vec::new(),
        }
    }

    #[test]
    fn delegation_is_inconclusive_when_parent_ns_query_fails() {
        let report = delegation_report(
            Vec::new(),
            Some("timeout".to_string()),
            Vec::new(),
            None,
        );
        let result = diagnose_delegation(&report);
        assert_eq!(result.status, DiagnosticStatus::Inconclusive);
        assert!(!result.summary.to_lowercase().contains("mismatch"));
    }

    #[test]
    fn delegation_is_inconclusive_when_child_ns_query_fails() {
        let report = delegation_report(
            vec!["ns1.example.com".to_string()],
            None,
            Vec::new(),
            Some("timeout".to_string()),
        );
        let result = diagnose_delegation(&report);
        assert_eq!(result.status, DiagnosticStatus::Inconclusive);
        assert!(!result.evidence.iter().any(|e| e.value.contains("parent only")));
    }

    #[test]
    fn delegation_passes_when_parent_and_child_match() {
        let report = delegation_report(
            vec!["ns1.example.com".to_string(), "ns2.example.com".to_string()],
            None,
            vec!["ns1.example.com".to_string(), "ns2.example.com".to_string()],
            None,
        );
        let result = diagnose_delegation(&report);
        assert_eq!(result.status, DiagnosticStatus::Pass);
    }

    #[test]
    fn delegation_warns_when_parent_and_child_differ() {
        let report = delegation_report(
            vec!["ns1.example.com".to_string()],
            None,
            vec!["ns2.example.com".to_string()],
            None,
        );
        let result = diagnose_delegation(&report);
        assert_eq!(result.status, DiagnosticStatus::Warning);
        assert!(result.evidence.iter().any(|e| e.value.contains("parent only")));
    }

    #[test]
    fn failed_child_ns_lookup_does_not_generate_parent_only_evidence() {
        let report = delegation_report(
            vec!["ns1.example.com".to_string(), "ns2.example.com".to_string()],
            None,
            Vec::new(),
            Some("timeout".to_string()),
        );
        let result = diagnose_delegation(&report);
        assert!(!result
            .evidence
            .iter()
            .any(|e| e.value.contains("parent only")));
    }

    fn soa_report(serials: Vec<Option<u32>>) -> DelegationReportDto {
        DelegationReportDto {
            domain: "example.com".to_string(),
            parent_ns: Vec::new(),
            parent_ns_error: Some("not used".to_string()),
            child_ns: Vec::new(),
            child_ns_error: Some("not used".to_string()),
            ns_consistent: None,
            glue_records: Vec::new(),
            authoritative_servers: Vec::new(),
            authoritative: false,
            ns_serials: serials
                .into_iter()
                .enumerate()
                .map(|(i, serial)| NsSerialDto {
                    name: format!("ns{i}.example.com"),
                    address: format!("192.0.2.{i}"),
                    serial,
                })
                .collect(),
            serial_consistent: None,
            notes: Vec::new(),
        }
    }

    #[test]
    fn soa_consistency_is_inconclusive_with_zero_responses() {
        let result = diagnose_soa_consistency(&soa_report(Vec::new()));
        assert_eq!(result.status, DiagnosticStatus::Inconclusive);
    }

    #[test]
    fn soa_consistency_is_inconclusive_with_one_response() {
        let result = diagnose_soa_consistency(&soa_report(vec![Some(1)]));
        assert_eq!(result.status, DiagnosticStatus::Inconclusive);
    }

    #[test]
    fn soa_consistency_passes_with_two_matching_serials() {
        let result = diagnose_soa_consistency(&soa_report(vec![Some(42), Some(42)]));
        assert_eq!(result.status, DiagnosticStatus::Pass);
    }

    #[test]
    fn soa_consistency_warns_with_two_differing_serials() {
        let result = diagnose_soa_consistency(&soa_report(vec![Some(1), Some(2)]));
        assert_eq!(result.status, DiagnosticStatus::Warning);
    }

    fn authoritatives(
        servers: Vec<(Vec<String>, crate::dns::probe::ProbeResult<Vec<String>>, crate::dns::probe::ProbeResult<u32>)>,
        child_known: bool,
    ) -> DelegationReportDto {
        DelegationReportDto {
            domain: "example.com".to_string(),
            parent_ns: vec!["ns1.example.com".to_string()],
            parent_ns_error: None,
            child_ns: if child_known {
                vec!["ns1.example.com".to_string()]
            } else {
                Vec::new()
            },
            child_ns_error: if child_known { None } else { Some("timeout".to_string()) },
            ns_consistent: None,
            glue_records: Vec::new(),
            authoritative_servers: servers
                .into_iter()
                .enumerate()
                .map(|(i, (addresses, ns_query, soa_query))| AuthoritativeServerDto {
                    name: format!("ns{i}.example.com"),
                    addresses,
                    ns_query,
                    soa_query,
                })
                .collect(),
            authoritative: false,
            ns_serials: Vec::new(),
            serial_consistent: None,
            notes: Vec::new(),
        }
    }

    fn ok_ns() -> crate::dns::probe::ProbeResult<Vec<String>> {
        crate::dns::probe::ProbeResult::Ok(ProbeSuccess {
            data: vec!["ns1.example.com".to_string()],
            server: "192.0.2.1".to_string(),
            rcode: "noerror".to_string(),
            aa_flag: true,
        })
    }

    fn err<T>() -> crate::dns::probe::ProbeResult<T> {
        crate::dns::probe::ProbeResult::Err(ProbeFailure {
            server: "192.0.2.1".to_string(),
            error: "timeout".to_string(),
            rcode: None,
        })
    }

    #[test]
    fn authoritative_ns_is_inconclusive_when_direct_queries_fail_but_resolver_succeeds() {
        let report = authoritatives(
            vec![(Vec::new(), err::<Vec<String>>(), err::<u32>())],
            true,
        );
        let result = diagnose_authoritative_ns(&report);
        assert_eq!(result.status, DiagnosticStatus::Inconclusive);
        assert!(!result.summary.to_lowercase().contains("error"));
    }

    #[test]
    fn authoritative_ns_is_error_when_all_direct_queries_fail_and_resolver_unknown() {
        let report = authoritatives(
            vec![(Vec::new(), err::<Vec<String>>(), err::<u32>())],
            false,
        );
        let result = diagnose_authoritative_ns(&report);
        assert_eq!(result.status, DiagnosticStatus::Error);
    }

    #[test]
    fn authoritative_ns_passes_when_servers_respond() {
        let report = DelegationReportDto {
            authoritative: true,
            ..authoritatives(vec![(vec!["192.0.2.1".to_string()], ok_ns(), ok_soa())], true)
        };
        let result = diagnose_authoritative_ns(&report);
        assert_eq!(result.status, DiagnosticStatus::Pass);
    }

    fn ok_soa() -> crate::dns::probe::ProbeResult<u32> {
        crate::dns::probe::ProbeResult::Ok(ProbeSuccess {
            data: 1,
            server: "192.0.2.1".to_string(),
            rcode: "noerror".to_string(),
            aa_flag: true,
        })
    }

    fn edns_report(
        full_rcode: u16,
        full_rcode_name: &str,
        opt_present: bool,
    ) -> EdnsSupportDto {
        EdnsSupportDto {
            opt_present,
            edns_version: 0,
            responder: "1.1.1.1".to_string(),
            requested_version: 1,
            header_rcode: full_rcode & 0x000F,
            extended_rcode: ((full_rcode >> 4) & 0x00FF) as u8,
            full_rcode,
            full_rcode_name: full_rcode_name.to_string(),
        }
    }

    #[test]
    fn edns_reports_pass_for_badvers_full_rcode_16() {
        let result = diagnose_edns(&edns_report(16, "badvers", true));
        assert_eq!(result.status, DiagnosticStatus::Pass);
        assert!(result.summary.to_lowercase().contains("badvers"));
    }

    #[test]
    fn edns_reports_info_for_noerror_full_rcode_0() {
        let result = diagnose_edns(&edns_report(0, "noerror", true));
        assert_eq!(result.status, DiagnosticStatus::Info);
    }
}
