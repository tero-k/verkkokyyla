mod common;

use common::mock_dns::start_mock;
use verkkokyyla_lib::dns::client::{DnsProtocol, ResolverEndpointDto};
use verkkokyyla_lib::dns::integrity::{resolver_integrity_check, zone_transfer_check};
use verkkokyyla_lib::dns::query::RecordTypeSpec;

fn endpoint(protocol: DnsProtocol, address: &str, name: &str) -> ResolverEndpointDto {
    ResolverEndpointDto {
        name: name.into(),
        protocol,
        address: address.into(),
    }
}

#[tokio::test]
async fn integrity_check_against_authoritative_fixture() {
    let (handle, udp, tcp, _tls, _cert) = start_mock().await;

    let report = resolver_integrity_check(
        &endpoint(DnsProtocol::Udp, &udp.to_string(), "primary"),
        &endpoint(DnsProtocol::Tcp, &tcp.to_string(), "reference"),
        "www.mock.test.",
        RecordTypeSpec::A,
    )
    .await
    .expect("integrity check should succeed");

    assert!(report.answers_match, "answers should match the authoritative reference");
    assert!(report.filtering_ok, "filtering should be ok when answers match");
    assert!(!report.open_resolver, "fixture should not be an open resolver");

    drop(handle);
}

#[tokio::test]
async fn axfr_allowed_with_matching_token() {
    let (handle, _udp, tcp, _tls, _cert) = start_mock().await;

    let result = zone_transfer_check(
        &endpoint(DnsProtocol::Tcp, &tcp.to_string(), "axfr"),
        "mock.test.",
        "mock.test.",
    )
    .await
    .expect("zone transfer check should complete");

    assert!(result.allowed, "AXFR should be allowed with matching token");
    assert!(result.record_count > 0, "zone should contain records");
    assert!(result.serial.is_some(), "SOA serial should be parsed");

    drop(handle);
}

#[tokio::test]
async fn axfr_denied_without_token() {
    let (handle, _udp, tcp, _tls, _cert) = start_mock().await;

    let result = zone_transfer_check(
        &endpoint(DnsProtocol::Tcp, &tcp.to_string(), "axfr"),
        "mock.test.",
        "",
    )
    .await
    .expect("zone transfer check should complete");

    assert!(!result.allowed, "AXFR should be denied without matching token");

    drop(handle);
}
