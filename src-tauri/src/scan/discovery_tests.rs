use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::sync::Arc;

use super::{discover_with, DiscoveryServices, ProbeOutcome};

#[tokio::test]
async fn discovery_uses_fake_ping_tcp_arp_and_dns_seams() {
    let ping_alive = HashSet::from([Ipv4Addr::new(192, 168, 1, 1)]);
    let tcp_alive = HashSet::from([Ipv4Addr::new(192, 168, 1, 2)]);
    let arp = HashMap::from([
        (
            Ipv4Addr::new(192, 168, 1, 1),
            "AA:BB:CC:DD:EE:01".to_owned(),
        ),
        (
            Ipv4Addr::new(192, 168, 1, 2),
            "AA:BB:CC:DD:EE:02".to_owned(),
        ),
    ]);
    let services = DiscoveryServices::new(
        Arc::new(move |ip| {
            let ping_alive = ping_alive.clone();
            Box::pin(async move { ping_alive.contains(&ip) })
        }),
        Arc::new(move |ip| {
            let tcp_alive = tcp_alive.clone();
            Box::pin(async move { tcp_alive.contains(&ip) })
        }),
        Arc::new(move || {
            let arp = arp.clone();
            Box::pin(async move { Ok(arp) })
        }),
        Arc::new(|ip| Box::pin(async move { Some(format!("host-{ip}")) })),
    );

    let result = discover_with("192.168.1.0/30", true, services)
        .await
        .expect("discover");

    assert_eq!(result.hosts.len(), 2);
    assert_eq!(result.hosts[0].found_by, ProbeOutcome::Ping);
    assert_eq!(result.hosts[1].found_by, ProbeOutcome::Tcp);
    assert_eq!(
        result.hosts[1].hostname.as_deref(),
        Some("host-192.168.1.2")
    );
    assert!(result.hosts.iter().all(|host| host.open_ports.is_empty()));
}

#[tokio::test]
async fn discovery_does_not_probe_ports() {
    let services = DiscoveryServices::new(
        Arc::new(|ip| Box::pin(async move { ip == Ipv4Addr::new(192, 168, 1, 1) })),
        Arc::new(|_| Box::pin(async { false })),
        Arc::new(|| Box::pin(async { Ok(HashMap::new()) })),
        Arc::new(|_| Box::pin(async { None })),
    );

    let result = discover_with("192.168.1.0/30", false, services)
        .await
        .expect("discover");

    assert_eq!(result.hosts.len(), 1);
    assert_eq!(result.hosts[0].ip, Ipv4Addr::new(192, 168, 1, 1));
    assert!(result.hosts[0].open_ports.is_empty());
}
