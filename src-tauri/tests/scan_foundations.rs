use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};

use verkkokyyla_lib::db::{Database, NewScan, ScanHostRow};
use verkkokyyla_lib::scan::cidr::{expand_ipv4_cidr, CidrError};
use verkkokyyla_lib::scan::interfaces::{filter_interface_candidates, InterfaceCandidate};
use verkkokyyla_lib::scan::oui::OuiDatabase;

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "verkkokyyla-scan-{name}-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        Self(dir)
    }

    fn db_file(&self) -> PathBuf {
        self.0.join("nested").join("scan.db")
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

async fn test_db(path: &Path) -> Database {
    Database::connect(path).await.expect("db")
}

fn scan_host(scan_id: i64, ip: &str, found_by: &str) -> ScanHostRow {
    ScanHostRow {
        scan_id,
        ip: ip.to_owned(),
        mac: Some("AA:BB:CC:DD:EE:FF".to_owned()),
        vendor: Some("Example Networks".to_owned()),
        hostname: Some(format!("host-{ip}")),
        found_by: found_by.to_owned(),
        open_ports: "[]".to_owned(),
        at: verkkokyyla_lib::db::now_rfc3339(),
    }
}

fn scan_host_with_ports(scan_id: i64, ip: &str, open_ports: &str) -> ScanHostRow {
    ScanHostRow {
        open_ports: open_ports.to_owned(),
        ..scan_host(scan_id, ip, "ping")
    }
}

#[tokio::test]
async fn scan_persistence_round_trips_hosts_and_cascades_delete() {
    let dir = TempDir::new("round-trip");
    let db = test_db(&dir.db_file()).await;
    let scan_id = db
        .create_scan(&NewScan {
            interface_name: "Ethernet".to_owned(),
            cidr: "192.168.1.0/24".to_owned(),
            tcp_fallback: true,
            started_at: verkkokyyla_lib::db::now_rfc3339(),
        })
        .await
        .expect("create scan");

    db.insert_scan_hosts_batch(
        scan_id,
        &[
            scan_host(scan_id, "192.168.1.1", "ping"),
            scan_host(scan_id, "192.168.1.2", "arp"),
        ],
    )
    .await
    .expect("insert hosts");
    db.finish_scan(scan_id, &verkkokyyla_lib::db::now_rfc3339(), "completed")
        .await
        .expect("finish scan");

    let scans = db.list_scans().await.expect("list scans");
    assert_eq!(scans.len(), 1);
    assert_eq!(scans[0].interface_name, "Ethernet");
    assert_eq!(scans[0].host_count, 2);
    assert_eq!(scans[0].status, "completed");

    let loaded = db.load_scan(scan_id).await.expect("load scan");
    assert_eq!(loaded.scan.cidr, "192.168.1.0/24");
    assert_eq!(loaded.hosts.len(), 2);
    assert_eq!(loaded.hosts[0].ip, "192.168.1.1");
    assert_eq!(loaded.hosts[1].found_by, "arp");

    db.delete_scan(scan_id).await.expect("delete scan");
    assert!(db.list_scans().await.expect("list after delete").is_empty());
    assert!(db
        .load_scan(scan_id)
        .await
        .expect("load deleted scan")
        .hosts
        .is_empty());
}

#[tokio::test]
async fn scan_persistence_round_trips_host_open_ports() {
    let dir = TempDir::new("open-ports");
    let db = test_db(&dir.db_file()).await;
    let scan_id = db
        .create_scan(&NewScan {
            interface_name: "Ethernet".to_owned(),
            cidr: "192.168.1.0/24".to_owned(),
            tcp_fallback: true,
            started_at: verkkokyyla_lib::db::now_rfc3339(),
        })
        .await
        .expect("create scan");
    let open_ports = r#"[{"port":22,"service":"ssh"},{"port":443,"service":"https"}]"#;

    db.insert_scan_hosts_batch(
        scan_id,
        &[scan_host_with_ports(scan_id, "192.168.1.10", open_ports)],
    )
    .await
    .expect("insert host with ports");

    let loaded = db.load_scan(scan_id).await.expect("load scan");

    assert_eq!(loaded.hosts.len(), 1);
    assert_eq!(loaded.hosts[0].open_ports, open_ports);
}

#[test]
fn cidr_expansion_accepts_up_to_22_and_rejects_larger_ranges() {
    assert_eq!(expand_ipv4_cidr("192.168.1.0/24").expect("/24").len(), 254);
    assert_eq!(expand_ipv4_cidr("10.0.0.0/22").expect("/22").len(), 1022);
    assert!(matches!(
        expand_ipv4_cidr("10.0.0.0/21"),
        Err(CidrError::RangeTooLarge { prefix_len: 21 })
    ));
}

#[test]
fn oui_lookup_uses_longest_prefix_randomized_detection_and_normalization() {
    let fixture = "A8:BB:CC Vendor24\nA8:BB:CC:D Vendor28\nA8:BB:CC:DD:E Vendor36\n";
    let db = OuiDatabase::parse(fixture).expect("parse manuf fixture");

    assert_eq!(
        db.lookup("a8-bb-cc-12-34-56").expect("lookup"),
        Some("Vendor24".to_owned())
    );
    assert_eq!(
        db.lookup("A8:BB:CC:D1:23:45").expect("lookup"),
        Some("Vendor28".to_owned())
    );
    assert_eq!(
        db.lookup("a8bb.ccdd.e123").expect("lookup"),
        Some("Vendor36".to_owned())
    );
    assert_eq!(
        db.lookup("02:00:00:00:00:01").expect("lookup"),
        Some("Randomized / locally administered".to_owned())
    );
}

#[test]
fn interface_filtering_keeps_physical_up_ipv4_interfaces_and_marks_primary() {
    let candidates = vec![
        InterfaceCandidate {
            name: "Loopback".to_owned(),
            description: "Loopback".to_owned(),
            ipv4: Ipv4Addr::new(127, 0, 0, 1),
            prefix_len: 8,
            is_up: true,
            is_loopback: true,
            has_default_gateway: false,
        },
        InterfaceCandidate {
            name: "Tailscale".to_owned(),
            description: "Tailscale Tunnel".to_owned(),
            ipv4: Ipv4Addr::new(100, 64, 0, 2),
            prefix_len: 32,
            is_up: true,
            is_loopback: false,
            has_default_gateway: false,
        },
        InterfaceCandidate {
            name: "Ethernet".to_owned(),
            description: "Intel Ethernet".to_owned(),
            ipv4: Ipv4Addr::new(192, 168, 1, 20),
            prefix_len: 24,
            is_up: true,
            is_loopback: false,
            has_default_gateway: true,
        },
        InterfaceCandidate {
            name: "Wi-Fi".to_owned(),
            description: "Wireless".to_owned(),
            ipv4: Ipv4Addr::new(192, 168, 2, 30),
            prefix_len: 24,
            is_up: false,
            is_loopback: false,
            has_default_gateway: false,
        },
    ];

    let interfaces = filter_interface_candidates(candidates);

    assert_eq!(interfaces.len(), 1);
    assert_eq!(interfaces[0].name, "Ethernet");
    assert!(interfaces[0].is_primary);
}
