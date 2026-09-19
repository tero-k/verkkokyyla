use std::net::IpAddr;

use rand::distributions::{Distribution, Uniform};
use serde::{Deserialize, Serialize};

use crate::dns::client::ResolverEndpointDto;
use crate::dns::error::DnsError;
use crate::dns::query::{is_nxdomain, query_once, QueryOpts, RecordTypeSpec};

const MAX_CNAME_HOPS: usize = 16;
const WILDCARD_PROBE_LEN: usize = 12;

/// Result of a reverse DNS lookup with forward-confirmed rDNS support.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReverseLookupDto {
    pub ip: String,
    pub ptr_name: String,
    pub ptr_target: Option<String>,
    pub forward_names: Vec<String>,
    pub fcrdns: bool,
    pub rcode: String,
}

/// Result of an iterative CNAME chain walk.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CnameChainDto {
    pub name: String,
    pub chain: Vec<String>,
    pub final_name: String,
    pub loop_detected: bool,
    pub rcode: String,
}

/// Result of a wildcard probe under a zone.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WildcardCheckDto {
    pub zone: String,
    pub probe: String,
    pub wildcard: bool,
    pub rcode: String,
    pub answers_count: usize,
}

/// Per-type existence matrix for a DNS name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NameExistenceDto {
    pub name: String,
    pub exists: bool,
    pub a: bool,
    pub aaaa: bool,
    pub mx: bool,
    pub txt: bool,
    pub cname: bool,
    pub rcode: String,
}

/// Build a lowercase DNS-safe random label of the requested length.
fn random_label(len: usize) -> String {
    let mut rng = rand::thread_rng();
    let dist = Uniform::new_inclusive(b'a', b'z');
    dist.sample_iter(&mut rng)
        .take(len)
        .map(|b| b as char)
        .collect()
}

/// Return an absolute, lowercase canonical name with a trailing dot.
pub fn normalize_name(name: &str) -> String {
    let name = name.trim().to_lowercase();
    if name.ends_with('.') {
        name
    } else {
        format!("{name}.")
    }
}

/// Build the reverse-lookup name for an IPv4 or IPv6 address.
fn reverse_name(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            format!(
                "{}.{}.{}.{}.in-addr.arpa.",
                octets[3], octets[2], octets[1], octets[0]
            )
        }
        IpAddr::V6(v6) => {
            const NIBBLES: &[u8; 16] = b"0123456789abcdef";
            let octets = v6.octets();
            let mut out = String::with_capacity(64);
            for byte in octets.iter().rev() {
                out.push(NIBBLES[(byte & 0x0f) as usize] as char);
                out.push('.');
                out.push(NIBBLES[(byte >> 4) as usize] as char);
                out.push('.');
            }
            out.push_str("ip6.arpa.");
            out
        }
    }
}

/// Perform a PTR lookup for `ip` and forward-confirm the returned name(s).
///
/// `fcrdns` is set to `true` only if a forward lookup (A or AAAA) of one of the
/// PTR targets yields the original IP address. A missing PTR or mismatching
/// forward answers is reported, not an error.
pub async fn reverse_lookup(
    endpoint: &ResolverEndpointDto,
    ip: IpAddr,
) -> Result<ReverseLookupDto, DnsError> {
    let ptr_name = reverse_name(ip);
    let result = query_once(
        endpoint,
        &ptr_name,
        RecordTypeSpec::Ptr,
        QueryOpts::default(),
    )
    .await?;

    let mut forward_names = Vec::new();
    let mut fcrdns = false;
    let ptr_target = result
        .answers
        .first()
        .map(|answer| normalize_name(&answer.data));

    if result.rcode == "noerror" {
        for answer in &result.answers {
            let target = normalize_name(&answer.data);
            if !forward_names.contains(&target) {
                forward_names.push(target.clone());
            }

            for rtype in [RecordTypeSpec::A, RecordTypeSpec::Aaaa] {
                let forward = query_once(endpoint, &target, rtype, QueryOpts::default()).await?;
                for fwd_answer in &forward.answers {
                    if let Ok(addr) = fwd_answer.data.parse::<IpAddr>() {
                        if addr == ip {
                            fcrdns = true;
                        }
                    }
                }
            }
        }
    }

    Ok(ReverseLookupDto {
        ip: ip.to_string(),
        ptr_name,
        ptr_target,
        forward_names,
        fcrdns,
        rcode: result.rcode,
    })
}

/// Walk the CNAME chain starting at `name`, following at most [`MAX_CNAME_HOPS`]
/// hops and detecting loops.
///
/// The returned chain always includes the starting name. `loop_detected` is
/// `true` when the chain would revisit a previously seen name before the hop
/// limit is reached.
pub async fn cname_chain(
    endpoint: &ResolverEndpointDto,
    name: &str,
) -> Result<CnameChainDto, DnsError> {
    let start = normalize_name(name);
    let mut chain = vec![start.clone()];
    let mut current = start.clone();
    let mut loop_detected = false;
    let mut last_rcode = String::new();

    for _ in 0..MAX_CNAME_HOPS {
        let result = query_once(
            endpoint,
            &current,
            RecordTypeSpec::Cname,
            QueryOpts::default(),
        )
        .await?;
        last_rcode = result.rcode.clone();

        if result.rcode != "noerror" || result.answers.is_empty() {
            break;
        }

        let next = normalize_name(&result.answers[0].data);
        if chain.contains(&next) {
            loop_detected = true;
            break;
        }

        chain.push(next.clone());
        current = next;
    }

    Ok(CnameChainDto {
        name: name.to_string(),
        final_name: current,
        chain,
        loop_detected,
        rcode: last_rcode,
    })
}

/// Probe a random 12-character label under `zone` to test for a wildcard record.
///
/// A positive answer is interpreted as a wildcard being present. An NXDOMAIN
/// response means no wildcard was found for that probe, but it is *not* proof of
/// authoritative absence for the whole zone (another probe label could match).
pub async fn wildcard_check(
    endpoint: &ResolverEndpointDto,
    zone: &str,
) -> Result<WildcardCheckDto, DnsError> {
    let zone = normalize_name(zone).trim_end_matches('.').to_string();
    let label = random_label(WILDCARD_PROBE_LEN);
    let probe = format!("{label}.{zone}.");

    let result = query_once(endpoint, &probe, RecordTypeSpec::A, QueryOpts::default()).await?;

    let wildcard = result.rcode == "noerror" && !result.answers.is_empty();

    Ok(WildcardCheckDto {
        zone,
        probe,
        wildcard,
        rcode: result.rcode,
        answers_count: result.answers.len(),
    })
}

/// Query a matrix of common record types for `name` and summarize existence.
///
/// A name is considered to exist if any of the queried types returns something
/// other than NXDOMAIN (NOERROR with answers, or NODATA with an SOA authority).
pub async fn classify_name(
    endpoint: &ResolverEndpointDto,
    name: &str,
) -> Result<NameExistenceDto, DnsError> {
    let name = normalize_name(name);
    let types = [
        RecordTypeSpec::A,
        RecordTypeSpec::Aaaa,
        RecordTypeSpec::Mx,
        RecordTypeSpec::Txt,
        RecordTypeSpec::Cname,
    ];

    let mut exists = false;
    let mut last_rcode = String::new();
    let mut dto = NameExistenceDto {
        name: name.trim_end_matches('.').to_string(),
        exists: false,
        a: false,
        aaaa: false,
        mx: false,
        txt: false,
        cname: false,
        rcode: String::new(),
    };

    for rtype in types {
        let result = query_once(endpoint, &name, rtype, QueryOpts::default()).await?;
        last_rcode = result.rcode.clone();
        let present = result.rcode == "noerror" && !result.answers.is_empty();
        match rtype {
            RecordTypeSpec::A => dto.a = present,
            RecordTypeSpec::Aaaa => dto.aaaa = present,
            RecordTypeSpec::Mx => dto.mx = present,
            RecordTypeSpec::Txt => dto.txt = present,
            RecordTypeSpec::Cname => dto.cname = present,
            _ => {}
        }
        if !is_nxdomain(&result) {
            exists = true;
        }
    }

    dto.exists = exists;
    dto.rcode = last_rcode;
    Ok(dto)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_name_v4() {
        assert_eq!(
            reverse_name(IpAddr::V4([192, 0, 2, 10].into())),
            "10.2.0.192.in-addr.arpa."
        );
    }

    #[test]
    fn reverse_name_v6() {
        let ip = "2001:db8::1".parse::<IpAddr>().unwrap();
        assert_eq!(
            reverse_name(ip),
            "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa."
        );
    }

    #[test]
    fn normalize_name_adds_trailing_dot_and_lowercases() {
        assert_eq!(normalize_name("WWW.Example.COM"), "www.example.com.");
        assert_eq!(normalize_name("www.example.com."), "www.example.com.");
    }

    #[test]
    fn random_label_is_lowercase_alphanumeric() {
        let label = random_label(12);
        assert_eq!(label.len(), 12);
        assert!(label.chars().all(|c| c.is_ascii_alphanumeric()));
        assert!(label.chars().all(|c| c.is_ascii_lowercase()));
    }
}
