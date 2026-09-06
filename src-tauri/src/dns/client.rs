use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use hickory_resolver::config::{ConnectionConfig, NameServerConfig, ResolverConfig, ResolverOpts};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::{Resolver, TokioResolver};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::dns::error::DnsError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DnsProtocol {
    Udp,
    Tcp,
    Tls,
    Https,
    Quic,
    H3,
}

impl DnsProtocol {
    pub(crate) const fn default_port(self) -> u16 {
        match self {
            Self::Udp | Self::Tcp => 53,
            Self::Tls | Self::Quic => 853,
            Self::Https | Self::H3 => 443,
        }
    }
}

impl fmt::Display for DnsProtocol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Udp => "udp",
            Self::Tcp => "tcp",
            Self::Tls => "tls",
            Self::Https => "https",
            Self::Quic => "quic",
            Self::H3 => "h3",
        })
    }
}

impl FromStr for DnsProtocol {
    type Err = DnsError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Ok(match input {
            "udp" => Self::Udp,
            "tcp" => Self::Tcp,
            "tls" => Self::Tls,
            "https" => Self::Https,
            "quic" => Self::Quic,
            "h3" => Self::H3,
            _ => return Err(DnsError::UnsupportedTransport(input.to_owned())),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolverEndpointDto {
    pub name: String,
    pub protocol: DnsProtocol,
    pub address: String,
}

struct UrlEndpoint {
    host: String,
    port: u16,
    path: String,
}

pub fn make_resolver(endpoint: &ResolverEndpointDto) -> Result<TokioResolver, DnsError> {
    let config = resolver_config(endpoint)?;
    let mut opts = ResolverOpts::default();
    opts.cache_size = 0;
    opts.attempts = 2;
    opts.timeout = Duration::from_secs(2);
    opts.validate = false;
    opts.try_tcp_on_error = true;

    Resolver::builder_with_config(config, TokioRuntimeProvider::default())
        .with_options(opts)
        .build()
        .map_err(|source| DnsError::Transport(source.to_string()))
}

pub fn detect_system_resolver() -> Vec<ResolverEndpointDto> {
    let Ok((config, _opts)) = hickory_resolver::system_conf::read_system_conf() else {
        return Vec::new();
    };

    config
        .name_servers()
        .iter()
        .map(|name_server| ResolverEndpointDto {
            name: "System".to_owned(),
            protocol: DnsProtocol::Udp,
            address: SocketAddr::new(name_server.ip, DnsProtocol::Udp.default_port()).to_string(),
        })
        .collect()
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

    Ok(ResolverConfig::from_parts(
        None,
        Vec::new(),
        vec![name_server],
    ))
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

    fn endpoint(address: &str) -> ResolverEndpointDto {
        ResolverEndpointDto {
            name: "test".into(),
            protocol: DnsProtocol::Udp,
            address: address.into(),
        }
    }

    #[test]
    fn protocol_display_round_trips_from_str() {
        for protocol in [
            DnsProtocol::Udp,
            DnsProtocol::Tcp,
            DnsProtocol::Tls,
            DnsProtocol::Https,
            DnsProtocol::Quic,
            DnsProtocol::H3,
        ] {
            assert!(matches!(
                protocol.to_string().parse::<DnsProtocol>(),
                Ok(parsed) if parsed == protocol
            ));
        }
    }

    #[test]
    fn protocol_rejects_unknown_name() {
        assert!(matches!(
            "dnscrypt".parse::<DnsProtocol>(),
            Err(DnsError::UnsupportedTransport(_))
        ));
    }

    #[test]
    fn socket_endpoint_defaults_ports_when_missing() {
        for (protocol, port) in [
            (DnsProtocol::Udp, 53),
            (DnsProtocol::Tcp, 53),
            (DnsProtocol::Tls, 853),
            (DnsProtocol::Quic, 853),
        ] {
            assert_eq!(
                parse_socket_endpoint("1.1.1.1", protocol).unwrap().port(),
                port
            );
        }
        assert_eq!(
            parse_socket_endpoint("1.1.1.1:5353", DnsProtocol::Udp)
                .unwrap()
                .port(),
            5353
        );
    }

    #[test]
    fn url_endpoint_parses_https_details() {
        let parsed =
            parse_url_endpoint("https://cloudflare-dns.com/dns-query", DnsProtocol::Https).unwrap();
        assert_eq!(parsed.host, "cloudflare-dns.com");
        assert_eq!(parsed.path, "/dns-query");
        assert_eq!(parsed.port, 443);
    }

    #[test]
    fn make_resolver_accepts_well_formed_udp_endpoint() {
        assert!(make_resolver(&endpoint("1.1.1.1")).is_ok());
    }

    #[test]
    fn make_resolver_rejects_malformed_udp_endpoint_as_invalid_endpoint() {
        let error = make_resolver(&endpoint("not-an-ip")).unwrap_err();
        assert_eq!(error.kind(), "invalid-endpoint");
    }
}
