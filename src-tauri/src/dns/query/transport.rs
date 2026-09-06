use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;

use hickory_net::runtime::TokioRuntimeProvider;
use hickory_net::tcp::TcpClientStream;
use hickory_net::tls::tls_client_connect_with_bind_addr;
use hickory_net::udp::UdpClientStream;
use hickory_net::xfer::{DnsExchange, DnsHandle, DnsMultiplexer, FirstAnswer};
use hickory_proto::op::DnsRequest;
use hickory_proto::op::{DnsRequestOptions, DnsResponse, Edns, Message, Metadata, Query, ResponseCode};
use hickory_proto::rr::Name;
use rustls::pki_types::{CertificateDer, ServerName};
use tokio::time::Instant;

use crate::dns::client::{DnsProtocol, ResolverEndpointDto};
use crate::dns::error::DnsError;
use crate::dns::query::types::{QueryOpts, QueryResultDto, RecordTypeSpec, record_type_name};

pub(super) fn build_request(
    name: &str,
    rtype: RecordTypeSpec,
    opts: &QueryOpts,
) -> Result<DnsRequest, DnsError> {
    let mut metadata = Metadata::new(0, hickory_proto::op::MessageType::Query, hickory_proto::op::OpCode::Query);
    metadata.recursion_desired = opts.rd;

    let mut message = Message::query();
    message.metadata = metadata;

    let query_name = Name::from_str(name).map_err(|source| DnsError::InvalidInput(format!("invalid qname {name}: {source}")))?;
    message.add_query(Query::query(query_name, crate::dns::query::types::to_hickory_record_type(rtype)));

    if opts.edns_size.is_some() || opts.dnssec_ok {
        let mut edns = Edns::new();
        if let Some(size) = opts.edns_size {
            edns.set_max_payload(size);
        }
        edns.set_version(opts.edns_version);
        edns.set_dnssec_ok(opts.dnssec_ok);
        message.set_edns(edns);
    }

    let mut request_opts = DnsRequestOptions::default();
    request_opts.recursion_desired = opts.rd;
    request_opts.use_edns = opts.edns_size.is_some() || opts.dnssec_ok;
    request_opts.edns_payload_len = opts.edns_size.unwrap_or(0);
    request_opts.edns_set_dnssec_ok = opts.dnssec_ok;

    Ok(DnsRequest::new(message, request_opts))
}

pub(super) fn parse_socket_addr(
    endpoint: &ResolverEndpointDto,
    default_port: u16,
) -> Result<SocketAddr, DnsError> {
    if let Ok(addr) = endpoint.address.parse::<SocketAddr>() {
        return Ok(addr);
    }

    let ip = endpoint
        .address
        .parse::<IpAddr>()
        .map_err(|_| DnsError::InvalidEndpoint(format!(
            "{} endpoint must be an IP address with optional port: {}",
            endpoint.protocol, endpoint.address
        )))?;
    Ok(SocketAddr::new(ip, default_port))
}

pub(super) async fn exchange(
    endpoint: &ResolverEndpointDto,
    request: DnsRequest,
    opts: &QueryOpts,
) -> Result<DnsResponse, DnsError> {
    let start = Instant::now();
    let response = match endpoint.protocol {
        DnsProtocol::Udp => udp_exchange(endpoint, request, opts).await,
        DnsProtocol::Tcp => tcp_exchange(endpoint, request, opts).await,
        DnsProtocol::Tls => tls_exchange(endpoint, request, opts).await,
        DnsProtocol::Https | DnsProtocol::Quic | DnsProtocol::H3 => {
            Err(DnsError::UnsupportedTransport(endpoint.protocol.to_string()))
        }
    };
    let _elapsed = start.elapsed();
    response
}

async fn udp_exchange(
    endpoint: &ResolverEndpointDto,
    request: DnsRequest,
    opts: &QueryOpts,
) -> Result<DnsResponse, DnsError> {
    let addr = parse_socket_addr(endpoint, 53)?;
    let stream = UdpClientStream::builder(addr, TokioRuntimeProvider::default())
        .with_timeout(Some(opts.timeout))
        .build();
    let (exchange, bg) = DnsExchange::<TokioRuntimeProvider>::from_stream(stream);
    tokio::spawn(bg);

    let response_stream = exchange.send(request);
    let response = tokio::time::timeout(opts.timeout, response_stream.first_answer())
        .await
        .map_err(|_| DnsError::Timeout)?
        .map_err(net_to_dns_error)?;
    Ok(response)
}

async fn tcp_exchange(
    endpoint: &ResolverEndpointDto,
    request: DnsRequest,
    opts: &QueryOpts,
) -> Result<DnsResponse, DnsError> {
    let addr = parse_socket_addr(endpoint, 53)?;
    let (stream_future, handle) = TcpClientStream::new::<TokioRuntimeProvider>(
        addr,
        None,
        Some(opts.timeout),
        TokioRuntimeProvider::default(),
    );
    let stream = tokio::time::timeout(opts.timeout, stream_future)
        .await
        .map_err(|_| DnsError::Timeout)?
        .map_err(net_to_dns_error)?;

    let multiplexer = DnsMultiplexer::new(stream, handle).with_timeout(opts.timeout);
    let (exchange, bg) = DnsExchange::<TokioRuntimeProvider>::from_stream(multiplexer);
    tokio::spawn(bg);

    let response_stream = exchange.send(request);
    let response = tokio::time::timeout(opts.timeout, response_stream.first_answer())
        .await
        .map_err(|_| DnsError::Timeout)?
        .map_err(net_to_dns_error)?;
    Ok(response)
}

async fn tls_exchange(
    endpoint: &ResolverEndpointDto,
    request: DnsRequest,
    opts: &QueryOpts,
) -> Result<DnsResponse, DnsError> {
    let addr = parse_socket_addr(endpoint, 853)?;
    let client_config = tls_client_config(endpoint, addr, opts)?;
    let server_name = tls_server_name(endpoint, addr)?;

    let (stream_future, handle) = tls_client_connect_with_bind_addr::<TokioRuntimeProvider>(
        addr,
        None,
        server_name,
        Arc::new(client_config),
        TokioRuntimeProvider::default(),
    );

    let stream = tokio::time::timeout(opts.timeout, stream_future)
        .await
        .map_err(|_| DnsError::Timeout)?
        .map_err(net_to_dns_error)?;

    let multiplexer = DnsMultiplexer::new(stream, handle).with_timeout(opts.timeout);
    let (exchange, bg) = DnsExchange::<TokioRuntimeProvider>::from_stream(multiplexer);
    tokio::spawn(bg);

    let response_stream = exchange.send(request);
    let response = tokio::time::timeout(opts.timeout, response_stream.first_answer())
        .await
        .map_err(|_| DnsError::Timeout)?
        .map_err(net_to_dns_error)?;
    Ok(response)
}

fn tls_client_config(
    endpoint: &ResolverEndpointDto,
    _addr: SocketAddr,
    opts: &QueryOpts,
) -> Result<rustls::ClientConfig, DnsError> {
    if let Some(der) = &opts.pinned_root_cert {
        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(CertificateDer::from(der.clone()))
            .map_err(|source| DnsError::Transport(format!("invalid pinned cert: {source}")))?;
        Ok(rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|source| DnsError::Transport(format!("tls protocol error: {source}")))?
            .with_root_certificates(roots)
            .with_no_client_auth())
    } else {
        hickory_net::tls::client_config()
            .map_err(|source| DnsError::Transport(format!("tls config error for {endpoint:?}: {source}")))
    }
}

fn tls_server_name(endpoint: &ResolverEndpointDto, addr: SocketAddr) -> Result<ServerName<'static>, DnsError> {
    if let Ok(name) = ServerName::try_from(endpoint.name.clone()) {
        return Ok(name);
    }
    Ok(ServerName::IpAddress(addr.ip().into()))
}

pub(super) fn build_result(
    response: &DnsResponse,
    query_name: &str,
    rtype: RecordTypeSpec,
    transport: &str,
    latency_ms: u64,
) -> QueryResultDto {
    let metadata = &**response;
    let answers = metadata.answers.iter().map(record_to_answer).collect::<Vec<_>>();
    let authority_soa = metadata
        .authorities
        .iter()
        .any(|record| record.record_type() == hickory_proto::rr::RecordType::SOA);
    let additional_glue = metadata.additionals.iter().map(record_to_answer).collect::<Vec<_>>();
    let answers_empty = answers.is_empty();

    let header_rcode = u16::from(metadata.response_code) & 0x000F;
    let extended_rcode = metadata
        .edns
        .as_ref()
        .map(|e| e.rcode_high())
        .unwrap_or(0);
    let full_rcode = ((u16::from(extended_rcode)) << 4) | (header_rcode & 0x000F);
    let full_rcode_high = ((full_rcode >> 4) & 0x00FF) as u8;
    let full_rcode_low = (full_rcode & 0x000F) as u8;

    QueryResultDto {
        query_name: query_name.to_owned(),
        record_type: record_type_name(crate::dns::query::types::to_hickory_record_type(rtype)),
        rcode: format!("{:?}", ResponseCode::from(full_rcode_high, full_rcode_low)).to_lowercase(),
        header_rcode,
        extended_rcode,
        full_rcode,
        answers,
        authority_nodata: metadata.response_code == hickory_proto::op::ResponseCode::NoError
            && answers_empty
            && authority_soa,
        authority_soa,
        ad_flag: metadata.authentic_data,
        aa_flag: metadata.authoritative,
        ra_flag: metadata.recursion_available,
        truncated: metadata.truncation,
        edns_present: metadata.edns.is_some(),
        latency_ms,
        transport_used: transport.to_owned(),
        response_bytes: response.as_buffer().len(),
        additional_glue,
    }
}

fn record_to_answer(record: &hickory_proto::rr::Record) -> crate::dns::query::types::AnswerDto {
    crate::dns::query::types::AnswerDto {
        data: record.data.to_string(),
        ttl: record.ttl,
    }
}

fn net_to_dns_error(source: hickory_net::NetError) -> DnsError {
    use hickory_net::NetError;
    match source {
        NetError::Timeout => DnsError::Timeout,
        NetError::Io(_) => DnsError::Io(source.to_string()),
        NetError::Proto(_) => DnsError::Resolution(source.to_string()),
        _ => DnsError::Transport(source.to_string()),
    }
}
