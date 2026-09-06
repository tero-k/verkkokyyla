pub mod transport;
pub mod types;

pub use types::{
    is_nodata, is_nxdomain, AnswerDto, QueryOpts, QueryResultDto, RecordTypeSpec,
};

use hickory_proto::op::DnsResponse;
use tokio::time::Instant;

use crate::dns::client::ResolverEndpointDto;
use crate::dns::error::DnsError;
use crate::dns::query::transport::{build_request, build_result, exchange};

/// Perform a single low-level DNS query over the protocol specified by `endpoint`.
///
/// The request is built as a hickory-proto [`DnsRequest`] with one query, the RD bit set
/// according to `opts.rd`, and optional EDNS/DNSSEC-OK options. The response is timed
/// with [`tokio::time::Instant`] and turned into a [`QueryResultDto`].
pub async fn query_once(
    endpoint: &ResolverEndpointDto,
    name: &str,
    rtype: RecordTypeSpec,
    opts: QueryOpts,
) -> Result<QueryResultDto, DnsError> {
    let request = build_request(name, rtype, &opts)?;
    let transport = endpoint.protocol.to_string();
    let start = Instant::now();

    let response: DnsResponse = exchange(endpoint, request, &opts).await?;
    let latency_ms = start.elapsed().as_millis() as u64;

    Ok(build_result(&response, name, rtype, &transport, latency_ms))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::dns::query::types::AnswerDto;

    #[test]
    fn record_type_spec_round_trips_from_str() {
        for spec in [
            RecordTypeSpec::A,
            RecordTypeSpec::Aaaa,
            RecordTypeSpec::Mx,
            RecordTypeSpec::Txt,
            RecordTypeSpec::Ns,
            RecordTypeSpec::Soa,
            RecordTypeSpec::Cname,
            RecordTypeSpec::Srv,
            RecordTypeSpec::Caa,
            RecordTypeSpec::Ptr,
            RecordTypeSpec::Other(65280),
        ] {
            let parsed: RecordTypeSpec = spec.to_string().parse().unwrap();
            assert_eq!(parsed, spec, "round-trip failed for {spec}");
        }
    }

    #[test]
    fn record_type_spec_rejects_unknown_name() {
        assert!(matches!(
            "UNKNOWN".parse::<RecordTypeSpec>(),
            Err(DnsError::InvalidInput(_))
        ));
    }

    #[test]
    fn query_result_dto_serializes_to_camel_case() {
        let dto = QueryResultDto {
            query_name: "www.mock.test.".into(),
            record_type: "A".into(),
            rcode: "noerror".into(),
            header_rcode: 0,
            extended_rcode: 0,
            full_rcode: 0,
            answers: vec![AnswerDto {
                data: "192.0.2.10".into(),
                ttl: 60,
            }],
            authority_nodata: false,
            authority_soa: false,
            ad_flag: false,
            aa_flag: true,
            ra_flag: false,
            truncated: false,
            edns_present: false,
            latency_ms: 12,
            transport_used: "udp".into(),
            response_bytes: 64,
            additional_glue: Vec::new(),
        };
        let json = serde_json::to_string_pretty(&dto).unwrap();
        assert!(json.contains("queryName"));
        assert!(json.contains("recordType"));
        assert!(json.contains("authorityNodata"));
        assert!(json.contains("authoritySoa"));
        assert!(json.contains("adFlag"));
        assert!(json.contains("aaFlag"));
        assert!(json.contains("raFlag"));
        assert!(json.contains("latencyMs"));
        assert!(json.contains("transportUsed"));
        assert!(json.contains("responseBytes"));
        assert!(json.contains("additionalGlue"));
    }

    #[test]
    fn is_nodata_true_for_noerror_empty_answers_soa_authority() {
        let dto = QueryResultDto {
            query_name: "www.mock.test.".into(),
            record_type: "TYPE65280".into(),
            rcode: "noerror".into(),
            header_rcode: 0,
            extended_rcode: 0,
            full_rcode: 0,
            answers: Vec::new(),
            authority_nodata: true,
            authority_soa: true,
            ad_flag: false,
            aa_flag: true,
            ra_flag: false,
            truncated: false,
            edns_present: false,
            latency_ms: 5,
            transport_used: "udp".into(),
            response_bytes: 96,
            additional_glue: Vec::new(),
        };
        assert!(is_nodata(&dto));
        assert!(!is_nxdomain(&dto));
    }

    #[test]
    fn is_nxdomain_true_for_nxdomain_rcode() {
        let dto = QueryResultDto {
            query_name: "absent.mock.test.".into(),
            record_type: "A".into(),
            rcode: "nxdomain".into(),
            header_rcode: 3,
            extended_rcode: 0,
            full_rcode: 3,
            answers: Vec::new(),
            authority_nodata: false,
            authority_soa: false,
            ad_flag: false,
            aa_flag: false,
            ra_flag: false,
            truncated: false,
            edns_present: false,
            latency_ms: 3,
            transport_used: "udp".into(),
            response_bytes: 64,
            additional_glue: Vec::new(),
        };
        assert!(is_nxdomain(&dto));
        assert!(!is_nodata(&dto));
    }

    #[test]
    fn query_opts_default_is_rd_true_timeout_two_seconds() {
        let opts = QueryOpts::default();
        assert!(opts.rd);
        assert_eq!(opts.timeout, Duration::from_secs(2));
        assert!(opts.edns_size.is_none());
        assert!(!opts.dnssec_ok);
        assert!(opts.pinned_root_cert.is_none());
    }
}
