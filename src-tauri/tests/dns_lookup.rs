mod common;

use common::mock_dns::start_mock;
use verkkokyyla_lib::dns::client::{DnsProtocol, ResolverEndpointDto};
use verkkokyyla_lib::dns::manager::run_dns_lookup;

fn endpoint(protocol: DnsProtocol, address: &str) -> ResolverEndpointDto {
    ResolverEndpointDto {
        name: "test".into(),
        protocol,
        address: address.into(),
    }
}

#[tokio::test]
async fn multi_record_type_lookup_streams_events_and_summary() {
    let (handle, udp, _tcp, _tls, _cert) = start_mock().await;
    let mut events = Vec::new();
    let summary = run_dns_lookup(
        "www.mock.test.",
        vec!["A".into(), "AAAA".into(), "MX".into(), "TXT".into(), "NS".into(), "CNAME".into()],
        endpoint(DnsProtocol::Udp, &udp.to_string()),
        |event| events.push(event),
    )
    .await
    .expect("lookup should succeed");

    assert_eq!(events.len(), 6);
    assert_eq!(summary.completed, 6);
    assert_eq!(summary.failed, 0);

    drop(handle);
}

#[tokio::test]
async fn unknown_record_type_produces_failed_event() {
    let (handle, udp, _tcp, _tls, _cert) = start_mock().await;
    let mut events = Vec::new();
    let summary = run_dns_lookup(
        "www.mock.test.",
        vec!["INVALID".into()],
        endpoint(DnsProtocol::Udp, &udp.to_string()),
        |event| events.push(event),
    )
    .await
    .expect("lookup should succeed");

    assert_eq!(events.len(), 1);
    assert_eq!(summary.completed, 0);
    assert_eq!(summary.failed, 1);

    drop(handle);
}

#[tokio::test]
async fn empty_record_types_returns_invalid_input() {
    let (handle, udp, _tcp, _tls, _cert) = start_mock().await;
    let result = run_dns_lookup(
        "www.mock.test.",
        vec![],
        endpoint(DnsProtocol::Udp, &udp.to_string()),
        |_| {},
    )
    .await;

    assert!(result.is_err(), "empty record types should fail");

    drop(handle);
}
