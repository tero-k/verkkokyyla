use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use hickory_proto::rr::Name;
use hickory_resolver::config::{
    ConnectionConfig, NameServerConfig, ResolverConfig, ResolverOpts,
};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::{Resolver, TokioResolver};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::dns::client::{DnsProtocol, ResolverEndpointDto};
use crate::dns::error::DnsError;
use crate::dns::query::{query_once, QueryOpts, RecordTypeSpec};

const DNSSEC_EXPERIMENTAL_CAVEAT: &str =
    "DNSSEC validation is experimental in Hickory; results are advisory.";

const DNSKEY_RECORD_TYPE: u16 = 48;
const DS_RECORD_TYPE: u16 = 43;
const RRSIG_RECORD_TYPE: u16 = 46;

/// Diagnostic report describing DNSSEC signals for a domain against a resolver.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DnssecReportDto {
    pub has_dnskey: bool,
    pub has_ds: bool,
    pub has_rrsig: bool,
    pub ad_flag: bool,
    pub validates: bool,
    pub bogus_domain_rejected: bool,
    pub notes: Vec<String>,
}

impl fmt::Display for DnssecReportDto {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "DnssecReport {{ dnskey={}, ds={}, rrsig={}, ad={}, validates={}, bogus_rejected={}, notes={} }}",
            self.has_dnskey,
            self.has_ds,
            self.has_rrsig,
            self.ad_flag,
            self.validates,
            self.bogus_domain_rejected,
            self.notes.len()
        )
    }
}

/// Build a DNSSEC diagnostic report for `domain` using `endpoint`.
///
/// The report does **not** infer validation from the upstream AD bit. Instead it runs an
/// explicit validating resolver pass with Hickory's built-in trust anchors and records whether
/// that pass produces answers or a validation failure. The AD bit is captured separately as
/// an observational signal.
pub async fn dnssec_report(
    endpoint: &ResolverEndpointDto,
    domain: &str,
) -> Result<DnssecReportDto, DnsError> {
    let mut notes: Vec<String> = Vec::new();
    let domain = normalize_domain(domain);

    let has_dnskey =
        presence_query(endpoint, &domain, RecordTypeSpec::Other(DNSKEY_RECORD_TYPE), "DNSKEY", &mut notes)
            .await;
    let has_ds =
        presence_query(endpoint, &domain, RecordTypeSpec::Other(DS_RECORD_TYPE), "DS", &mut notes)
            .await;
    let has_rrsig =
        presence_query(endpoint, &domain, RecordTypeSpec::Other(RRSIG_RECORD_TYPE), "RRSIG", &mut notes)
            .await;

    let ad_flag = capture_ad_flag(endpoint, &domain).await;

    let validates = validating_lookup(endpoint, &domain, &mut notes).await?;
    let bogus_domain_rejected = rejects_bogus_domain(endpoint).await?;

    notes.push(DNSSEC_EXPERIMENTAL_CAVEAT.to_owned());

    Ok(DnssecReportDto {
        has_dnskey,
        has_ds,
        has_rrsig,
        ad_flag,
        validates,
        bogus_domain_rejected,
        notes,
    })
}

fn normalize_domain(domain: &str) -> String {
    let trimmed = domain.trim();
    if trimmed.is_empty() {
        return ".".to_owned();
    }
    if trimmed.ends_with('.') {
        trimmed.to_owned()
    } else {
        format!("{}.", trimmed)
    }
}

async fn presence_query(
    endpoint: &ResolverEndpointDto,
    domain: &str,
    rtype: RecordTypeSpec,
    label: &str,
    notes: &mut Vec<String>,
) -> bool {
    let mut opts = QueryOpts::default();
    opts.dnssec_ok = true;

    match query_once(endpoint, domain, rtype, opts).await {
        Ok(result) => !result.answers.is_empty(),
        Err(err) => {
            notes.push(format!("{label} query failed: {err}"));
            false
        }
    }
}

async fn capture_ad_flag(endpoint: &ResolverEndpointDto, domain: &str) -> bool {
    let mut opts = QueryOpts::default();
    opts.dnssec_ok = true;

    match query_once(endpoint, domain, RecordTypeSpec::A, opts).await {
        Ok(result) => result.ad_flag,
        Err(_) => false,
    }
}

async fn validating_lookup(
    endpoint: &ResolverEndpointDto,
    domain: &str,
    notes: &mut Vec<String>,
) -> Result<bool, DnsError> {
    let resolver = make_validating_resolver(endpoint)?;
    let name = Name::from_str(domain)
        .map_err(|source| DnsError::InvalidInput(format!("invalid domain {domain}: {source}")))?;

    match resolver.ipv4_lookup(name).await {
        Ok(_) => Ok(true),
        Err(err) => {
            notes.push(format!("validation did not succeed: {err}"));
            Ok(false)
        }
    }
}

async fn rejects_bogus_domain(endpoint: &ResolverEndpointDto) -> Result<bool, DnsError> {
    let resolver = make_validating_resolver(endpoint)?;
    let name = Name::from_str("dnssec-failed.org.").map_err(|source| {
        DnsError::InvalidInput(format!("invalid bogus probe name: {source}"))
    })?;

    match resolver.ipv4_lookup(name).await {
        Ok(_) => Ok(false),
        Err(err) => {
            let msg = err.to_string().to_lowercase();
            Ok(msg.contains("servfail")
                || msg.contains("bogus")
                || msg.contains("validation")
                || msg.contains("proof"))
        }
    }
}

fn make_validating_resolver(endpoint: &ResolverEndpointDto) -> Result<TokioResolver, DnsError> {
    let config = resolver_config(endpoint)?;
    let mut opts = ResolverOpts::default();
    opts.validate = true;
    opts.cache_size = 0;
    opts.attempts = 1;
    opts.timeout = Duration::from_secs(5);

    Resolver::builder_with_config(config, TokioRuntimeProvider::default())
        .with_options(opts)
        .build()
        .map_err(|source| DnsError::Transport(source.to_string()))
}

fn resolver_config(endpoint: &ResolverEndpointDto) -> Result<ResolverConfig, DnsError> {
    let name_server = match endpoint.protocol {
        DnsProtocol::Udp | DnsProtocol::Tcp | DnsProtocol::Tls | DnsProtocol::Quic => {
            let address = parse_socket_endpoint(&endpoint.address, endpoint.protocol)?;
            socket_name_server(endpoint.protocol, address)
        }
        DnsProtocol::Https | DnsProtocol::H3 => {
            let parsed = parse_url_endpoint(&endpoint.address, endpoint.protocol)?;
            let ip = parsed.host.parse::<IpAddr>().map_err(|_| {
                DnsError::InvalidEndpoint(format!(
                    "{} URL host must be an IP address: {}",
                    endpoint.protocol, endpoint.address
                ))
            })?;
            url_name_server(endpoint.protocol, ip, parsed)
        }
    };

    Ok(ResolverConfig::from_parts(None, Vec::new(), vec![name_server]))
}

struct UrlEndpoint {
    host: String,
    port: u16,
    path: String,
}

fn socket_name_server(protocol: DnsProtocol, address: SocketAddr) -> NameServerConfig {
    let mut connection = match protocol {
        DnsProtocol::Udp => ConnectionConfig::udp(),
        DnsProtocol::Tcp => ConnectionConfig::tcp(),
        DnsProtocol::Tls => ConnectionConfig::tls(Arc::from(address.ip().to_string())),
        DnsProtocol::Quic => ConnectionConfig::quic(Arc::from(address.ip().to_string())),
        DnsProtocol::Https | DnsProtocol::H3 => {
            unreachable!("URL protocols are handled separately")
        }
    };
    connection.port = address.port();
    NameServerConfig::new(address.ip(), true, vec![connection])
}

fn url_name_server(protocol: DnsProtocol, ip: IpAddr, parsed: UrlEndpoint) -> NameServerConfig {
    let path = Arc::<str>::from(parsed.path);
    let server_name = Arc::<str>::from(parsed.host);
    let mut connection = match protocol {
        DnsProtocol::Https => ConnectionConfig::https(server_name, Some(path)),
        DnsProtocol::H3 => ConnectionConfig::h3(server_name, Some(path)),
        DnsProtocol::Udp | DnsProtocol::Tcp | DnsProtocol::Tls | DnsProtocol::Quic => {
            unreachable!("socket protocols are handled separately")
        }
    };
    connection.port = parsed.port;
    NameServerConfig::new(ip, true, vec![connection])
}

fn parse_socket_endpoint(address: &str, protocol: DnsProtocol) -> Result<SocketAddr, DnsError> {
    if let Ok(socket_addr) = address.parse::<SocketAddr>() {
        return Ok(socket_addr);
    }

    let ip = address.parse::<IpAddr>().map_err(|_| {
        DnsError::InvalidEndpoint(format!(
            "{} endpoint must be an IP address with optional port: {}",
            protocol, address
        ))
    })?;
    Ok(SocketAddr::new(ip, protocol.default_port()))
}

fn parse_url_endpoint(address: &str, protocol: DnsProtocol) -> Result<UrlEndpoint, DnsError> {
    let url = Url::parse(address).map_err(|source| {
        DnsError::InvalidEndpoint(format!("{} endpoint URL is invalid: {source}", protocol))
    })?;
    if url.scheme() != "https" {
        return Err(DnsError::InvalidEndpoint(format!(
            "{} endpoint URL must use https: {}",
            protocol, address
        )));
    }

    let host = url.host_str().ok_or_else(|| {
        DnsError::InvalidEndpoint(format!("{} endpoint URL must include a host", protocol))
    })?;
    let path = match url.query() {
        Some(query) => format!("{}?{}", url.path(), query),
        None => url.path().to_owned(),
    };

    Ok(UrlEndpoint {
        host: host.to_owned(),
        port: url.port().unwrap_or_else(|| protocol.default_port()),
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_domain_adds_trailing_dot() {
        assert_eq!(normalize_domain("mock.test"), "mock.test.");
    }

    #[test]
    fn normalize_domain_preserves_existing_trailing_dot() {
        assert_eq!(normalize_domain("mock.test."), "mock.test.");
    }

    #[test]
    fn dnssec_experimental_caveat_is_constant() {
        assert!(DNSSEC_EXPERIMENTAL_CAVEAT.contains("experimental"));
    }
}
