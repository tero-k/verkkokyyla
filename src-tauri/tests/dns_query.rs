mod common;

use common::mock_dns::start_mock;
use verkkokyyla_lib::dns::client::{DnsProtocol, ResolverEndpointDto};
use verkkokyyla_lib::dns::query::{query_once, is_nodata, is_nxdomain, QueryOpts, RecordTypeSpec};

fn endpoint(protocol: DnsProtocol, address: &str, name: &str) -> ResolverEndpointDto {
    ResolverEndpointDto {
        name: name.into(),
        protocol,
        address: address.into(),
    }
}

#[tokio::test]
async fn udp_a_query_returns_answer() {
    let (handle, udp, _tcp, _tls, _cert) = start_mock().await;
    let result = query_once(
        &endpoint(DnsProtocol::Udp, &udp.to_string(), "test"),
        "www.mock.test.",
        RecordTypeSpec::A,
        QueryOpts::default(),
    )
    .await
    .expect("UDP A query should succeed");

    assert_eq!(result.rcode, "noerror");
    assert_eq!(result.transport_used, "udp");
    assert_eq!(result.answers.len(), 1);
    assert_eq!(result.answers[0].data, "192.0.2.10");
    assert!(result.aa_flag);

    drop(handle);
}

#[tokio::test]
async fn tcp_a_query_returns_answer() {
    let (handle, _udp, tcp, _tls, _cert) = start_mock().await;
    let result = query_once(
        &endpoint(DnsProtocol::Tcp, &tcp.to_string(), "test"),
        "www.mock.test.",
        RecordTypeSpec::A,
        QueryOpts::default(),
    )
    .await
    .expect("TCP A query should succeed");

    assert_eq!(result.rcode, "noerror");
    assert_eq!(result.transport_used, "tcp");
    assert_eq!(result.answers.len(), 1);
    assert_eq!(result.answers[0].data, "192.0.2.10");

    drop(handle);
}

#[tokio::test]
async fn tls_a_query_with_pinned_cert_returns_answer() {
    let (handle, _udp, _tcp, tls, cert) = start_mock().await;
    let mut opts = QueryOpts::default();
    opts.pinned_root_cert = Some(cert);
    let result = query_once(
        &endpoint(DnsProtocol::Tls, &tls.to_string(), "mock.test"),
        "www.mock.test.",
        RecordTypeSpec::A,
        opts,
    )
    .await
    .expect("TLS A query with pinned cert should succeed");

    assert_eq!(result.rcode, "noerror");
    assert_eq!(result.transport_used, "tls");
    assert_eq!(result.answers.len(), 1);
    assert_eq!(result.answers[0].data, "192.0.2.10");

    drop(handle);
}

#[tokio::test]
async fn nxdomain_for_absent_name() {
    let (handle, udp, _tcp, _tls, _cert) = start_mock().await;
    let result = query_once(
        &endpoint(DnsProtocol::Udp, &udp.to_string(), "test"),
        "absent.mock.test.",
        RecordTypeSpec::A,
        QueryOpts::default(),
    )
    .await
    .expect("Query should return NXDOMAIN, not fail");

    assert_eq!(result.rcode, "nxdomain");
    assert!(is_nxdomain(&result));
    assert!(!is_nodata(&result));

    drop(handle);
}

#[tokio::test]
async fn nodata_for_unknown_type_with_soa_authority() {
    let (handle, udp, _tcp, _tls, _cert) = start_mock().await;
    let result = query_once(
        &endpoint(DnsProtocol::Udp, &udp.to_string(), "test"),
        "www.mock.test.",
        RecordTypeSpec::Other(65280),
        QueryOpts::default(),
    )
    .await
    .expect("NODATA query should succeed");

    assert_eq!(result.rcode, "noerror");
    assert!(result.answers.is_empty());
    assert!(result.authority_soa);
    assert!(is_nodata(&result));

    drop(handle);
}

#[tokio::test]
async fn rd_zero_referral_returns_additional_glue() {
    let (handle, udp, _tcp, _tls, _cert) = start_mock().await;
    let mut opts = QueryOpts::default();
    opts.rd = false;
    let result = query_once(
        &endpoint(DnsProtocol::Udp, &udp.to_string(), "test"),
        "host.sub.mock.test.",
        RecordTypeSpec::A,
        opts,
    )
    .await
    .expect("Referral query should succeed");

    assert_eq!(result.rcode, "noerror");
    assert!(result.answers.is_empty());
    assert!(!result.ra_flag);
    assert!(
        result.additional_glue.iter().any(|answer| answer.data == "192.0.2.60"),
        "expected glue A record in additional section"
    );

    drop(handle);
}
