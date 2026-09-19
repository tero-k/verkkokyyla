use std::collections::{HashMap, HashSet};

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};

use crate::dns::client::ResolverEndpointDto;
use crate::dns::error::DnsError;
use crate::dns::query::{query_once, QueryOpts, RecordTypeSpec};

const SPF_LOOKUP_LIMIT: usize = 10;
const SPF_MAX_DEPTH: usize = 5;

/// Verdict for one section of the email-security report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EmailSecurityVerdict {
    Pass,
    Warn,
    Fail,
}

/// Aggregated email-security report (SPF, DKIM, DMARC).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailSecurityReportDto {
    pub domain: String,
    pub spf: SpfReportDto,
    pub dkim: Vec<DkimSelectorReportDto>,
    pub dmarc: DmarcReportDto,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpfReportDto {
    pub verdict: EmailSecurityVerdict,
    pub record: Option<String>,
    pub record_count: usize,
    pub all_mechanism: Option<String>,
    pub lookup_count: usize,
    pub lookup_limit_ok: bool,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DkimSelectorReportDto {
    pub selector: String,
    pub found: bool,
    pub record: Option<String>,
    pub key_present: bool,
    pub key_bits_approx: Option<usize>,
    pub revoked: bool,
    pub verdict: EmailSecurityVerdict,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DmarcReportDto {
    pub verdict: EmailSecurityVerdict,
    pub found: bool,
    pub record: Option<String>,
    pub policy: Option<String>,
    pub subdomain_policy: Option<String>,
    pub pct: Option<u8>,
    pub reporting_address: Option<String>,
    pub notes: Vec<String>,
}

/// Build an email-security report for `domain`.
///
/// `dkim_selectors` lists selectors to probe; if empty a small default set is used.
pub async fn email_security_report(
    endpoint: &ResolverEndpointDto,
    domain: &str,
    dkim_selectors: &[String],
) -> Result<EmailSecurityReportDto, DnsError> {
    let start = tokio::time::Instant::now();
    let canonical = canonicalize_domain(domain);

    let spf = evaluate_spf(endpoint, &canonical).await;
    let dkim = evaluate_dkim(endpoint, &canonical, dkim_selectors).await;
    let dmarc = evaluate_dmarc(endpoint, &canonical).await;

    Ok(EmailSecurityReportDto {
        domain: canonical.trim_end_matches('.').to_string(),
        spf,
        dkim,
        dmarc,
        elapsed_ms: start.elapsed().as_millis() as u64,
    })
}

fn canonicalize_domain(name: &str) -> String {
    let name = name.trim().to_lowercase();
    if name.ends_with('.') {
        name
    } else {
        format!("{name}.")
    }
}

// -----------------------------------------------------------------------------
// TXT helpers
// -----------------------------------------------------------------------------

async fn query_txt(endpoint: &ResolverEndpointDto, name: &str) -> Result<Vec<String>, DnsError> {
    let result = query_once(endpoint, name, RecordTypeSpec::Txt, QueryOpts::default()).await?;
    Ok(result
        .answers
        .into_iter()
        .map(|a| unquote_txt(&a.data))
        .collect())
}

/// Hickory renders TXT data as either the plain content or multiple quoted chunks.
/// Keep the content verbatim when there are no quotes; otherwise concatenate
/// quoted chunks, dropping the quotes and the whitespace between them.
fn unquote_txt(data: &str) -> String {
    if !data.contains('"') {
        return data.to_string();
    }
    let mut out = String::with_capacity(data.len());
    let mut in_quotes = false;
    for ch in data.chars() {
        match ch {
            '"' => in_quotes = !in_quotes,
            _ if in_quotes => out.push(ch),
            _ => {}
        }
    }
    out
}

// -----------------------------------------------------------------------------
// SPF
// -----------------------------------------------------------------------------

async fn evaluate_spf(endpoint: &ResolverEndpointDto, domain: &str) -> SpfReportDto {
    let mut notes = Vec::new();
    let mut verdict = EmailSecurityVerdict::Pass;

    let records = match query_txt(endpoint, domain).await {
        Ok(recs) => recs,
        Err(e) => {
            return SpfReportDto {
                verdict: EmailSecurityVerdict::Fail,
                record: None,
                record_count: 0,
                all_mechanism: None,
                lookup_count: 0,
                lookup_limit_ok: false,
                notes: vec![format!("TXT query failed: {e}")],
            }
        }
    };

    let spf_records: Vec<String> = records
        .into_iter()
        .filter(|r| r.trim().to_lowercase().starts_with("v=spf1"))
        .collect();

    if spf_records.is_empty() {
        return SpfReportDto {
            verdict: EmailSecurityVerdict::Fail,
            record: None,
            record_count: 0,
            all_mechanism: None,
            lookup_count: 0,
            lookup_limit_ok: false,
            notes: vec!["No SPF record found".to_string()],
        };
    }

    if spf_records.len() > 1 {
        notes.push("Multiple SPF records found; receivers will reject them".to_string());
        verdict = EmailSecurityVerdict::Fail;
    }

    let record_count = spf_records.len();
    let record = spf_records.into_iter().next().unwrap();
    let all = find_all_mechanism(&record);
    let all_mechanism = all.map(|(qual, _)| qual.to_string());

    if let Some((qual, _)) = all {
        match qual {
            "+all" => {
                notes.push(
                    "SPF uses '+all' which allows any sender; messages can be spoofed".to_string(),
                );
                verdict = EmailSecurityVerdict::Fail;
            }
            "?all" => {
                notes.push("SPF uses '?all' (neutral); it provides no useful policy".to_string());
                verdict = verdict.max(EmailSecurityVerdict::Warn);
            }
            "~all" => notes
                .push("SPF uses '~all' (softfail); acceptable but '-all' is stronger".to_string()),
            "-all" => notes.push("SPF uses '-all' (hard fail); best practice".to_string()),
            _ => {}
        }
    } else {
        notes.push("No 'all' mechanism found; SPF record may be incomplete".to_string());
        verdict = verdict.max(EmailSecurityVerdict::Warn);
    }

    // Look for obviously broad IP ranges.
    if record.contains("ip4:0.0.0.0/0") || record.contains("ip6::/0") {
        notes.push("SPF authorizes the entire internet (0.0.0.0/0 or ::/0)".to_string());
        verdict = EmailSecurityVerdict::Fail;
    }
    if record.to_lowercase().contains(" ptr:")
        || record
            .split_whitespace()
            .any(|t| t.trim_start_matches("+-~?").starts_with("ptr"))
    {
        notes.push("SPF uses 'ptr' mechanism; it is deprecated and slow".to_string());
        verdict = verdict.max(EmailSecurityVerdict::Warn);
    }

    let lookup_count =
        match count_spf_lookups(endpoint, domain, &record, 0, &mut HashSet::new()).await {
            Ok(n) => n,
            Err(e) => {
                notes.push(format!("Could not evaluate SPF lookups: {e}"));
                verdict = verdict.max(EmailSecurityVerdict::Warn);
                0
            }
        };
    let lookup_limit_ok = lookup_count <= SPF_LOOKUP_LIMIT;
    if !lookup_limit_ok {
        notes.push(format!(
            "SPF requires {lookup_count} DNS lookups, exceeding the RFC 7208 limit of {SPF_LOOKUP_LIMIT}"
        ));
        verdict = EmailSecurityVerdict::Fail;
    }

    SpfReportDto {
        verdict,
        record: Some(record.clone()),
        record_count,
        all_mechanism,
        lookup_count,
        lookup_limit_ok,
        notes,
    }
}

fn find_all_mechanism(record: &str) -> Option<(&str, &str)> {
    for token in record.split_whitespace() {
        let stripped = strip_qualifier(token);
        if stripped == "all" {
            let qual = token
                .chars()
                .next()
                .filter(|c| matches!(c, '+' | '-' | '~' | '?'))
                .map_or("+", |c| {
                    let s = &token[..c.len_utf8()];
                    match s {
                        "-" => "-all",
                        "~" => "~all",
                        "?" => "?all",
                        _ => "+all",
                    }
                });
            return Some((qual, token));
        }
    }
    None
}

fn strip_qualifier(token: &str) -> &str {
    if let Some(first) = token.chars().next() {
        if matches!(first, '+' | '-' | '~' | '?') {
            return &token[first.len_utf8()..];
        }
    }
    token
}

async fn count_spf_lookups(
    endpoint: &ResolverEndpointDto,
    domain: &str,
    record: &str,
    depth: usize,
    visited: &mut HashSet<String>,
) -> Result<usize, DnsError> {
    if depth > SPF_MAX_DEPTH {
        return Ok(0);
    }
    if !visited.insert(domain.to_string()) {
        return Ok(0);
    }

    let mut count = 0usize;
    for token in record.split_whitespace() {
        let stripped = strip_qualifier(token);

        if let Some(target) = stripped.strip_prefix("include:") {
            count += 1;
            let target = canonicalize_domain(target);
            let child = query_txt(endpoint, &target).await?;
            if let Some(child_spf) = child
                .into_iter()
                .find(|r| r.to_lowercase().starts_with("v=spf1"))
            {
                count += Box::pin(count_spf_lookups(
                    endpoint,
                    &target,
                    &child_spf,
                    depth + 1,
                    visited,
                ))
                .await?;
            }
        } else if stripped == "a"
            || stripped.starts_with("a:")
            || stripped.starts_with("a/")
            || stripped == "mx"
            || stripped.starts_with("mx:")
            || stripped.starts_with("mx/")
            || stripped == "ptr"
            || stripped.starts_with("ptr:")
            || stripped.starts_with("exists:")
        {
            count += 1;
        } else if let Some(target) = stripped.strip_prefix("redirect=") {
            count += 1;
            let target = canonicalize_domain(target);
            let child = query_txt(endpoint, &target).await?;
            if let Some(child_spf) = child
                .into_iter()
                .find(|r| r.to_lowercase().starts_with("v=spf1"))
            {
                count += Box::pin(count_spf_lookups(
                    endpoint,
                    &target,
                    &child_spf,
                    depth + 1,
                    visited,
                ))
                .await?;
            }
        }
    }
    Ok(count)
}

// -----------------------------------------------------------------------------
// DKIM
// -----------------------------------------------------------------------------

async fn evaluate_dkim(
    endpoint: &ResolverEndpointDto,
    domain: &str,
    selectors: &[String],
) -> Vec<DkimSelectorReportDto> {
    let selectors: Vec<String> = if selectors.is_empty() {
        vec![
            "default".to_string(),
            "google".to_string(),
            "selector1".to_string(),
            "selector2".to_string(),
            "k1".to_string(),
            "mail".to_string(),
            "dkim".to_string(),
        ]
    } else {
        selectors.to_vec()
    };

    let mut out = Vec::with_capacity(selectors.len());
    for selector in selectors {
        let name = format!("{}._domainkey.{}", selector.trim(), domain);
        out.push(evaluate_dkim_selector(endpoint, &selector, &name).await);
    }
    out
}

async fn evaluate_dkim_selector(
    endpoint: &ResolverEndpointDto,
    selector: &str,
    name: &str,
) -> DkimSelectorReportDto {
    let mut notes = Vec::new();
    let mut verdict = EmailSecurityVerdict::Pass;

    let records = match query_txt(endpoint, name).await {
        Ok(r) => r,
        Err(e) => {
            return DkimSelectorReportDto {
                selector: selector.to_string(),
                found: false,
                record: None,
                key_present: false,
                key_bits_approx: None,
                revoked: false,
                verdict: EmailSecurityVerdict::Fail,
                notes: vec![format!("TXT query failed: {e}")],
            }
        }
    };

    let record = match records.into_iter().next() {
        Some(r) => r,
        None => {
            return DkimSelectorReportDto {
                selector: selector.to_string(),
                found: false,
                record: None,
                key_present: false,
                key_bits_approx: None,
                revoked: false,
                verdict: EmailSecurityVerdict::Fail,
                notes: vec!["No DKIM record found for this selector".to_string()],
            }
        }
    };

    if !record.to_lowercase().starts_with("v=dkim1") {
        notes.push("DKIM record does not start with 'v=DKIM1'".to_string());
        verdict = EmailSecurityVerdict::Fail;
    }

    let tags = parse_tag_value(&record);
    let p = tags.get("p").map(String::as_str);
    let key_present = p.is_some_and(|v| !v.is_empty());
    let revoked = p == Some("");

    if revoked {
        notes.push("DKIM key is revoked (empty p=)".to_string());
        verdict = EmailSecurityVerdict::Fail;
    } else if !key_present {
        notes.push("DKIM record is missing a public key (p=)".to_string());
        verdict = EmailSecurityVerdict::Fail;
    } else {
        let bits = p.and_then(rsa_key_bits_from_base64);
        if let Some(b) = bits {
            if b < 1024 {
                notes.push(format!(
                    "DKIM key is {b} bits; 1024-bit is the minimum and 2048-bit is recommended"
                ));
                verdict = EmailSecurityVerdict::Fail;
            } else if b < 2048 {
                notes.push(format!(
                    "DKIM key is {b} bits; consider upgrading to 2048-bit"
                ));
                verdict = verdict.max(EmailSecurityVerdict::Warn);
            } else {
                notes.push(format!("DKIM key is {b} bits"));
            }
        } else {
            notes.push("Could not determine DKIM key size from p= value".to_string());
        }
    }

    DkimSelectorReportDto {
        selector: selector.to_string(),
        found: true,
        record: Some(record.clone()),
        key_present,
        key_bits_approx: if key_present && !revoked {
            p.and_then(rsa_key_bits_from_base64)
        } else {
            None
        },
        revoked,
        verdict,
        notes,
    }
}

/// Decode a DKIM `p=` value (base64-encoded RSA SubjectPublicKeyInfo DER) and
/// return the RSA modulus size in bits.
fn rsa_key_bits_from_base64(p: &str) -> Option<usize> {
    let clean: String = p.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.is_empty() {
        return None;
    }
    let der = STANDARD.decode(clean).ok()?;
    rsa_key_bits_from_spki_der(&der)
}

fn rsa_key_bits_from_spki_der(data: &[u8]) -> Option<usize> {
    let mut i = 0;
    read_tag(data, &mut i, 0x30)?;
    let outer_len = read_der_length(data, &mut i)?;
    let _outer_end = i.checked_add(outer_len)?;

    // AlgorithmIdentifier
    read_tag(data, &mut i, 0x30)?;
    let alg_len = read_der_length(data, &mut i)?;
    i = i.checked_add(alg_len)?;

    // subjectPublicKey BIT STRING
    read_tag(data, &mut i, 0x03)?;
    let bit_len = read_der_length(data, &mut i)?;
    let unused = *data.get(i)?;
    if unused != 0 {
        return None;
    }
    i += 1;
    let rsa_der = data.get(i..i.checked_add(bit_len.checked_sub(1)?)?)?;

    // RSAPublicKey SEQUENCE { modulus INTEGER, publicExponent INTEGER }
    let mut j = 0;
    read_tag(rsa_der, &mut j, 0x30)?;
    let _rsa_len = read_der_length(rsa_der, &mut j)?;

    read_tag(rsa_der, &mut j, 0x02)?;
    let mod_len = read_der_length(rsa_der, &mut j)?;
    let modulus = rsa_der.get(j..j.checked_add(mod_len)?)?;

    // Drop leading zero bytes that were added to keep the integer positive.
    let leading_zeros = modulus.iter().take_while(|&&b| b == 0).count();
    let byte_len = mod_len.saturating_sub(leading_zeros);
    byte_len.checked_mul(8)
}

fn read_tag(data: &[u8], idx: &mut usize, expected: u8) -> Option<()> {
    let tag = *data.get(*idx)?;
    if tag != expected {
        return None;
    }
    *idx += 1;
    Some(())
}

fn read_der_length(data: &[u8], idx: &mut usize) -> Option<usize> {
    let b = *data.get(*idx)?;
    *idx += 1;
    if b & 0x80 == 0 {
        return Some(b as usize);
    }
    let num_bytes = (b & 0x7f) as usize;
    if num_bytes == 0 || num_bytes > 4 {
        // Indefinite form or unreasonably large length not supported.
        return None;
    }
    let mut len = 0usize;
    for _ in 0..num_bytes {
        len = (len << 8) | (*data.get(*idx)? as usize);
        *idx += 1;
    }
    Some(len)
}

// -----------------------------------------------------------------------------
// DMARC
// -----------------------------------------------------------------------------

async fn evaluate_dmarc(endpoint: &ResolverEndpointDto, domain: &str) -> DmarcReportDto {
    let mut notes = Vec::new();
    let mut verdict = EmailSecurityVerdict::Pass;

    let name = format!("_dmarc.{}", domain);
    let records = match query_txt(endpoint, &name).await {
        Ok(r) => r,
        Err(e) => {
            return DmarcReportDto {
                verdict: EmailSecurityVerdict::Fail,
                found: false,
                record: None,
                policy: None,
                subdomain_policy: None,
                pct: None,
                reporting_address: None,
                notes: vec![format!("TXT query failed: {e}")],
            }
        }
    };

    let record = match records.into_iter().next() {
        Some(r) => r,
        None => {
            return DmarcReportDto {
                verdict: EmailSecurityVerdict::Fail,
                found: false,
                record: None,
                policy: None,
                subdomain_policy: None,
                pct: None,
                reporting_address: None,
                notes: vec!["No DMARC record found".to_string()],
            }
        }
    };

    if !record.to_lowercase().starts_with("v=dmarc1") {
        notes.push("DMARC record does not start with 'v=DMARC1'".to_string());
        verdict = EmailSecurityVerdict::Fail;
    }

    let tags = parse_tag_value(&record);
    let policy = tags.get("p").cloned();
    let subdomain_policy = tags.get("sp").cloned();
    let pct = tags.get("pct").and_then(|v| v.parse::<u8>().ok());
    let reporting_address = tags.get("rua").cloned();

    match policy.as_deref() {
        Some("reject") => notes.push("DMARC policy is 'reject'; strongest protection".to_string()),
        Some("quarantine") => {
            notes.push("DMARC policy is 'quarantine'; acceptable, consider 'reject'".to_string());
            verdict = verdict.max(EmailSecurityVerdict::Warn);
        }
        Some("none") => {
            notes
                .push("DMARC policy is 'none'; receivers only monitor, no enforcement".to_string());
            verdict = EmailSecurityVerdict::Warn;
        }
        Some(other) => {
            notes.push(format!("Unknown DMARC policy '{other}'"));
            verdict = EmailSecurityVerdict::Fail;
        }
        None => {
            notes.push("DMARC record is missing required 'p' tag".to_string());
            verdict = EmailSecurityVerdict::Fail;
        }
    }

    if reporting_address.is_none() {
        notes.push(
            "No 'rua' reporting address configured; you will not receive DMARC reports".to_string(),
        );
        verdict = verdict.max(EmailSecurityVerdict::Warn);
    }

    if let Some(pct) = pct {
        if pct < 100 {
            notes.push(format!(
                "DMARC 'pct' is {pct}%; policy is only applied to a subset of messages"
            ));
            verdict = verdict.max(EmailSecurityVerdict::Warn);
        }
    }

    DmarcReportDto {
        verdict,
        found: true,
        record: Some(record),
        policy,
        subdomain_policy,
        pct,
        reporting_address,
        notes,
    }
}

// -----------------------------------------------------------------------------
// Shared tag=value parser
// -----------------------------------------------------------------------------

fn parse_tag_value(record: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    // Strip the version token.
    for token in record
        .split(';')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        if let Some((key, value)) = token.split_once('=') {
            map.insert(key.trim().to_lowercase(), value.trim().to_string());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsa_key_bits_from_valid_2048_spki_base64() {
        let der = build_test_rsa_spki(256);
        let b64 = STANDARD.encode(&der);
        assert_eq!(rsa_key_bits_from_base64(&b64), Some(2048));
    }

    #[test]
    fn rsa_key_bits_from_valid_1024_spki_base64() {
        let der = build_test_rsa_spki(128);
        let b64 = STANDARD.encode(&der);
        assert_eq!(rsa_key_bits_from_base64(&b64), Some(1024));
    }

    #[test]
    fn rsa_key_bits_from_invalid_base64_is_none() {
        assert_eq!(rsa_key_bits_from_base64("not-valid!!!"), None);
    }

    #[test]
    fn rsa_key_bits_from_empty_p_is_none() {
        assert_eq!(rsa_key_bits_from_base64(""), None);
    }

    /// Build a minimal, valid RSA SubjectPublicKeyInfo DER with a modulus of `modulus_bytes`.
    fn build_test_rsa_spki(modulus_bytes: usize) -> Vec<u8> {
        // RSA encryption OID 1.2.840.113549.1.1.1 + NULL parameters.
        let algorithm_identifier = vec![
            0x30, 0x0d, 0x06, 0x09, 0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x01, 0x05,
            0x00,
        ];

        // Modulus with high bit set; prefix 0x00 to keep integer positive.
        let modulus = std::iter::once(0x00)
            .chain(std::iter::repeat_n(0xab, modulus_bytes))
            .collect::<Vec<_>>();
        let modulus_int = wrap_integer(&modulus);

        // Public exponent 65537.
        let exponent = wrap_integer(&[0x01, 0x00, 0x01]);

        let rsa_public_key = wrap_sequence([modulus_int, exponent].concat());
        let subject_public_key = wrap_bit_string(&rsa_public_key);

        wrap_sequence([algorithm_identifier, subject_public_key].concat())
    }

    fn wrap_sequence(content: Vec<u8>) -> Vec<u8> {
        let mut out = vec![0x30];
        out.extend(der_length(content.len()));
        out.extend(content);
        out
    }

    fn wrap_integer(content: &[u8]) -> Vec<u8> {
        let mut out = vec![0x02];
        out.extend(der_length(content.len()));
        out.extend(content);
        out
    }

    fn wrap_bit_string(content: &[u8]) -> Vec<u8> {
        let mut out = vec![0x03];
        out.extend(der_length(content.len() + 1));
        out.push(0x00); // unused bits
        out.extend(content);
        out
    }

    fn der_length(len: usize) -> Vec<u8> {
        if len < 0x80 {
            vec![len as u8]
        } else if len <= 0xff {
            vec![0x81, len as u8]
        } else if len <= 0xffff {
            vec![0x82, (len >> 8) as u8, len as u8]
        } else if len <= 0xffffff {
            vec![0x83, (len >> 16) as u8, (len >> 8) as u8, len as u8]
        } else {
            vec![
                0x84,
                (len >> 24) as u8,
                (len >> 16) as u8,
                (len >> 8) as u8,
                len as u8,
            ]
        }
    }
}
