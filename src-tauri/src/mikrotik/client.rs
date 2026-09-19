//! Per-profile MikroTik REST client (RouterOS v7.1+).
//!
//! A fresh reqwest client is built per connection (http_client.rs has no
//! per-client certificate override, so it cannot be reused): rustls TLS,
//! `danger_accept_invalid_certs` per the profile, 5s connect timeout, 10s
//! default request timeout (RouterOS's REST ceiling is 60s, used for the
//! long-running command endpoints), HTTP Basic auth from the profile
//! username and the keyring-fetched password. The password is never logged
//! and never included in any error message.

use std::time::Duration;

use base64::Engine as _;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::{json, Value};

use crate::mikrotik::error::{classify_transport, MikrotikError};
use crate::mikrotik::parse::{
    parse_bonding, parse_bridge_vlans, parse_ethernet_monitor, parse_ethernet_stats, parse_files,
    parse_health, parse_interfaces, parse_log_entries, parse_resource, parse_routerboard,
    parse_update_status, parse_vlans, BondingDto, BridgeVlanDto, EthernetMonitorDto,
    EthernetStatsDto, FileEntryDto, InterfaceDto, LogEntryDto, ResourceDto, RouterboardDto,
    SensorDto, UpdateStatusDto, VlanDto,
};
use crate::mikrotik::types::MikrotikApi;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
/// RouterOS's REST server ceiling; backup/export/update commands legitimately
/// exceed the 10s default, so they use this dedicated command timeout.
pub const COMMAND_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Connection parameters for one MikroTik profile. The password arrives
/// keyring-fetched from the caller (todo 4 owns the secret store); it is
/// used only to build the Basic auth header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MikrotikConnection {
    pub host: String,
    pub port: u16,
    pub use_tls: bool,
    pub allow_invalid_certs: bool,
    pub username: String,
    pub password: String,
}

/// How the response status is interpreted by the shared handler.
enum Probe {
    Plain,
    /// `/rest/system/resource`: a 404 here means the RouterOS version
    /// predates REST support (message split by transport scheme).
    Resource,
}

pub struct MikrotikClient {
    client: reqwest::Client,
    base_url: String,
    auth_header: String,
    use_tls: bool,
}

impl MikrotikClient {
    /// Build a fresh client for a profile. `allow_invalid_certs` maps to
    /// reqwest's `danger_accept_invalid_certs`.
    pub fn new(conn: &MikrotikConnection) -> Result<Self, MikrotikError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(DEFAULT_REQUEST_TIMEOUT)
            .danger_accept_invalid_certs(conn.allow_invalid_certs)
            .build()
            .map_err(|e| MikrotikError::Connect(e.to_string()))?;
        let scheme = if conn.use_tls { "https" } else { "http" };
        let base_url = format!("{scheme}://{}:{}/rest", conn.host, conn.port);
        let credentials = format!("{}:{}", conn.username, conn.password);
        let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
        Ok(Self {
            client,
            base_url,
            auth_header: format!("Basic {encoded}"),
            use_tls: conn.use_tls,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base_url, path.trim_start_matches('/'))
    }

    async fn get(&self, path: &str, probe: Probe) -> Result<Value, MikrotikError> {
        let response = self
            .client
            .get(self.url(path))
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .await
            .map_err(|e| classify_transport(&e))?;
        self.handle_response(response, probe).await
    }

    async fn post(
        &self,
        path: &str,
        body: Value,
        timeout: Duration,
    ) -> Result<Value, MikrotikError> {
        // reqwest is built without the `json` feature: serialize manually.
        let payload = serde_json::to_vec(&body)
            .map_err(|e| MikrotikError::Parse(format!("serialize request body: {e}")))?;
        let response = self
            .client
            .post(self.url(path))
            .header(AUTHORIZATION, &self.auth_header)
            .header(CONTENT_TYPE, "application/json")
            .timeout(timeout)
            .body(payload)
            .send()
            .await
            .map_err(|e| classify_transport(&e))?;
        self.handle_response(response, Probe::Plain).await
    }

    async fn handle_response(
        &self,
        response: reqwest::Response,
        probe: Probe,
    ) -> Result<Value, MikrotikError> {
        let status = response.status().as_u16();
        // Read the body even for error statuses: RouterOS explains failures
        // in the body (as JSON, HTML, or plain text). Snippet only.
        let body = response.text().await.map_err(|e| classify_transport(&e))?;
        match status {
            200..=299 => {
                if body.trim().is_empty() {
                    // Command endpoints (backup/save, /export) return an
                    // empty result on success.
                    return Ok(Value::Null);
                }
                serde_json::from_str(&body).map_err(|_| {
                    MikrotikError::Parse(format!("response is not valid JSON: {}", snippet(&body)))
                })
            }
            401 => Err(MikrotikError::Unauthorized),
            403 => Err(MikrotikError::Forbidden),
            404 => match probe {
                Probe::Resource => Err(MikrotikError::UnsupportedVersion(
                    version_requirement_message(self.use_tls).to_owned(),
                )),
                Probe::Plain => Err(MikrotikError::Api {
                    status,
                    message: snippet(&body),
                }),
            },
            _ => Err(MikrotikError::Api {
                status,
                message: snippet(&body),
            }),
        }
    }

    /// `GET /rest/system/resource`.
    pub async fn get_resource(&self) -> Result<ResourceDto, MikrotikError> {
        let value = self.get("system/resource", Probe::Resource).await?;
        parse_resource(&value)
    }

    /// `GET /rest/system/health` — property-set or sensor-record shape.
    pub async fn get_health(&self) -> Result<Vec<SensorDto>, MikrotikError> {
        let value = self.get("system/health", Probe::Plain).await?;
        parse_health(&value)
    }

    /// Counter-bearing interface data. A bare `GET /rest/interface` is
    /// metadata-only, so this MUST be the `print` command with `stats`.
    pub async fn get_interfaces(&self) -> Result<Vec<InterfaceDto>, MikrotikError> {
        let value = self
            .post(
                "interface/print",
                json!({
                    "stats": "",
                    ".proplist": "name,comment,type,running,disabled,rx-byte,tx-byte,rx-packet,\
                        tx-packet,tx-queue-drop,link-downs,rx-error,tx-error,rx-drop"
                }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await?;
        parse_interfaces(&value)
    }

    /// Stats-detail union (per-interface detail counters).
    pub async fn get_interface_stats_detail(&self) -> Result<Vec<InterfaceDto>, MikrotikError> {
        let value = self
            .post(
                "interface/print",
                json!({
                    "stats-detail": "",
                    ".proplist": "name,rx-error,tx-error,rx-drop,link-downs,rx-error-events,\
                        tx-error-events,rx-fcs-error,rx-align-error,tx-collision,tx-drop"
                }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await?;
        parse_interfaces(&value)
    }

    /// DRIVER-level error counters — they live in the ethernet stats print,
    /// NOT in the ethernet monitor output.
    pub async fn get_ethernet_stats(&self) -> Result<Vec<EthernetStatsDto>, MikrotikError> {
        let value = self
            .post(
                "interface/ethernet/print",
                json!({
                    "stats": "",
                    ".proplist": "name,default-name,rx-error-events,tx-error-events,\
                        rx-fcs-error,rx-align-error,tx-collision,tx-drop"
                }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await?;
        parse_ethernet_stats(&value)
    }

    /// Negotiated link rate/duplex for one ethernet interface
    /// (`{"numbers": name, "once": ""}`).
    pub async fn get_ethernet_monitor(
        &self,
        name: &str,
    ) -> Result<Vec<EthernetMonitorDto>, MikrotikError> {
        let value = self
            .post(
                "interface/ethernet/monitor",
                json!({ "numbers": name, "once": "" }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await?;
        parse_ethernet_monitor(&value)
    }

    /// `GET /rest/interface/vlan`.
    pub async fn get_vlans(&self) -> Result<Vec<VlanDto>, MikrotikError> {
        let value = self.get("interface/vlan", Probe::Plain).await?;
        parse_vlans(&value)
    }

    /// `GET /rest/interface/bridge/vlan`.
    pub async fn get_bridge_vlans(&self) -> Result<Vec<BridgeVlanDto>, MikrotikError> {
        let value = self.get("interface/bridge/vlan", Probe::Plain).await?;
        parse_bridge_vlans(&value)
    }

    /// `GET /rest/interface/bonding` — bonding masters and their slave
    /// ports. Boards without any bonding print return an empty array.
    pub async fn get_bonding(&self) -> Result<Vec<BondingDto>, MikrotikError> {
        let value = self.get("interface/bonding", Probe::Plain).await?;
        parse_bonding(&value)
    }

    /// `GET /rest/system/package/update`.
    pub async fn get_update_status(&self) -> Result<UpdateStatusDto, MikrotikError> {
        let value = self.get("system/package/update", Probe::Plain).await?;
        parse_update_status(&value)
    }

    /// `POST /rest/system/package/update/check-for-updates` — streams
    /// progressive status sections and may legitimately run past the 10s
    /// default, so it uses the 60s command timeout. The LAST section of a
    /// progressive array is the current state.
    pub async fn check_for_updates(&self) -> Result<UpdateStatusDto, MikrotikError> {
        let value = self
            .post(
                "system/package/update/check-for-updates",
                json!({}),
                COMMAND_REQUEST_TIMEOUT,
            )
            .await?;
        parse_update_status(&value)
    }

    /// `GET /rest/system/routerboard`.
    pub async fn get_routerboard(&self) -> Result<RouterboardDto, MikrotikError> {
        let value = self.get("system/routerboard", Probe::Plain).await?;
        parse_routerboard(&value)
    }

    /// `POST /rest/system/backup/save` — creates `<name>.backup` on the
    /// router (60s command timeout; generation legitimately exceeds 10s).
    pub async fn backup_save(
        &self,
        name: &str,
        password: Option<&str>,
    ) -> Result<(), MikrotikError> {
        let mut body = json!({ "name": name });
        if let Some(password) = password {
            body["password"] = Value::String(password.to_owned());
        }
        self.post("system/backup/save", body, COMMAND_REQUEST_TIMEOUT)
            .await?;
        Ok(())
    }

    /// `POST /rest/export` with `file=` — creates `<name>.rsc` on the router.
    pub async fn export_rsc(&self, name: &str) -> Result<(), MikrotikError> {
        self.post("export", json!({ "file": name }), COMMAND_REQUEST_TIMEOUT)
            .await?;
        Ok(())
    }

    /// `GET /rest/file`.
    pub async fn list_files(&self) -> Result<Vec<FileEntryDto>, MikrotikError> {
        let value = self.get("file", Probe::Plain).await?;
        parse_files(&value)
    }

    /// `POST /rest/log/print` — in-memory log entries (`.id,time,topics,
    /// message`). REST offers no streaming, so the log stream runtime polls
    /// this and dedupes by record id.
    pub async fn get_log(&self) -> Result<Vec<LogEntryDto>, MikrotikError> {
        let value = self
            .post(
                "log/print",
                json!({ ".proplist": ".id,time,topics,message" }),
                DEFAULT_REQUEST_TIMEOUT,
            )
            .await?;
        parse_log_entries(&value)
    }

    /// Resolve a file NAME to its record id via `/rest/file`, then DELETE by
    /// id. RouterOS record ids contain `*` and official REST examples use the
    /// RAW id (`/rest/file/*1`); percent-encoding rules were tightened only
    /// in later RouterOS versions, so the RAW path is tried first and a 404
    /// retries once with the encoded form (`%2A1`).
    pub async fn delete_file(&self, name: &str) -> Result<(), MikrotikError> {
        let files = self.list_files().await?;
        let id = files
            .iter()
            .find(|f| f.name.as_deref() == Some(name))
            .and_then(|f| f.id.clone())
            .ok_or_else(|| MikrotikError::FileNotFound(name.to_owned()))?;

        let raw_url = self.url(&format!("file/{id}"));
        let response = self
            .client
            .delete(&raw_url)
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .await
            .map_err(|e| classify_transport(&e))?;
        let status = response.status().as_u16();
        if (200..=299).contains(&status) {
            return Ok(());
        }
        let body = response.text().await.map_err(|e| classify_transport(&e))?;
        if status != 404 {
            return Err(MikrotikError::Api {
                status,
                message: snippet(&body),
            });
        }

        // Raw form rejected (tightened percent-encoding rules): retry once
        // with the `*` percent-encoded.
        let encoded_url = self.url(&format!("file/{}", encode_record_id(&id)));
        let response = self
            .client
            .delete(encoded_url)
            .header(AUTHORIZATION, &self.auth_header)
            .send()
            .await
            .map_err(|e| classify_transport(&e))?;
        let status = response.status().as_u16();
        if (200..=299).contains(&status) {
            return Ok(());
        }
        let body = response.text().await.map_err(|e| classify_transport(&e))?;
        Err(MikrotikError::Api {
            status,
            message: snippet(&body),
        })
    }
}

/// Percent-encode a RouterOS record id for the fallback delete path: only
/// the `*` needs encoding (`*1` -> `%2A1`); the alphanumerics pass through.
fn encode_record_id(id: &str) -> String {
    id.replace('*', "%2A")
}

#[async_trait::async_trait]
impl MikrotikApi for MikrotikClient {
    async fn get_resource(&self) -> Result<ResourceDto, MikrotikError> {
        MikrotikClient::get_resource(self).await
    }

    async fn get_interfaces(&self) -> Result<Vec<InterfaceDto>, MikrotikError> {
        MikrotikClient::get_interfaces(self).await
    }

    async fn get_health(&self) -> Result<Vec<SensorDto>, MikrotikError> {
        MikrotikClient::get_health(self).await
    }

    async fn get_interface_stats_detail(&self) -> Result<Vec<InterfaceDto>, MikrotikError> {
        MikrotikClient::get_interface_stats_detail(self).await
    }

    async fn get_ethernet_stats(&self) -> Result<Vec<EthernetStatsDto>, MikrotikError> {
        MikrotikClient::get_ethernet_stats(self).await
    }

    async fn get_ethernet_monitor(
        &self,
        name: &str,
    ) -> Result<Vec<EthernetMonitorDto>, MikrotikError> {
        MikrotikClient::get_ethernet_monitor(self, name).await
    }

    async fn get_vlans(&self) -> Result<Vec<VlanDto>, MikrotikError> {
        MikrotikClient::get_vlans(self).await
    }

    async fn get_bridge_vlans(&self) -> Result<Vec<BridgeVlanDto>, MikrotikError> {
        MikrotikClient::get_bridge_vlans(self).await
    }

    async fn get_bonding(&self) -> Result<Vec<BondingDto>, MikrotikError> {
        MikrotikClient::get_bonding(self).await
    }

    async fn get_update_status(&self) -> Result<UpdateStatusDto, MikrotikError> {
        MikrotikClient::get_update_status(self).await
    }

    async fn check_for_updates(&self) -> Result<UpdateStatusDto, MikrotikError> {
        MikrotikClient::check_for_updates(self).await
    }

    async fn get_routerboard(&self) -> Result<RouterboardDto, MikrotikError> {
        MikrotikClient::get_routerboard(self).await
    }

    async fn get_log(&self) -> Result<Vec<LogEntryDto>, MikrotikError> {
        MikrotikClient::get_log(self).await
    }
}

/// RouterOS's version-requirement hint, split by transport scheme: a 404 on
/// the resource probe over HTTPS means the www-ssl service / v7.1+ is
/// missing; over plain HTTP it means the `www` service on v7.9+.
fn version_requirement_message(use_tls: bool) -> &'static str {
    if use_tls {
        "RouterOS v7.1+ with the www-ssl service required"
    } else {
        "plain HTTP REST requires RouterOS v7.9+ and the www service — or enable HTTPS/www-ssl"
    }
}

/// Truncate an arbitrary response body for error messages (untrusted
/// external text must not balloon error strings).
fn snippet(body: &str) -> String {
    const LIMIT: usize = 160;
    let trimmed = body.trim();
    if trimmed.chars().count() <= LIMIT {
        trimmed.to_owned()
    } else {
        format!("{}…", trimmed.chars().take(LIMIT).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(use_tls: bool) -> MikrotikConnection {
        MikrotikConnection {
            host: "192.0.2.10".to_owned(),
            port: if use_tls { 443 } else { 80 },
            use_tls,
            allow_invalid_certs: false,
            username: "admin".to_owned(),
            password: "s3cr3t".to_owned(),
        }
    }

    #[test]
    fn mikrotik_client_builds_base_url_per_scheme() {
        let https = MikrotikClient::new(&conn(true)).unwrap();
        assert!(https.base_url.starts_with("https://192.0.2.10:443/rest"));
        let http = MikrotikClient::new(&conn(false)).unwrap();
        assert!(http.base_url.starts_with("http://192.0.2.10:80/rest"));
    }

    #[test]
    fn mikrotik_client_basic_auth_header_exact() {
        let client = MikrotikClient::new(&conn(true)).unwrap();
        // base64("admin:s3cr3t") — verified against the RFC 7617 format.
        assert_eq!(
            client.auth_header,
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode("admin:s3cr3t")
            )
        );
    }

    #[test]
    fn mikrotik_client_password_never_in_error_text() {
        // The password only feeds the Basic auth header; every error path
        // must stay free of it (MikrotikClient has no Debug derive for the
        // same reason — it holds the auth header).
        let page = snippet("<html>500 internal error page from router</html>");
        assert!(!page.contains("s3cr3t"));
        let api = MikrotikError::Api {
            status: 500,
            message: page,
        };
        let text = api.to_string();
        assert!(text.contains("500"));
        assert!(!text.contains("s3cr3t"));
    }

    #[test]
    fn mikrotik_client_record_id_encoding_fallback() {
        assert_eq!(encode_record_id("*1"), "%2A1");
        assert_eq!(encode_record_id("*10"), "%2A10");
        assert_eq!(encode_record_id("ether1"), "ether1");
    }

    #[test]
    fn mikrotik_client_version_message_split_by_transport() {
        assert_eq!(
            version_requirement_message(true),
            "RouterOS v7.1+ with the www-ssl service required"
        );
        let http_msg = version_requirement_message(false);
        assert!(http_msg.contains("v7.9+"));
        assert!(http_msg.contains("www service"));
    }

    #[test]
    fn mikrotik_client_snippet_truncates_long_bodies() {
        let long = "x".repeat(500);
        let cut = snippet(&long);
        assert!(cut.chars().count() <= 161);
        assert!(cut.ends_with('…'));
        assert_eq!(snippet("short"), "short");
        assert_eq!(snippet("  padded  "), "padded");
    }
}
