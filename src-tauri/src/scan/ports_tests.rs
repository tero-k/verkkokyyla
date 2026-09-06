use std::collections::HashSet;
use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{
    probable_service, probe_ports_with_connector, production_port_connector, OpenPort,
    PortConnector, COMMON_PORTS,
};

#[tokio::test]
async fn probe_ports_returns_fake_open_ports_when_list_contains_them() {
    let (_cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
    let open = Arc::new(HashSet::from([22, 443]));
    let connector: PortConnector = Arc::new(move |_host, port| {
        let open = Arc::clone(&open);
        Box::pin(async move { open.contains(&port) })
    });
    let ports = [(22, "ssh"), (80, "http"), (443, "https")];

    let result = probe_ports_with_connector(
        Ipv4Addr::new(192, 168, 1, 10),
        &ports,
        connector,
        2,
        Duration::ZERO,
        &mut cancel_rx,
    )
    .await;

    assert_eq!(
        result,
        vec![
            OpenPort {
                port: 22,
                service: "ssh".to_owned()
            },
            OpenPort {
                port: 443,
                service: "https".to_owned()
            }
        ]
    );
}

#[tokio::test]
async fn probe_ports_returns_empty_when_port_list_is_empty() {
    let (_cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
    let attempts = Arc::new(AtomicUsize::new(0));
    let connector = counting_connector(Arc::clone(&attempts), HashSet::from([22]));

    let result = probe_ports_with_connector(
        Ipv4Addr::LOCALHOST,
        &[],
        connector,
        4,
        Duration::ZERO,
        &mut cancel_rx,
    )
    .await;

    assert!(result.is_empty());
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn probe_ports_stops_starting_probes_when_cancelled_mid_scan() {
    let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
    let attempts = Arc::new(AtomicUsize::new(0));
    let connector = counting_connector(Arc::clone(&attempts), HashSet::from([21]));
    let ports = [(21, "ftp"), (22, "ssh"), (23, "telnet")];

    let scan = tokio::spawn(async move {
        probe_ports_with_connector(
            Ipv4Addr::LOCALHOST,
            &ports,
            connector,
            1,
            Duration::from_millis(200),
            &mut cancel_rx,
        )
        .await
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(cancel_tx.send(true).is_ok());
    let result = scan.await.expect("port scan task");

    assert_eq!(
        result,
        vec![OpenPort {
            port: 21,
            service: "ftp".to_owned()
        }]
    );
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn probe_ports_returns_empty_without_connector_calls_when_pre_cancelled() {
    let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
    let attempts = Arc::new(AtomicUsize::new(0));
    let connector = counting_connector(Arc::clone(&attempts), HashSet::from([22]));
    cancel_tx.send(true).expect("cancel scan");

    let result = probe_ports_with_connector(
        Ipv4Addr::LOCALHOST,
        &[(22, "ssh")],
        connector,
        1,
        Duration::ZERO,
        &mut cancel_rx,
    )
    .await;

    assert!(result.is_empty());
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn probe_ports_paces_attempts_by_min_interval() {
    let (_cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
    let attempts = Arc::new(AtomicUsize::new(0));
    let connector = counting_connector(Arc::clone(&attempts), HashSet::from([21, 22, 23]));
    let ports = [(21, "ftp"), (22, "ssh"), (23, "telnet")];

    let started = Instant::now();
    let result = probe_ports_with_connector(
        Ipv4Addr::LOCALHOST,
        &ports,
        connector,
        3,
        Duration::from_millis(35),
        &mut cancel_rx,
    )
    .await;

    assert_eq!(result.len(), 3);
    assert_eq!(attempts.load(Ordering::SeqCst), 3);
    assert!(started.elapsed() >= Duration::from_millis(60));
}

#[tokio::test]
async fn production_port_connector_respects_connect_timeout_when_host_does_not_answer() {
    let connector = production_port_connector(Duration::from_millis(1));
    let started = Instant::now();

    let result = connector(Ipv4Addr::new(192, 0, 2, 1), 65000).await;

    assert!(!result);
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn probable_service_returns_known_service_and_unknown_fallback() {
    assert_eq!(probable_service(22), "ssh");
    assert_eq!(probable_service(8443), "https-alt");
    assert_eq!(probable_service(65000), "unknown");
    assert_eq!(COMMON_PORTS.len(), 25);
}

fn counting_connector(attempts: Arc<AtomicUsize>, open_ports: HashSet<u16>) -> PortConnector {
    Arc::new(move |_host, port| {
        let attempts = Arc::clone(&attempts);
        let open_ports = open_ports.clone();
        Box::pin(async move {
            attempts.fetch_add(1, Ordering::SeqCst);
            open_ports.contains(&port)
        })
    })
}
