mod common;

use common::mock_dns::start_mock;
use verkkokyyla_lib::dns::client::{DnsProtocol, ResolverEndpointDto};
use verkkokyyla_lib::dns::delegation::delegation_report;

fn endpoint(protocol: DnsProtocol, address: &str, name: &str) -> ResolverEndpointDto {
    ResolverEndpointDto {
        name: name.into(),
        protocol,
        address: address.into(),
    }
}

#[tokio::test]
async fn delegation_report_for_sub_zone() {
    let (handle, udp, _tcp, _tls, _cert) = start_mock().await;

    let report = delegation_report(
        &endpoint(DnsProtocol::Udp, &udp.to_string(), "udp"),
        "sub.mock.test",
    )
    .await
    .expect("delegation report should succeed");

    assert!(
        !report.parent_ns.is_empty(),
        "parent ns list should not be empty"
    );
    assert!(
        !report.child_ns.is_empty(),
        "child ns list should be retrievable from the resolver"
    );
    assert_eq!(
        report.ns_consistent,
        Some(true),
        "recursive and authoritative NS sets should match on fixture"
    );
    assert!(
        report.authoritative,
        "fixture should return an authoritative response"
    );
    // A single-server fixture cannot prove serial consistency, so the result
    // is deliberately left as None.
    assert_eq!(
        report.serial_consistent, None,
        "one server cannot be compared"
    );

    drop(handle);
}
