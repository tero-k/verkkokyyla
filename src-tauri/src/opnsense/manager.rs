//! OPNsense profile manager: CRUD over `opnsense_profiles`, API key/secret
//! storage in the shared [`SecretStore`] as one JSON entry per profile, and
//! a shared activity registry so deletion can be refused while a monitor,
//! log stream or terminal is live for the profile.
//!
//! The API credentials are device-local read-only HTTP credentials; the
//! interactive SSH terminal authenticates with a SEPARATE user/password
//! (`terminal.rs`) and never touches these.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::db::{now_rfc3339, Database, NewOpnsenseProfile, OpnsenseProfile};
use crate::opnsense::client::OpnsenseConnection;
use crate::opnsense::types::{
    CreateOpnsenseProfileRequest, OpnsenseApiFactory, OpnsenseDeleteResultDto,
    OpnsenseManagerError, OpnsenseProfileDto, OpnsenseTestConnectionDto,
    UpdateOpnsenseProfileRequest,
};
use crate::secrets::SecretStore;

/// Which profile-backed sessions are currently live. Monitor, log stream
/// and terminal managers record their sessions here so
/// [`OpnsenseManager::delete_profile`] can refuse deletion while anything
/// is active (same safeguard as MikroTik, extended to all three kinds).
#[derive(Default)]
pub struct OpnsenseActivity {
    monitors: HashMap<i64, usize>,
    logs: HashMap<i64, usize>,
    terminals: HashMap<i64, usize>,
}

impl OpnsenseActivity {
    pub fn is_active(&self, profile_id: i64) -> bool {
        self.monitors.contains_key(&profile_id)
            || self.logs.contains_key(&profile_id)
            || self.terminals.contains_key(&profile_id)
    }

    fn enter(sessions: &mut HashMap<i64, usize>, profile_id: i64) {
        *sessions.entry(profile_id).or_default() += 1;
    }

    fn exit(sessions: &mut HashMap<i64, usize>, profile_id: i64) {
        if let Some(count) = sessions.get_mut(&profile_id) {
            *count -= 1;
            if *count == 0 {
                sessions.remove(&profile_id);
            }
        }
    }

    pub fn monitor_enter(&mut self, profile_id: i64) {
        Self::enter(&mut self.monitors, profile_id);
    }
    pub fn monitor_exit(&mut self, profile_id: i64) {
        Self::exit(&mut self.monitors, profile_id);
    }
    pub fn log_enter(&mut self, profile_id: i64) {
        Self::enter(&mut self.logs, profile_id);
    }
    pub fn log_exit(&mut self, profile_id: i64) {
        Self::exit(&mut self.logs, profile_id);
    }
    pub fn terminal_enter(&mut self, profile_id: i64) {
        Self::enter(&mut self.terminals, profile_id);
    }
    pub fn terminal_exit(&mut self, profile_id: i64) {
        Self::exit(&mut self.terminals, profile_id);
    }
}

pub type OpnsenseActivityRegistry = Arc<Mutex<OpnsenseActivity>>;

#[derive(Clone)]
pub struct OpnsenseManager {
    db: Arc<Database>,
    store: Arc<dyn SecretStore>,
    factory: OpnsenseApiFactory,
    activity: OpnsenseActivityRegistry,
}

impl OpnsenseManager {
    pub fn new(db: Database, store: Arc<dyn SecretStore>, factory: OpnsenseApiFactory) -> Self {
        Self {
            db: Arc::new(db),
            store,
            factory,
            activity: Arc::new(Mutex::new(OpnsenseActivity::default())),
        }
    }

    /// The shared activity registry — monitor/log/terminal managers clone
    /// this at construction.
    pub fn activity(&self) -> OpnsenseActivityRegistry {
        Arc::clone(&self.activity)
    }

    pub(crate) fn api_factory(&self) -> OpnsenseApiFactory {
        Arc::clone(&self.factory)
    }

    pub(crate) async fn require_profile(
        &self,
        id: i64,
    ) -> Result<OpnsenseProfile, OpnsenseManagerError> {
        self.db
            .load_opnsense_profile(id)
            .await?
            .ok_or(OpnsenseManagerError::ProfileNotFound(id))
    }

    /// Fetch the API credentials from the keyring and build connection
    /// parameters. Credentials arrive as one JSON `{"key","secret"}` entry.
    pub(crate) async fn connection_for(
        &self,
        profile: &OpnsenseProfile,
    ) -> Result<OpnsenseConnection, OpnsenseManagerError> {
        let stored = self.store.get(&profile.secret_key).await?;
        let parsed: serde_json::Value = serde_json::from_str(&stored).map_err(|_| {
            OpnsenseManagerError::Api(crate::opnsense::client::OpnsenseError::Parse(
                "stored API credentials are corrupt — re-enter them".to_owned(),
            ))
        })?;
        let key = parsed
            .get("key")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(OpnsenseManagerError::InvalidCredentials)?;
        let secret = parsed
            .get("secret")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or(OpnsenseManagerError::InvalidCredentials)?;
        Ok(OpnsenseConnection {
            host: profile.host.clone(),
            port: u16::try_from(profile.port)
                .map_err(|_| OpnsenseManagerError::InvalidPort(profile.port))?,
            allow_invalid_certs: profile.allow_invalid_certs,
            api_key: key.to_owned(),
            api_secret: secret.to_owned(),
        })
    }

    // ---- Profiles -----------------------------------------------------------

    pub async fn list_profiles(&self) -> Result<Vec<OpnsenseProfileDto>, OpnsenseManagerError> {
        let mut dtos = Vec::new();
        for profile in self.db.list_opnsense_profiles().await? {
            dtos.push(self.profile_dto(profile).await);
        }
        Ok(dtos)
    }

    pub async fn create_profile(
        &self,
        request: &CreateOpnsenseProfileRequest,
    ) -> Result<OpnsenseProfileDto, OpnsenseManagerError> {
        validate_host(&request.host)?;
        validate_port(request.port).map_err(|_| OpnsenseManagerError::InvalidPort(request.port))?;
        validate_port(request.ssh_port)
            .map_err(|_| OpnsenseManagerError::InvalidSshPort(request.ssh_port))?;
        let profile = self
            .db
            .create_opnsense_profile(&NewOpnsenseProfile {
                name: request.name.clone(),
                host: request.host.clone(),
                port: request.port,
                allow_invalid_certs: request.allow_invalid_certs,
                ssh_port: request.ssh_port,
                created_at: now_rfc3339(),
            })
            .await?;
        Ok(self.profile_dto(profile).await)
    }

    /// Update editable fields. Changing the host or SSH port invalidates the
    /// pinned SSH host key (a different endpoint needs re-verification);
    /// name/API-port edits keep it.
    pub async fn update_profile(
        &self,
        request: &UpdateOpnsenseProfileRequest,
    ) -> Result<OpnsenseProfileDto, OpnsenseManagerError> {
        validate_host(&request.host)?;
        validate_port(request.port).map_err(|_| OpnsenseManagerError::InvalidPort(request.port))?;
        validate_port(request.ssh_port)
            .map_err(|_| OpnsenseManagerError::InvalidSshPort(request.ssh_port))?;
        let existing = self.require_profile(request.id).await?;
        self.db
            .update_opnsense_profile(
                request.id,
                &NewOpnsenseProfile {
                    name: request.name.clone(),
                    host: request.host.clone(),
                    port: request.port,
                    allow_invalid_certs: request.allow_invalid_certs,
                    ssh_port: request.ssh_port,
                    created_at: existing.created_at.clone(),
                },
            )
            .await?;
        if existing.host != request.host || existing.ssh_port != request.ssh_port {
            self.db.set_opnsense_ssh_host_key(request.id, None).await?;
        }
        let profile = self.require_profile(request.id).await?;
        Ok(self.profile_dto(profile).await)
    }

    /// Delete is refused with a typed `ProfileInUse` while any monitor, log
    /// stream or terminal is live for the profile. Otherwise the DB row
    /// goes FIRST and the keyring delete is best-effort (a failure never
    /// blocks the delete).
    pub async fn delete_profile(
        &self,
        id: i64,
    ) -> Result<OpnsenseDeleteResultDto, OpnsenseManagerError> {
        if self.activity.lock().await.is_active(id) {
            return Err(OpnsenseManagerError::ProfileInUse(id));
        }
        let profile = self.require_profile(id).await?;
        self.db.delete_opnsense_profile(id).await?;
        match self.store.delete(&profile.secret_key).await {
            Ok(()) => Ok(OpnsenseDeleteResultDto {
                deleted: true,
                secret_deleted: true,
                warning: None,
            }),
            Err(err) => Ok(OpnsenseDeleteResultDto {
                deleted: true,
                secret_deleted: false,
                warning: Some(format!("profile deleted; keyring cleanup failed: {err}")),
            }),
        }
    }

    /// Store the API key/secret as ONE keyring entry (`{"key","secret"}`),
    /// overwriting any previous entry. Both fields must be non-empty.
    pub async fn set_credentials(
        &self,
        id: i64,
        key: &str,
        secret: &str,
    ) -> Result<(), OpnsenseManagerError> {
        if key.is_empty() || secret.is_empty() {
            return Err(OpnsenseManagerError::InvalidCredentials);
        }
        let profile = self.require_profile(id).await?;
        let payload = serde_json::json!({ "key": key, "secret": secret }).to_string();
        self.store.set(&profile.secret_key, &payload).await?;
        Ok(())
    }

    /// Probe `GET /api/core/system/status`; success proves connectivity and
    /// authorization. No device payload crosses the boundary.
    pub async fn test_connection(
        &self,
        id: i64,
    ) -> Result<OpnsenseTestConnectionDto, OpnsenseManagerError> {
        let profile = self.require_profile(id).await?;
        let conn = self.connection_for(&profile).await?;
        let api = (self.factory)(conn).await?;
        api.get_system_status().await?;
        Ok(OpnsenseTestConnectionDto {
            connected: true,
            authorized: true,
        })
    }

    async fn profile_dto(&self, profile: OpnsenseProfile) -> OpnsenseProfileDto {
        let has_credentials = self.store.get(&profile.secret_key).await.is_ok();
        OpnsenseProfileDto {
            id: profile.id,
            name: profile.name,
            host: profile.host,
            port: profile.port,
            allow_invalid_certs: profile.allow_invalid_certs,
            ssh_port: profile.ssh_port,
            has_credentials,
            ssh_host_key_verified: profile.ssh_host_key.is_some(),
            created_at: profile.created_at,
        }
    }
}

/// Host must be a bare DNS name or IP literal: no scheme, path, query,
/// userinfo or whitespace. A colon is only acceptable inside an IPv6
/// literal.
fn validate_host(host: &str) -> Result<(), OpnsenseManagerError> {
    let reject = || OpnsenseManagerError::InvalidHost(host.to_owned());
    if host.is_empty() || host.len() > 253 {
        return Err(reject());
    }
    if host.chars().any(char::is_whitespace)
        || host.contains(['/', '@', '?', '#'])
        || host.contains("://")
    {
        return Err(reject());
    }
    if host.contains(':') {
        // Only valid as a bare IPv6 literal.
        let unbracketed = host.trim_start_matches('[').trim_end_matches(']');
        if unbracketed.parse::<std::net::Ipv6Addr>().is_err() {
            return Err(reject());
        }
    }
    Ok(())
}

fn validate_port(port: i64) -> Result<u16, ()> {
    if !(1..=65535).contains(&port) {
        return Err(());
    }
    u16::try_from(port).map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_validation_accepts_dns_and_ips() {
        for host in [
            "fw.example.com",
            "192.168.1.1",
            "opnsense",
            "::1",
            "2001:db8::1",
        ] {
            assert!(validate_host(host).is_ok(), "{host:?} must be accepted");
        }
    }

    #[test]
    fn host_validation_rejects_scheme_path_userinfo_and_ports() {
        for host in [
            "",
            "https://fw.example.com",
            "fw.example.com/",
            "fw.example.com/api",
            "user@fw.example.com",
            "fw.example.com:8443",
            "fw example.com",
            " fw.example.com",
        ] {
            assert!(validate_host(host).is_err(), "{host:?} must be rejected");
        }
    }

    #[test]
    fn terminal_activity_remains_until_last_session_closes() {
        let mut activity = OpnsenseActivity::default();
        activity.terminal_enter(1);
        activity.terminal_enter(1);
        activity.terminal_exit(1);
        assert!(activity.is_active(1));
        activity.terminal_exit(1);
        assert!(!activity.is_active(1));
    }

    #[test]
    fn overlapping_monitors_and_logs_keep_profile_in_use() {
        let mut activity = OpnsenseActivity::default();
        activity.monitor_enter(1);
        activity.monitor_enter(1);
        activity.log_enter(1);
        activity.log_enter(1);
        activity.monitor_exit(1);
        activity.log_exit(1);
        assert!(activity.is_active(1));
        activity.monitor_exit(1);
        assert!(activity.is_active(1));
        activity.log_exit(1);
        assert!(!activity.is_active(1));
    }

    #[test]
    fn port_validation_bounds() {
        assert!(validate_port(443).is_ok());
        assert!(validate_port(1).is_ok());
        assert!(validate_port(65535).is_ok());
        assert!(validate_port(0).is_err());
        assert!(validate_port(65536).is_err());
        assert!(validate_port(-1).is_err());
    }
}
