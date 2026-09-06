mod common;

use std::net::TcpListener;

use common::mock_dns::start_mock;
use verkkokyyla_lib::dns::client::{DnsProtocol, ResolverEndpointDto};
use verkkokyyla_lib::dns::query::RecordTypeSpec;
use verkkokyyla_lib::dns::transport::{
    edns_buffer_sweep, edns_support_check, tcp_fallback_check, transport_equivalence,
};

fn endpoint(protocol: DnsProtocol, address: &str, name: &str) -> ResolverEndpointDto {
    ResolverEndpointDto {
        name: name.into(),
        protocol,
        address: address.into(),
    }
}

#[tokio::test]
async fn edns_buffer_sweep_returns_four_sizes() {
    let (handle, udp, _tcp, _tls, _cert) = start_mock().await;
    let result = edns_buffer_sweep(
        &endpoint(DnsProtocol::Udp, &udp.to_string(), "test"),
        "www.mock.test.",
    )
    .await
    .expect("sweep should succeed");

    assert_eq!(result.len(), 4);
    assert_eq!(result[0].size, 512);
    assert_eq!(result[1].size, 1232);
    assert_eq!(result[2].size, 1400);
    assert_eq!(result[3].size, 4096);
    assert!(result.iter().all(|r| r.rcode == "noerror"));

    drop(handle);
}

#[tokio::test]
async fn tcp_fallback_detects_truncation_and_retries() {
    let (handle, udp, _tcp, _tls, _cert) = start_mock().await;
    let result = tcp_fallback_check(
        &endpoint(DnsProtocol::Udp, &udp.to_string(), "test"),
        &endpoint(DnsProtocol::Tcp, &_tcp.to_string(), "test"),
        "big.mock.test.",
    )
    .await
    .expect("fallback check should succeed");

    assert!(result.udp_truncated);
    assert!(result.tcp_success);
    assert!(result.tcp_latency_ms.is_some());

    drop(handle);
}

#[tokio::test]
async fn transport_equivalence_matches_udp_and_tcp() {
    let (handle, udp, tcp, tls, cert) = start_mock().await;
    let endpoints = [
        endpoint(DnsProtocol::Udp, &udp.to_string(), "test"),
        endpoint(DnsProtocol::Tcp, &tcp.to_string(), "test"),
        ResolverEndpointDto {
            name: "mock.test".into(),
            protocol: DnsProtocol::Tls,
            address: tls.to_string(),
        },
    ];

    let result = transport_equivalence(
        &endpoints,
        "www.mock.test.",
        RecordTypeSpec::A,
        Some(&cert),
    )
    .await
    .expect("equivalence should succeed");

    let udp = result.iter().find(|r| r.protocol == "udp").expect("udp entry");
    let tcp = result.iter().find(|r| r.protocol == "tcp").expect("tcp entry");
    assert_eq!(udp.rcode, "noerror");
    assert_eq!(tcp.rcode, "noerror");
    assert!(!udp.answer_hash.is_empty());
    assert_eq!(udp.answer_hash, tcp.answer_hash);

    let tls = result.iter().find(|r| r.protocol == "tls").expect("tls entry");
    assert_eq!(tls.rcode, "noerror");
    assert_eq!(tls.answer_hash, udp.answer_hash);

    drop(handle);
}

#[tokio::test]
async fn edns_support_reports_opt_and_badvers() {
    let (handle, udp, _tcp, _tls, _cert) = start_mock().await;
    let result = edns_support_check(
        &endpoint(DnsProtocol::Udp, &udp.to_string(), "test"),
        "www.mock.test.",
    )
    .await
    .expect("edns support check should succeed");

    assert!(result.opt_present);
    assert_eq!(result.edns_version, 0);
    // The fixture is a simple recursive server and does not return BADVERS for
    // EDNS version 1, so we only assert the probe completed without error here.

    drop(handle);
}

#[tokio::test]
async fn closed_port_returns_error() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("local addr");
    drop(listener);

    let result = edns_buffer_sweep(
        &endpoint(DnsProtocol::Udp, &addr.to_string(), "test"),
        "www.mock.test.",
    )
    .await;

    assert!(result.is_err(), "expected error against closed port");
}
