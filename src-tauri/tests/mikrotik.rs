//! Integration tests for the MikroTik REST client against a LOCAL wiremock
//! server, covering the plan's connection contract: happy path → typed DTO,
//! 401 → Unauthorized (never UnsupportedVersion), 404 on the resource probe
//! → UnsupportedVersion, connect drop → Connect, HTML/malformed bodies →
//! Parse, plus a LOCAL TLS server with an rcgen self-signed certificate for
//! the allow_invalid_certs true/false contract.

use std::sync::Arc;
use std::time::Duration;

use verkkokyyla_lib::mikrotik::client::{MikrotikClient, MikrotikConnection};
use verkkokyyla_lib::mikrotik::error::MikrotikError;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const RESOURCE_FIXTURE: &str = r#"{
    "architecture-name": "arm64",
    "board-name": "RB4011iGS+",
    "cpu-load": "12",
    "free-memory": "1048576000",
    "total-memory": "2097152000",
    "uptime": "3d 04:12:33",
    "version": "7.18.2"
}"#;

fn conn_for(server: &MockServer) -> MikrotikConnection {
    let port: u16 = server
        .uri()
        .trim_start_matches("http://")
        .rsplit(':')
        .next()
        .expect("port in uri")
        .parse()
        .expect("numeric port");
    MikrotikConnection {
        host: "127.0.0.1".to_owned(),
        port,
        use_tls: false,
        allow_invalid_certs: false,
        username: "admin".to_owned(),
        password: "s3cr3t".to_owned(),
    }
}

fn basic_auth_value() -> String {
    use base64::Engine as _;
    format!(
        "Basic {}",
        base64::engine::general_purpose::STANDARD.encode("admin:s3cr3t")
    )
}

#[tokio::test]
async fn mikrotik_client_resource_happy_path_returns_typed_dto() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/system/resource"))
        .and(header("Authorization", basic_auth_value()))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(RESOURCE_FIXTURE)
                .insert_header("content-type", "application/json"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let client = MikrotikClient::new(&conn_for(&server)).unwrap();
    let dto = client.get_resource().await.unwrap();
    assert_eq!(dto.board_name.as_deref(), Some("RB4011iGS+"));
    assert_eq!(dto.version.as_deref(), Some("7.18.2"));
    assert_eq!(dto.cpu_load, Some(12.0));
    assert_eq!(dto.mem_total_bytes, Some(2_097_152_000));
    assert_eq!(dto.mem_used_bytes, Some(2_097_152_000 - 1_048_576_000));
}

#[tokio::test]
async fn mikrotik_client_interfaces_come_from_print_stats_post() {
    let server = MockServer::start().await;
    // No GET /rest/interface mock is registered: a bare-GET client would get
    // wiremock's 404 and fail, so this mock ONLY satisfies the print-stats
    // POST contract. The body matchers lock `stats` and the counter proplist.
    Mock::given(method("POST"))
        .and(path("/rest/interface/print"))
        .and(body_string_contains("\"stats\""))
        .and(body_string_contains("tx-queue-drop"))
        .and(header("Authorization", basic_auth_value()))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                ".id": "*1",
                "name": "ether1",
                "type": "ether",
                "running": "true",
                "disabled": "false",
                "rx-byte": "1000000",
                "tx-byte": "2000000",
                "rx-packet": "9000",
                "tx-packet": "8000",
                "tx-queue-drop": "3",
                "link-downs": "2",
                "rx-error": "11",
                "tx-error": "12",
                "rx-drop": "13"
            }
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let client = MikrotikClient::new(&conn_for(&server)).unwrap();
    let ifaces = client.get_interfaces().await.unwrap();
    assert_eq!(ifaces.len(), 1);
    assert_eq!(ifaces[0].name, "ether1");
    assert_eq!(ifaces[0].rx_byte, Some(1_000_000));
    assert_eq!(ifaces[0].tx_queue_drop, Some(3));
    assert_eq!(ifaces[0].link_downs, Some(2));
    assert_eq!(ifaces[0].rx_error, Some(11));
    assert_eq!(ifaces[0].rx_drop, Some(13));
    assert_eq!(ifaces[0].rx_fcs_error, None);
}

#[tokio::test]
async fn mikrotik_client_401_is_unauthorized_never_unsupported_version() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/system/resource"))
        .respond_with(
            ResponseTemplate::new(401).set_body_string(r#"{"detail":"Invalid username/password"}"#),
        )
        .mount(&server)
        .await;

    let client = MikrotikClient::new(&conn_for(&server)).unwrap();
    let err = client.get_resource().await.unwrap_err();
    assert!(matches!(err, MikrotikError::Unauthorized));
    assert!(!matches!(err, MikrotikError::UnsupportedVersion(_)));
}

#[tokio::test]
async fn mikrotik_client_404_resource_probe_is_unsupported_version_http() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/system/resource"))
        .respond_with(ResponseTemplate::new(404).set_body_string("not found"))
        .mount(&server)
        .await;

    let client = MikrotikClient::new(&conn_for(&server)).unwrap();
    let err = client.get_resource().await.unwrap_err();
    match err {
        MikrotikError::UnsupportedVersion(message) => {
            assert!(message.contains("v7.9+"), "message: {message}");
            assert!(message.contains("www service"), "message: {message}");
        }
        other => panic!("expected UnsupportedVersion, got {other:?}"),
    }
}

#[tokio::test]
async fn mikrotik_client_html_error_page_is_parse_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/system/health"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<html><body>RouterOS www error page</body></html>")
                .insert_header("content-type", "text/html"),
        )
        .mount(&server)
        .await;

    let client = MikrotikClient::new(&conn_for(&server)).unwrap();
    let err = client.get_health().await.unwrap_err();
    assert!(matches!(err, MikrotikError::Parse(_)), "got {err:?}");
}

#[tokio::test]
async fn mikrotik_client_malformed_json_is_parse_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/system/resource"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("{")
                .insert_header("content-type", "application/json"),
        )
        .mount(&server)
        .await;

    let client = MikrotikClient::new(&conn_for(&server)).unwrap();
    let err = client.get_resource().await.unwrap_err();
    assert!(matches!(err, MikrotikError::Parse(_)), "got {err:?}");
}

#[tokio::test]
async fn mikrotik_client_connection_refused_is_connect_error() {
    // Bind then immediately release a port so nothing is listening: the
    // connection is refused/dropped at the transport level.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut conn = conn_for(&MockServer::start().await);
    conn.port = port;
    let client = MikrotikClient::new(&conn).unwrap();
    let err = client.get_resource().await.unwrap_err();
    assert!(matches!(err, MikrotikError::Connect(_)), "got {err:?}");
    assert!(!matches!(err, MikrotikError::Timeout(_)));
}

#[tokio::test]
async fn mikrotik_client_slow_response_past_default_is_timeout() {
    let server = MockServer::start().await;
    // 11s mock delay vs the 10s request timeout (real time).
    Mock::given(method("GET"))
        .and(path("/rest/system/resource"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(11)))
        .mount(&server)
        .await;

    let client = MikrotikClient::new(&conn_for(&server)).unwrap();
    let err = client.get_resource().await.unwrap_err();
    assert!(matches!(err, MikrotikError::Timeout(_)), "got {err:?}");
}

#[tokio::test]
async fn mikrotik_client_update_status_absent_latest_version_is_none() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/system/package/update"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "installed-version": "7.19",
            "channel": "stable",
            "status": "System is already up to date"
        })))
        .mount(&server)
        .await;

    let client = MikrotikClient::new(&conn_for(&server)).unwrap();
    let dto = client.get_update_status().await.unwrap();
    assert_eq!(dto.installed_version.as_deref(), Some("7.19"));
    // Absent latest-version must be None — "unknown", never "up to date".
    assert_eq!(dto.latest_version, None);
}

#[tokio::test]
async fn mikrotik_client_check_for_updates_uses_last_progressive_section() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/rest/system/package/update/check-for-updates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            { "status": "finding out latest version..." },
            {
                "installed-version": "7.18.2",
                "latest-version": "7.19",
                "channel": "stable",
                "status": "New version is available"
            }
        ])))
        .expect(1)
        .mount(&server)
        .await;

    let client = MikrotikClient::new(&conn_for(&server)).unwrap();
    let dto = client.check_for_updates().await.unwrap();
    assert_eq!(dto.installed_version.as_deref(), Some("7.18.2"));
    assert_eq!(dto.latest_version.as_deref(), Some("7.19"));
    assert_eq!(dto.status.as_deref(), Some("New version is available"));
}

#[tokio::test]
async fn mikrotik_client_delete_file_resolves_id_raw_first_then_encoded() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/file"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            { ".id": "*9", "name": "other.rsc", "size": "10" },
            { ".id": "*1", "name": "verkkokyyla-20260906-120000.backup", "size": "1048576" }
        ])))
        .expect(1)
        .mount(&server)
        .await;
    // Raw path tried first and rejected (tightened encoding rules)...
    Mock::given(method("DELETE"))
        .and(path("/rest/file/*1"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    // ...then the encoded fallback succeeds. Both orderings locked by mocks.
    Mock::given(method("DELETE"))
        .and(path("/rest/file/%2A1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let client = MikrotikClient::new(&conn_for(&server)).unwrap();
    client
        .delete_file("verkkokyyla-20260906-120000.backup")
        .await
        .unwrap();
}

#[tokio::test]
async fn mikrotik_client_delete_file_missing_name_is_file_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rest/file"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            { ".id": "*9", "name": "other.rsc", "size": "10" }
        ])))
        .mount(&server)
        .await;

    let client = MikrotikClient::new(&conn_for(&server)).unwrap();
    let err = client.delete_file("missing.backup").await.unwrap_err();
    assert!(matches!(err, MikrotikError::FileNotFound(_)), "got {err:?}");
}

// ---------------------------------------------------------------------------
// Local TLS fixture server (rcgen self-signed, per the connection contract)
// ---------------------------------------------------------------------------

use rcgen::generate_simple_self_signed;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

/// A minimal HTTP/1.1 responder over TLS that always serves the resource
/// fixture. Enough to prove the certificate-validation contract; wiremock
/// covers the REST semantics.
async fn start_tls_fixture_server() -> (u16, tokio::task::JoinHandle<()>) {
    let certified =
        generate_simple_self_signed(vec!["localhost".to_owned(), "127.0.0.1".to_owned()])
            .expect("generate self-signed cert");
    let key_der =
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der()));
    let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("protocol versions")
    .with_no_client_auth()
    .with_single_cert(vec![certified.cert.der().clone()], key_der)
    .expect("server config");
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let handle = tokio::spawn(async move {
        loop {
            let (socket, _) = match listener.accept().await {
                Ok(pair) => pair,
                Err(_) => continue,
            };
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let mut tls = match acceptor.accept(socket).await {
                    Ok(stream) => stream,
                    Err(_) => return,
                };
                // Read request headers (best effort, capped).
                let mut header_buf = Vec::new();
                let mut tmp = [0u8; 1024];
                loop {
                    if header_buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                    match tls.read(&mut tmp).await {
                        Ok(0) => return,
                        Ok(n) => header_buf.extend_from_slice(&tmp[..n]),
                        Err(_) => return,
                    }
                    if header_buf.len() > 16384 {
                        return;
                    }
                }
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                    RESOURCE_FIXTURE.len(),
                    RESOURCE_FIXTURE
                );
                if tls.write_all(response.as_bytes()).await.is_err() {
                    return;
                }
                let _ = tls.shutdown().await;
            });
        }
    });
    (port, handle)
}

fn tls_conn(port: u16, allow_invalid_certs: bool) -> MikrotikConnection {
    MikrotikConnection {
        host: "127.0.0.1".to_owned(),
        port,
        use_tls: true,
        allow_invalid_certs,
        username: "admin".to_owned(),
        password: "s3cr3t".to_owned(),
    }
}

#[tokio::test]
async fn mikrotik_client_tls_self_signed_rejected_without_override() {
    let (port, handle) = start_tls_fixture_server().await;
    let client = MikrotikClient::new(&tls_conn(port, false)).unwrap();
    let err = client.get_resource().await.unwrap_err();
    assert!(matches!(err, MikrotikError::Tls(_)), "got {err:?}");
    handle.abort();
}

#[tokio::test]
async fn mikrotik_client_tls_self_signed_accepted_with_override() {
    let (port, handle) = start_tls_fixture_server().await;
    let client = MikrotikClient::new(&tls_conn(port, true)).unwrap();
    let dto = client.get_resource().await.unwrap();
    assert_eq!(dto.board_name.as_deref(), Some("RB4011iGS+"));
    assert_eq!(dto.version.as_deref(), Some("7.18.2"));
    handle.abort();
}
