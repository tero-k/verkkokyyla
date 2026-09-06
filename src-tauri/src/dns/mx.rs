use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::dns::client::ResolverEndpointDto;
use crate::dns::error::DnsError;
use crate::dns::names::normalize_name;
use crate::dns::query::{query_once, QueryOpts, RecordTypeSpec};

/// Result of an MX validation diagnostic run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MxReportDto {
    pub has_mx: bool,
    pub entries: Vec<MxEntryDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MxEntryDto {
    pub target: String,
    pub priority: u16,
    pub issues: Vec<String>,
}

/// Validate MX records for `domain`.
pub async fn mx_report(endpoint: &ResolverEndpointDto, domain: &str) -> Result<MxReportDto, DnsError> {
    let name = normalize_name(domain);
    let result = query_once(endpoint, &name, RecordTypeSpec::Mx, QueryOpts::default()).await?;

    if result.answers.is_empty() {
        return Ok(MxReportDto {
            has_mx: false,
            entries: Vec::new(),
        });
    }

    let mut entries = Vec::new();
    let mut seen_targets: HashSet<String> = HashSet::new();

    for answer in &result.answers {
        let (priority, target) = match parse_mx_rdata(&answer.data) {
            Some(parts) => parts,
            None => {
                entries.push(MxEntryDto {
                    target: answer.data.clone(),
                    priority: 0,
                    issues: vec!["Unparseable MX record".to_string()],
                });
                continue;
            }
        };

        let mut issues = Vec::new();

        // Null MX semantics: RFC 7505.
        if target == "." {
            if priority != 0 {
                issues.push("Null MX target '.' must have priority 0".to_string());
            }
            entries.push(MxEntryDto {
                target: ".".to_string(),
                priority,
                issues,
            });
            continue;
        }

        if target.parse::<std::net::IpAddr>().is_ok() {
            issues.push("MX target must be a host name, not an IP address".to_string());
        }

        if target.eq_ignore_ascii_case(name.trim_end_matches('.')) {
            issues.push("MX target points back to the queried domain, which is usually a misconfiguration".to_string());
        }

        if !seen_targets.insert(target.clone()) {
            issues.push("Duplicate MX target".to_string());
        }

        // Check CNAME.
        if let Ok(cname) = query_once(endpoint, &target, RecordTypeSpec::Cname, QueryOpts::default()).await {
            if !cname.answers.is_empty() {
                issues.push("MX target is a CNAME; RFC 5321 requires MX targets to be host names, not aliases".to_string());
            }
        }

        // Check address records.
        let a_ok = query_once(endpoint, &target, RecordTypeSpec::A, QueryOpts::default())
            .await
            .map(|r| !r.answers.is_empty())
            .unwrap_or(false);
        let aaaa_ok = query_once(endpoint, &target, RecordTypeSpec::Aaaa, QueryOpts::default())
            .await
            .map(|r| !r.answers.is_empty())
            .unwrap_or(false);
        if !a_ok && !aaaa_ok {
            issues.push("MX target does not resolve to an A or AAAA record".to_string());
        }

        entries.push(MxEntryDto {
            target,
            priority,
            issues,
        });
    }

    Ok(MxReportDto {
        has_mx: true,
        entries,
    })
}

fn parse_mx_rdata(data: &str) -> Option<(u16, String)> {
    let data = data.trim();
    // Hickory renders MX as "<priority> <target>" (target may include a trailing dot).
    let mut parts = data.split_whitespace();
    let priority = parts.next()?.parse::<u16>().ok()?;
    let target = parts.next()?;
    if parts.next().is_some() {
        // More than two tokens is unexpected.
        return None;
    }
    Some((priority, target.trim_end_matches('.').to_lowercase()))
}
