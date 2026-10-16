//! Deterministic HTTPS fixture for the read-only OPNsense integration.
//! The fixture refuses non-GET methods, records the wire requests and uses
//! fake credentials only; no firewall or OS keyring is required.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine as _;
use parking_lot::Mutex;
use rcgen::generate_simple_self_signed;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use verkkokyyla_lib::db::Database;
use verkkokyyla_lib::opnsense::backup::{OpnsenseBackupError, OpnsenseBackupManager};
use verkkokyyla_lib::opnsense::client::{
    OpnsenseApi, OpnsenseClient, OpnsenseConnection, OpnsenseError, MAX_REVISION_BYTES,
};
use verkkokyyla_lib::opnsense::firmware::OpnsenseFirmwareManager;
use verkkokyyla_lib::opnsense::manager::OpnsenseManager;
use verkkokyyla_lib::opnsense::monitor::OpnsenseMonitorManager;
use verkkokyyla_lib::opnsense::types::{
    CreateOpnsenseProfileRequest, OpnsenseApiFactory, OpnsenseSnapshotDto,
    UpdateOpnsenseProfileRequest,
};
use verkkokyyla_lib::secrets::{MemoryStore, SecretError, SecretStore};

const API_KEY: &str = "fixture-api-key-only";
const API_SECRET: &str = "fixture-api-secret-only";
const XML: &str =
    "<?xml version=\"1.0\"?><opnsense><system><hostname>fixture</hostname></system></opnsense>";
const PASS_UUID: &str = "11111111-1111-4111-8111-111111111111";
const BLOCK_UUID: &str = "22222222-2222-4222-8222-222222222222";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "verkkokyyla-opnsense-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos(),
            rand::random::<u64>()
        ));
        std::fs::create_dir(&path).expect("create test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Debug)]
struct Request {
    method: String,
    target: String,
    authorization: Option<String>,
}

#[derive(Clone)]
struct Reply {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Reply {
    fn json(body: &str) -> Self {
        Self {
            status: 200,
            content_type: "application/json",
            body: body.as_bytes().to_vec(),
        }
    }

    fn status(status: u16) -> Self {
        Self {
            status,
            content_type: "text/plain",
            // Deliberately echo credential-looking data: client errors must
            // never expose the raw response body.
            body: format!("device error: {API_KEY} {API_SECRET}").into_bytes(),
        }
    }
}

struct Fixture {
    port: u16,
    requests: Arc<Mutex<Vec<Request>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Fixture {
    fn connection(&self, allow_invalid_certs: bool) -> OpnsenseConnection {
        OpnsenseConnection {
            host: "127.0.0.1".to_owned(),
            port: self.port,
            allow_invalid_certs,
            api_key: API_KEY.to_owned(),
            api_secret: API_SECRET.to_owned(),
        }
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().clone()
    }

    fn assert_get_only(&self) {
        let requests = self.requests();
        assert!(!requests.is_empty(), "fixture received no requests");
        for request in &requests {
            assert_eq!(
                request.method, "GET",
                "unexpected remote method: {request:?}"
            );
            assert!(
                request.target.starts_with("/api/"),
                "unexpected path: {}",
                request.target
            );
            assert_eq!(
                request.authorization.as_deref(),
                Some(expected_auth().as_str()),
                "bad Basic auth for {}",
                request.target
            );
            assert!(!request.target.contains(API_KEY));
            assert!(!request.target.contains(API_SECRET));
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn expected_auth() -> String {
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode(format!("{API_KEY}:{API_SECRET}"))
    )
}

fn default_reply(path: &str, target: &str) -> Reply {
    match path {
        "/api/core/system/status" => Reply::json(r#"{"status":"ok"}"#),
        "/api/firewall/filter/searchRule" => {
            let query =
                url::Url::parse(&format!("https://fixture{target}")).expect("rule query URL");
            let params: HashMap<_, _> = query.query_pairs().into_owned().collect();
            assert_eq!(params.get("show_all").map(String::as_str), Some("1"));
            // GET pagination does not exist: one response with ALL rows.
            assert!(!params.contains_key("current"));
            assert!(!params.contains_key("rowCount"));
            Reply::json(&format!(
                r#"{{"total":2,"rowCount":9999,"current":1,"rows":[{{"uuid":"{PASS_UUID}","action":"pass","description":"allow LAN","interface":"lan","enabled":"1"}},{{"uuid":"{BLOCK_UUID}","action":"block","description":"deny WAN","interface":"wan","enabled":"0"}}]}}"#
            ))
        }
        "/api/firewall/filter_util/ruleStats" => Reply::json(&format!(
            r#"{{"status":"ok","stats":{{"{PASS_UUID}":{{"packets":"12","evaluations":"21","bytes":"1200"}},"{BLOCK_UUID}":{{"packets":"0","evaluations":"3","bytes":"0"}}}}}}"#
        )),
        "/api/diagnostics/firewall/listRuleIds" => Reply::json(r#"{"rows":[]}"#),
        "/api/interfaces/overview/interfacesInfo" => {
            Reply::json(
                r#"{"total":3,"rowCount":9999,"current":1,"rows":[
                    {"device":"igb0","description":"LAN","identifier":"lan","enabled":"1","status":"up","macaddr":"aa:bb:cc:dd:ee:ff","mtu":"1500","media":"1000baseT <full-duplex>","addr4":"192.0.2.1/24","addr6":"","ipv4":[{"ipaddr":"192.0.2.1/24"}],"ipv6":[]},
                    {"device":"lagg0","description":"UPLINK","identifier":"opt1","enabled":"1","status":"up","laggproto":"lacp","laggport":{"em0":{"state":["attached","active"]},"em1":{"state":["attached"]}},"addr4":"","addr6":""},
                    {"device":"em0","enabled":"1","status":"up","media":"10Gbase-SR <full-duplex>","addr4":"","addr6":""}
                ]}"#,
            )
        }
        "/api/diagnostics/interface/get_interface_statistics" => Reply::json(
            // Real shape: display-label keys, per-address entries,
            // ifconfig-style counter names.
            r#"{"statistics":{"[LAN] (igb0) / 192.0.2.1":{"name":"igb0","address":"192.0.2.1","received-packets":7,"received-bytes":42,"sent-packets":5,"sent-bytes":12},"[LAN] (igb0) / aa:bb:cc:dd:ee:ff":{"name":"igb0","network":"<Link#1>","address":"aa:bb:cc:dd:ee:ff","received-packets":9,"received-bytes":99,"sent-packets":6,"sent-bytes":60}}}"#,
        ),
        "/api/diagnostics/firewall/pfStates" => Reply::json(r#"{"current":"23","limit":"1000"}"#),
        "/api/diagnostics/system/system_resources" => {
            Reply::json(r#"{"memory":{"used":2684354560,"total":"8589934592","used_frmt":"2560","total_frmt":"8192","arc":"134217728"}}"#)
        }
        "/api/diagnostics/system/system_time" => {
            Reply::json(r#"{"uptime":"3 days, 04:05:06","datetime":"Thu Oct 9 12:00:00 CEST 2026","loadavg":"0.12, 0.08, 0.06"}"#)
        }
        "/api/diagnostics/cpu_usage/stream" => Reply {
            status: 200,
            content_type: "text/event-stream",
            body: b"data: {\"total\":55,\"user\":20,\"sys\":35}\n\ndata: {\"total\":7,\"user\":4,\"sys\":3}\n\n"
                .to_vec(),
        },
        "/api/wireguard/service/show" => Reply::json(
            r#"{"rows":[
                {"type":"interface","if":"wg0","name":"Roadwarriors","status":"up"},
                {"type":"peer","if":"wg0","ifname":"Roadwarriors","name":"peer1","peer-status":"stale","latest-handshake-age":900,"endpoint":"198.51.100.9:51820","transfer-rx":1000,"transfer-tx":2000}
            ]}"#,
        ),
        "/api/openvpn/service/searchSessions" => Reply::json(
            r#"{"total":1,"rowCount":9999,"current":1,"rows":[{"type":"client","is_client":true,"common_name":"road-1","description":"Road warriors","status":"connected","real_address":"198.51.100.7:1194","virtual_address":"10.8.0.2","connected_since":"2026-10-09 08:15:00","bytes_received":123,"bytes_sent":456}]}"#,
        ),
        "/api/ipsec/sessions/searchPhase1" => {
            Reply::json(r#"{"rows":[{"name":"site-to-site","phase1desc":"Branch","connected":true,"remote-host":"203.0.113.1","version":"IKEv2"}]}"#)
        }
        "/api/tailscale/status/status" => Reply::status(404),
        "/api/tailscale/service/status" => Reply::json(r#"{"status":"1"}"#),
        "/api/core/firmware/info" => Reply::json(
            r#"{"product":{"CORE_ABI":"26.7","CORE_SERIES":"26.7","CORE_VERSION":"26.7.6","product_check":{"upgrade_packages":[{"name":"os-tailscale","new_version":"1.5"}]}},"product_version":"26.7.6_3","plugin":[{"name":"os-tailscale","version":"1.4","installed":"1"},{"name":"os-nginx","version":"1.0","installed":"0"}],"changelog":[{"series":"26.7","version":"26.7.7","date":"2026-10-13"},{"series":"26.7","version":"26.7.6","date":"2026-10-08"},{"series":"27.1","version":"27.1.1","date":"2027-01-10"},{"series":"26.7","version":"26.7.r1","date":"2026-07-01"}]}"#,
        ),
        "/api/core/backup/backups/this" => Reply::json(
            r#"{"items":[{"id":"config-older.xml","time_iso":"2025-01-01T00:00:00Z"},{"id":"config-newer.xml","time_iso":"2025-01-02T00:00:00Z"}]}"#,
        ),
        "/api/core/backup/download/this/config-newer.xml" => Reply {
            status: 200,
            content_type: "application/xml",
            body: XML.as_bytes().to_vec(),
        },
        "/api/diagnostics/firewall/streamLog" => Reply {
            status: 200,
            content_type: "text/event-stream",
            body: b"data: {\"action\":\"block\",\"src\":\"192.0.2.2\"}\n\n".to_vec(),
        },
        _ => Reply::status(404),
    }
}

async fn start_tls_fixture_server(overrides: HashMap<&'static str, Reply>) -> Fixture {
    let certificate = generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()])
        .expect("self-signed fixture certificate");
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
        certificate.key_pair.serialize_der(),
    ));
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("TLS versions")
    .with_no_client_auth()
    .with_single_cert(vec![certificate.cert.der().clone()], key)
    .expect("TLS fixture config");
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture");
    let port = listener.local_addr().expect("fixture address").port();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = Arc::clone(&requests);
    let overrides = Arc::new(overrides);
    let task = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.expect("fixture accept");
            let acceptor = acceptor.clone();
            let recorded = Arc::clone(&recorded);
            let overrides = Arc::clone(&overrides);
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(socket).await else {
                    return; // Expected when the self-signed cert is rejected.
                };
                let mut header = Vec::new();
                let mut chunk = [0u8; 1024];
                while !header.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let Ok(size) = tls.read(&mut chunk).await else {
                        return;
                    };
                    if size == 0 || header.len() + size > 16 * 1024 {
                        return;
                    }
                    header.extend_from_slice(&chunk[..size]);
                }
                let header = String::from_utf8(header).expect("HTTP headers");
                let mut lines = header.split("\r\n");
                let mut start = lines.next().expect("request line").split_whitespace();
                let method = start.next().expect("HTTP method").to_owned();
                let target = start.next().expect("HTTP target").to_owned();
                let authorization = lines.find_map(|line| {
                    line.split_once(':').and_then(|(name, value)| {
                        name.eq_ignore_ascii_case("authorization")
                            .then(|| value.trim().to_owned())
                    })
                });
                recorded.lock().push(Request {
                    method: method.clone(),
                    target: target.clone(),
                    authorization: authorization.clone(),
                });
                let path = target.split('?').next().expect("request path");
                let reply = if method != "GET" {
                    Reply::status(405)
                } else if path.starts_with("/api/")
                    && authorization.as_deref() != Some(expected_auth().as_str())
                {
                    Reply::status(401)
                } else {
                    overrides
                        .get(path)
                        .cloned()
                        .unwrap_or_else(|| default_reply(path, &target))
                };
                let status_text = match reply.status {
                    200 => "OK",
                    401 => "Unauthorized",
                    403 => "Forbidden",
                    404 => "Not Found",
                    405 => "Method Not Allowed",
                    _ => "Error",
                };
                let response = format!(
                    "HTTP/1.1 {} {}\r\ncontent-type: {}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    reply.status, status_text, reply.content_type, reply.body.len()
                );
                if tls.write_all(response.as_bytes()).await.is_ok() {
                    let _ = tls.write_all(&reply.body).await;
                    let _ = tls.shutdown().await;
                }
            });
        }
    });
    Fixture {
        port,
        requests,
        task,
    }
}

fn factory() -> OpnsenseApiFactory {
    Arc::new(|connection| {
        Box::pin(
            async move { Ok(Arc::new(OpnsenseClient::new(&connection)?) as Arc<dyn OpnsenseApi>) },
        )
    })
}

async fn profile_manager(port: u16) -> (TempDir, OpnsenseManager, MemoryStore, i64) {
    let dir = TempDir::new();
    let db = Database::connect(&dir.path().join("fixture.db"))
        .await
        .expect("open fixture SQLite");
    let store = MemoryStore::new();
    let manager = OpnsenseManager::new(db, Arc::new(store.clone()), factory());
    let profile = manager
        .create_profile(&CreateOpnsenseProfileRequest {
            name: "Read-only test firewall".into(),
            host: "127.0.0.1".into(),
            port: i64::from(port),
            allow_invalid_certs: true,
            ssh_port: 22,
        })
        .await
        .expect("create profile");
    manager
        .set_credentials(profile.id, API_KEY, API_SECRET)
        .await
        .expect("store test credentials");
    (dir, manager, store, profile.id)
}

#[tokio::test]
async fn opnsense_profiles_keep_basic_credentials_out_of_sqlite_dtos_and_errors() {
    let fixture = start_tls_fixture_server(HashMap::new()).await;
    let (dir, manager, store, id) = profile_manager(fixture.port).await;
    let db = Database::connect(&dir.path().join("fixture.db"))
        .await
        .expect("inspect SQLite");
    let row = db.load_opnsense_profile(id).await.unwrap().unwrap();
    assert!(row.secret_key.starts_with("opnsense:"));
    assert!(row.secret_key.ends_with(":api"));
    assert_ne!(row.secret_key, id.to_string());
    assert_eq!(
        store.get(&row.secret_key).await.unwrap(),
        format!(r#"{{"key":"{API_KEY}","secret":"{API_SECRET}"}}"#)
    );
    let listed = manager.list_profiles().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].has_credentials);
    for dto in &listed {
        let serialized = serde_json::to_string(dto).unwrap();
        assert!(!serialized.contains(API_KEY));
        assert!(!serialized.contains(API_SECRET));
        assert!(!serialized.contains(&row.secret_key));
    }
    let sqlite = std::fs::read(dir.path().join("fixture.db")).unwrap();
    for secret in [API_KEY, API_SECRET] {
        assert!(!sqlite
            .windows(secret.len())
            .any(|bytes| bytes == secret.as_bytes()));
    }
    let probe = manager
        .test_connection(id)
        .await
        .expect("GET connection probe");
    assert!(probe.connected && probe.authorized);
    let updated = manager
        .update_profile(&UpdateOpnsenseProfileRequest {
            id,
            name: "Renamed firewall".into(),
            host: "127.0.0.1".into(),
            port: i64::from(fixture.port),
            allow_invalid_certs: true,
            ssh_port: 22,
        })
        .await
        .unwrap();
    assert_eq!(updated.name, "Renamed firewall");
    assert!(updated.has_credentials);
    assert_eq!(
        store.get(&row.secret_key).await.unwrap(),
        format!(r#"{{"key":"{API_KEY}","secret":"{API_SECRET}"}}"#)
    );
    let second = manager
        .create_profile(&CreateOpnsenseProfileRequest {
            name: "Other".into(),
            host: "127.0.0.1".into(),
            port: i64::from(fixture.port),
            allow_invalid_certs: true,
            ssh_port: 22,
        })
        .await
        .unwrap();
    assert!(!second.has_credentials);
    let other_key = db
        .load_opnsense_profile(second.id)
        .await
        .unwrap()
        .unwrap()
        .secret_key;
    assert_ne!(row.secret_key, other_key);
    assert!(matches!(
        store.get(&other_key).await,
        Err(SecretError::NotStored)
    ));
    assert!(manager.delete_profile(id).await.unwrap().secret_deleted);
    assert!(matches!(
        store.get(&row.secret_key).await,
        Err(SecretError::NotStored)
    ));
    assert_eq!(manager.list_profiles().await.unwrap().len(), 1);
    assert!(!manager.list_profiles().await.unwrap()[0].has_credentials);
    fixture.assert_get_only();
}

#[tokio::test]
async fn opnsense_tls_auth_and_status_errors_never_echo_remote_body() {
    let fixture = start_tls_fixture_server(HashMap::from([
        ("/api/diagnostics/firewall/pfStates", Reply::status(403)),
        ("/api/tailscale/status/status", Reply::status(404)),
    ]))
    .await;
    let invalid_cert = OpnsenseClient::new(&fixture.connection(false)).unwrap();
    assert!(matches!(
        invalid_cert.get_system_status().await,
        Err(OpnsenseError::Tls(_))
    ));
    let client = OpnsenseClient::new(&fixture.connection(true)).unwrap();
    client.get_system_status().await.unwrap();
    assert_eq!(
        client.pf_states().await.unwrap_err(),
        OpnsenseError::Forbidden
    );
    assert!(matches!(
        client.tailscale_status().await,
        Err(OpnsenseError::NotFound(_))
    ));
    let mut wrong = fixture.connection(true);
    wrong.api_secret = "wrong-secret".into();
    let err = OpnsenseClient::new(&wrong)
        .unwrap()
        .get_system_status()
        .await
        .unwrap_err();
    assert_eq!(err, OpnsenseError::Unauthorized);
    for err in [
        err.to_string(),
        client.pf_states().await.unwrap_err().to_string(),
        client.tailscale_status().await.unwrap_err().to_string(),
    ] {
        assert!(!err.contains(API_KEY));
        assert!(!err.contains(API_SECRET));
    }
    let requests = fixture.requests();
    assert_eq!(requests.len(), 6);
    assert!(requests.iter().all(|request| request.method == "GET"));
    assert!(
        requests
            .iter()
            .filter(|request| request.authorization.as_deref() == Some(expected_auth().as_str()))
            .count()
            >= 5
    );
}

#[tokio::test]
async fn opnsense_monitor_snapshot_joins_uuid_counters_and_keeps_vpn_independent() {
    let fixture = start_tls_fixture_server(HashMap::new()).await;
    let (_dir, manager, _store, id) = profile_manager(fixture.port).await;
    let monitor = OpnsenseMonitorManager::new(manager.clone());
    let (send, mut receive) = tokio::sync::mpsc::unbounded_channel::<OpnsenseSnapshotDto>();
    monitor
        .start(
            id,
            Arc::new(move |snapshot| {
                let _ = send.send(snapshot);
            }),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    let snapshot = tokio::time::timeout(std::time::Duration::from_secs(5), receive.recv())
        .await
        .expect("first monitor fetch")
        .expect("monitor snapshot");
    monitor.stop(id).await.unwrap();
    assert_eq!(snapshot.profile_id, id);
    let rules = snapshot
        .rules
        .expect("rules refreshed")
        .data
        .expect("rules section");
    assert_eq!(rules.total, 2);
    assert_eq!(rules.rules.len(), 2);
    assert!(!rules.truncated);
    assert_eq!(rules.rules[0].uuid, PASS_UUID);
    assert_eq!(rules.rules[0].packets, Some(12));
    assert_eq!(rules.rules[0].evaluations, Some(21));
    assert_eq!(rules.rules[1].uuid, BLOCK_UUID);
    assert_eq!(rules.rules[1].packets, Some(0));
    assert!(!rules.rules[1].enabled);
    assert!(rules.counters_error.is_none());
    let system = snapshot.system.data.expect("system section");
    assert_eq!(system.interfaces[0].name, "igb0");
    assert_eq!(system.interfaces[0].ipv4, vec!["192.0.2.1/24".to_owned()]);
    assert_eq!(
        system.interfaces[0].media.as_deref(),
        Some("1000baseT <full-duplex>")
    );
    assert_eq!(system.interfaces[0].rx_bytes, Some(99));
    // The lagg parent lists its members with link/media joined from the
    // members' own rows.
    let lagg = system.interfaces[1].lagg.as_ref().expect("lagg details");
    assert_eq!(lagg.proto.as_deref(), Some("lacp"));
    assert_eq!(lagg.members.len(), 2);
    assert_eq!(lagg.members[0].name, "em0");
    assert_eq!(
        lagg.members[0].media.as_deref(),
        Some("10Gbase-SR <full-duplex>")
    );
    assert!(lagg.members[1].media.is_none(), "em1 has no media row");
    assert_eq!(system.pf_states.as_ref().unwrap().current, Some(23));
    // CPU from the SSE interval sample, uptime/load from system_time,
    // memory from system_resources.
    let resources = system.resources.as_ref().expect("resources");
    assert_eq!(
        resources.cpu_usage_percent,
        Some(7.0),
        "second SSE event is the interval sample"
    );
    assert_eq!(resources.uptime.as_deref(), Some("3 days, 04:05:06"));
    assert_eq!(resources.loadavg.as_deref(), Some("0.12, 0.08, 0.06"));
    assert_eq!(resources.mem_used_bytes, Some(2684354560));
    let vpn = snapshot.vpn.data.expect("VPN section");
    let wg = vpn.wireguard.data.unwrap();
    assert_eq!(wg.interfaces[0].name, "Roadwarriors");
    let peer = &wg.interfaces[0].peers[0];
    assert_eq!(peer.status, "stale");
    assert_eq!(peer.endpoint.as_deref(), Some("198.51.100.9:51820"));
    let openvpn = vpn.openvpn.data.unwrap();
    assert_eq!(openvpn.len(), 1);
    assert_eq!(openvpn[0].name, "road-1");
    assert!(openvpn[0].connected);
    assert_eq!(
        openvpn[0].real_address.as_deref(),
        Some("198.51.100.7:1194")
    );
    let ipsec = vpn.ipsec.data.unwrap();
    assert_eq!(ipsec[0].remote_host.as_deref(), Some("203.0.113.1"));
    assert!(ipsec[0].connected);
    assert_eq!(vpn.tailscale.state, "unavailable");
    assert!(vpn.tailscale.data.is_none());
    let requests = fixture.requests();
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.target.starts_with("/api/firewall/filter/searchRule"))
            .count(),
        1,
        "single GET, no paging"
    );
    fixture.assert_get_only();
}

#[tokio::test]
async fn opnsense_rules_reported_total_above_rows_is_flagged_truncated() {
    let static_page = Reply::json(&format!(
        r#"{{"total":3,"rowCount":9999,"current":1,"rows":[{{"uuid":"{PASS_UUID}","action":"pass"}},{{"uuid":"{BLOCK_UUID}","action":"block"}}]}}"#
    ));
    let fixture = start_tls_fixture_server(HashMap::from([(
        "/api/firewall/filter/searchRule",
        static_page,
    )]))
    .await;
    let (_dir, manager, _store, id) = profile_manager(fixture.port).await;
    let monitor = OpnsenseMonitorManager::new(manager);
    let (send, mut receive) = tokio::sync::mpsc::unbounded_channel::<OpnsenseSnapshotDto>();
    monitor
        .start(
            id,
            Arc::new(move |snapshot| {
                let _ = send.send(snapshot);
            }),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    let snapshot = tokio::time::timeout(std::time::Duration::from_secs(5), receive.recv())
        .await
        .expect("first monitor fetch")
        .expect("monitor snapshot");
    monitor.stop(id).await.unwrap();
    let rules = snapshot
        .rules
        .expect("rules refreshed")
        .data
        .expect("rules section");
    assert_eq!(rules.total, 3);
    assert_eq!(rules.rules.len(), 2);
    assert!(rules.truncated);
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|r| r.target.starts_with("/api/firewall/filter/searchRule"))
            .count(),
        1
    );
    fixture.assert_get_only();
}

#[tokio::test]
async fn opnsense_denied_counters_keep_rules_and_denied_plugin_does_not_hide_other_vpns() {
    let fixture = start_tls_fixture_server(HashMap::from([
        ("/api/firewall/filter_util/ruleStats", Reply::status(403)),
        ("/api/tailscale/status/status", Reply::status(403)),
    ]))
    .await;
    let (_dir, manager, _store, id) = profile_manager(fixture.port).await;
    let monitor = OpnsenseMonitorManager::new(manager);
    let (send, mut receive) = tokio::sync::mpsc::unbounded_channel();
    monitor
        .start(
            id,
            Arc::new(move |snapshot| {
                let _ = send.send(snapshot);
            }),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    let snapshot: OpnsenseSnapshotDto =
        tokio::time::timeout(std::time::Duration::from_secs(5), receive.recv())
            .await
            .expect("first monitor fetch")
            .expect("snapshot");
    monitor.stop(id).await.unwrap();
    let rules = snapshot.rules.expect("rules refreshed").data.unwrap();
    assert_eq!(rules.rules.len(), 2);
    assert_eq!(rules.rules[0].packets, None);
    assert_eq!(rules.counters_error.unwrap().kind, "forbidden");
    let vpn = snapshot.vpn.data.unwrap();
    assert_eq!(vpn.tailscale.state, "access-denied");
    assert!(vpn.ipsec.data.unwrap()[0].connected);
    fixture.assert_get_only();
}

#[tokio::test]
async fn opnsense_check_updates_compares_installed_with_cached_changelog() {
    let fixture = start_tls_fixture_server(HashMap::new()).await;
    let (_dir, manager, _store, id) = profile_manager(fixture.port).await;
    let firmware = OpnsenseFirmwareManager::new(manager);
    let result = firmware.check(id).await.unwrap();
    assert_eq!(result.state, "update-available");
    assert_eq!(result.installed_version.as_deref(), Some("26.7.6_3"));
    assert_eq!(result.abi.as_deref(), Some("26.7"));
    assert_eq!(result.series.as_deref(), Some("26.7"));
    // Future series and release candidates never count as current-series
    // production updates; the installed package revision is ignored.
    assert_eq!(result.latest_version.as_deref(), Some("26.7.7"));
    assert_eq!(result.changelog.len(), 1);
    assert_eq!(result.changelog[0].version, "26.7.7");
    assert_eq!(result.changelog[0].date.as_deref(), Some("2026-10-13"));
    // Installed plugins ride along (os-nginx not installed → filtered).
    assert_eq!(result.plugins.len(), 1);
    assert_eq!(result.plugins[0].name, "os-tailscale");
    assert_eq!(
        result.plugins[0].cached_upgrade_version.as_deref(),
        Some("1.5")
    );
    fixture.assert_get_only();
    assert!(
        fixture
            .requests()
            .iter()
            .all(|r| r.target == "/api/core/firmware/info"),
        "the check must not call a remote CDN or trigger firewall updates"
    );
}

#[tokio::test]
async fn opnsense_check_updates_reports_up_to_date_and_missing_cache() {
    let up_to_date = start_tls_fixture_server(HashMap::from([(
        "/api/core/firmware/info",
        Reply::json(
            r#"{"product_version":"26.7.6_3","product":{"CORE_ABI":"26.7"},"changelog":[{"series":"26.7","version":"26.7.6","date":"2026-10-08"}]}"#,
        ),
    )]))
    .await;
    let (_dir, manager, _store, id) = profile_manager(up_to_date.port).await;
    let result = OpnsenseFirmwareManager::new(manager)
        .check(id)
        .await
        .unwrap();
    assert_eq!(result.state, "up-to-date");
    assert_eq!(result.latest_version.as_deref(), Some("26.7.6"));
    assert!(result.changelog.is_empty());
    up_to_date.assert_get_only();

    // No cached changelog → unknown, never "up to date".
    let empty = start_tls_fixture_server(HashMap::from([(
        "/api/core/firmware/info",
        Reply::json(
            r#"{"product_version":"26.7.6_3","product":{"CORE_ABI":"26.7"},"changelog":[]}"#,
        ),
    )]))
    .await;
    let (_dir, manager, _store, id) = profile_manager(empty.port).await;
    let result = OpnsenseFirmwareManager::new(manager)
        .check(id)
        .await
        .unwrap();
    assert_eq!(result.state, "unknown");
    assert_eq!(result.installed_version.as_deref(), Some("26.7.6_3"));
    assert!(result
        .message
        .unwrap()
        .contains("No cached release changelog"));
    empty.assert_get_only();
}

#[tokio::test]
async fn opnsense_backup_downloads_latest_saved_revision_as_age_without_plaintext_file() {
    let fixture = start_tls_fixture_server(HashMap::new()).await;
    let (dir, manager, _store, id) = profile_manager(fixture.port).await;
    let backup = OpnsenseBackupManager::new(&manager);
    let revisions = backup.list_revisions(id).await.unwrap();
    assert_eq!(revisions[0].id, "config-newer.xml");
    assert_eq!(revisions[1].id, "config-older.xml");
    let passphrase = "fixture-backup-passphrase";
    let result = backup
        .backup(
            id,
            dir.path().to_str().unwrap(),
            "saved-revision",
            passphrase,
            None,
        )
        .await
        .unwrap();
    assert_eq!(result.source_revision_id, "config-newer.xml");
    assert_eq!(
        result.source_time_iso.as_deref(),
        Some("2025-01-02T00:00:00Z")
    );
    assert!(result.path.ends_with("saved-revision.xml.age"));
    let ciphertext = std::fs::read(&result.path).unwrap();
    assert!(!ciphertext
        .windows(XML.len())
        .any(|chunk| chunk == XML.as_bytes()));
    let decryptor = age::Decryptor::new(std::io::BufReader::new(
        std::fs::File::open(&result.path).unwrap(),
    ))
    .unwrap();
    let identity = age::scrypt::Identity::new(age::secrecy::SecretString::new(
        passphrase.to_owned().into(),
    ));
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .unwrap();
    let mut plaintext = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut plaintext).unwrap();
    assert_eq!(plaintext, XML.as_bytes());
    assert!(!dir.path().join("saved-revision.xml").exists());
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
        let path = entry.unwrap().path();
        path.extension().is_none_or(|ext| ext != "xml")
    }));
    assert!(matches!(
        backup
            .backup(
                id,
                dir.path().to_str().unwrap(),
                "saved-revision",
                passphrase,
                None,
            )
            .await,
        Err(OpnsenseBackupError::OutputExists(_))
    ));
    let targets: Vec<_> = fixture.requests().into_iter().map(|r| r.target).collect();
    assert_eq!(
        targets,
        [
            "/api/core/backup/backups/this",
            "/api/core/backup/backups/this",
            "/api/core/backup/download/this/config-newer.xml",
            "/api/core/backup/backups/this",
            "/api/core/backup/download/this/config-newer.xml",
        ]
    );
    fixture.assert_get_only();
}

#[tokio::test]
async fn opnsense_backup_rejects_html_and_oversize_without_writing_files() {
    for reply in [
        Reply::json("<html>not a config</html>"),
        Reply {
            status: 200,
            content_type: "application/xml",
            body: vec![b'x'; MAX_REVISION_BYTES + 1],
        },
    ] {
        let fixture = start_tls_fixture_server(HashMap::from([(
            "/api/core/backup/download/this/config-newer.xml",
            reply,
        )]))
        .await;
        let (dir, manager, _store, id) = profile_manager(fixture.port).await;
        let error = OpnsenseBackupManager::new(&manager)
            .backup(
                id,
                dir.path().to_str().unwrap(),
                "bad",
                "fixture-backup-passphrase",
                None,
            )
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                OpnsenseBackupError::InvalidConfig(_)
                    | OpnsenseBackupError::Api(OpnsenseError::TooLarge(_))
            ),
            "{error:?}"
        );
        assert!(!dir.path().join("bad.xml.age").exists());
        fixture.assert_get_only();
    }
}

#[tokio::test]
async fn opnsense_sse_stream_is_authenticated_get() {
    use futures_util::StreamExt as _;
    let fixture = start_tls_fixture_server(HashMap::new()).await;
    let client = OpnsenseClient::new(&fixture.connection(true)).unwrap();
    let mut stream = client.open_log_stream().await.unwrap();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        body.extend_from_slice(&chunk.unwrap());
    }
    assert_eq!(
        body,
        b"data: {\"action\":\"block\",\"src\":\"192.0.2.2\"}\n\n"
    );
    assert_eq!(fixture.requests().len(), 1);
    fixture.assert_get_only();
}
