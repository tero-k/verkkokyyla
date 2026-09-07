//! Configuration backup via RouterOS REST + SFTP download.
//!
//! Flow: `POST /rest/system/backup/save` (60s command timeout) creates
//! `<name>.backup` on the router; `/rest/export` optionally creates
//! `<name>.rsc`; both are polled for on `/rest/file`, downloaded over SFTP
//! (russh + russh-sftp, password auth from the keyring), and written into the
//! user-chosen destination directory. Router-side temp files are deleted
//! best-effort on EVERY exit path after the REST save (cleanup guard) so the
//! router never keeps temp files; delete failures surface as
//! `cleanupWarnings` ("saved locally; router temp file <name> may remain").
//!
//! PORT LOCK: SFTP always connects on EXACTLY port 22, regardless of the
//! profile's REST port (the SSH service default).
//!
//! Accepted risk: unknown SSH host keys are accepted (LAN routers); README
//! documents this. Passwords are never logged.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use thiserror::Error;

use crate::db::{Database, DbError};
use crate::mikrotik::client::{MikrotikClient, MikrotikConnection};
use crate::mikrotik::error::MikrotikError;
use crate::mikrotik::secrets::{KeyringStore, SecretError, SecretStore};

/// SFTP connects on EXACTLY this port, never the profile's REST port.
pub const SFTP_PORT: u16 = 22;
/// Poll `/rest/file` at this interval while waiting for the router to
/// materialize the backup/export file.
const FILE_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Give up waiting for the router-side file after this long.
const FILE_POLL_TIMEOUT: Duration = Duration::from_secs(60);

/// `/rest/file` poll cadence. Production uses [`FILE_POLL_INTERVAL`] /
/// [`FILE_POLL_TIMEOUT`]; tests shrink it so the never-listed path does not
/// burn a minute of real time.
#[derive(Clone, Copy, Debug)]
pub struct FilePoll {
    pub interval: Duration,
    pub timeout: Duration,
}

impl Default for FilePoll {
    fn default() -> Self {
        Self {
            interval: FILE_POLL_INTERVAL,
            timeout: FILE_POLL_TIMEOUT,
        }
    }
}

/// Result of a successful backup: local paths + router-cleanup warnings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupResultDto {
    /// Local `<destination_dir>/<backup_name>.backup` file.
    pub backup_path: String,
    /// Local `<destination_dir>/<backup_name>.rsc` file (null when the
    /// export was not requested).
    pub export_path: Option<String>,
    /// Non-empty when a best-effort router-side temp-file delete failed;
    /// the UI shows "saved locally; router temp file <name> may remain".
    pub cleanup_warnings: Vec<String>,
}

/// Typed errors for the backup flow; serialized to the frontend as
/// `{ kind, message }` (same contract as `session::SessionError`).
#[derive(Debug, Error)]
pub enum BackupError {
    /// `backup_name` violates the name rule or is a Windows device name.
    #[error(
        "invalid backup name {0:?}: must match ^[A-Za-z0-9][A-Za-z0-9._-]{{0,63}}$ \
         and must not be a Windows device name (CON, PRN, AUX, NUL, COM1-9, LPT1-9)"
    )]
    InvalidName(String),

    /// `destination_dir` does not exist, is not a directory, or an output
    /// path resolves outside it.
    #[error("destination directory invalid: {0}")]
    DestinationInvalid(String),

    /// The output file exists and `overwrite` is false.
    #[error("output already exists: {0} (pass overwrite to replace it)")]
    OutputExists(String),

    /// Writing the downloaded bytes locally failed (permissions, disk full).
    #[error("local write failed for {path}: {message}")]
    LocalWrite { path: String, message: String },

    /// TCP connect to the SSH service failed (service disabled/firewalled).
    #[error(
        "SSH/SFTP unreachable on {host}: {message} — enable the SSH service \
         on the router (IP > Services, default port 22)"
    )]
    SshUnreachable { host: String, message: String },

    /// SSH password authentication was rejected.
    #[error("SSH authentication failed: {0}")]
    SshAuth(String),

    /// SFTP subsystem or transfer failure (distinct from connect-refused).
    #[error("SFTP error: {0}")]
    Sftp(String),

    /// `/rest/file` never listed the expected file within the deadline.
    #[error("backup timed out: {0}")]
    BackupTimeout(String),

    /// The REST save succeeded but the SFTP download failed.
    #[error("partial download: {0}")]
    PartialDownload(String),

    /// `profile_id` did not match a stored profile.
    #[error("no MikroTik profile with id {0}")]
    ProfileNotFound(i64),

    /// REST client/parse failure (reuses the todo-2 typed errors).
    #[error("router API error: {0}")]
    Rest(#[from] MikrotikError),

    /// Keyring lookup for the profile password failed.
    #[error("password store error: {0}")]
    Secret(#[from] SecretError),

    /// SQLite failure while loading the profile.
    #[error("database error: {0}")]
    Db(#[from] DbError),
}

impl BackupError {
    /// Stable machine-readable discriminator for the frontend.
    pub fn kind(&self) -> &'static str {
        match self {
            BackupError::InvalidName(_) => "InvalidName",
            BackupError::DestinationInvalid(_) => "DestinationInvalid",
            BackupError::OutputExists(_) => "OutputExists",
            BackupError::LocalWrite { .. } => "LocalWrite",
            BackupError::SshUnreachable { .. } => "SshUnreachable",
            BackupError::SshAuth(_) => "SshAuth",
            BackupError::Sftp(_) => "Sftp",
            BackupError::BackupTimeout(_) => "BackupTimeout",
            BackupError::PartialDownload(_) => "PartialDownload",
            BackupError::ProfileNotFound(_) => "ProfileNotFound",
            BackupError::Rest(err) => match err {
                MikrotikError::Unauthorized => "Unauthorized",
                MikrotikError::Forbidden => "Forbidden",
                MikrotikError::Connect(_) => "Connect",
                MikrotikError::Timeout(_) => "Timeout",
                MikrotikError::Tls(_) => "Tls",
                MikrotikError::Api { .. } => "Api",
                MikrotikError::Parse(_) => "Parse",
                MikrotikError::FileNotFound(_) => "FileNotFound",
                MikrotikError::UnsupportedVersion(_) => "UnsupportedVersion",
            },
            BackupError::Secret(err) => match err {
                SecretError::NotStored => "PasswordNotStored",
                SecretError::Keyring(_) => "Keyring",
            },
            BackupError::Db(_) => "Db",
        }
    }
}

impl Serialize for BackupError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("BackupError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

// ---------------------------------------------------------------------------
// Backup-name validation
// ---------------------------------------------------------------------------

/// Validate `backup_name` BEFORE any router call: `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`
/// and the stem before the first dot, case-insensitively, must not be a
/// Windows device name (so `CON`, `con.txt`, `Com1.backup` all reject).
fn validate_backup_name(name: &str) -> Result<(), BackupError> {
    let mut chars = name.chars();
    let first = chars
        .next()
        .ok_or_else(|| BackupError::InvalidName(name.to_owned()))?;
    let valid = first.is_ascii_alphanumeric()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !valid {
        return Err(BackupError::InvalidName(name.to_owned()));
    }
    let stem = name.split('.').next().unwrap_or(name);
    if is_windows_device_name(stem) {
        return Err(BackupError::InvalidName(name.to_owned()));
    }
    Ok(())
}

/// CON, PRN, AUX, NUL, COM1-9, LPT1-9 (case-insensitive; COM0/LPT0 are
/// NOT reserved device names).
fn is_windows_device_name(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let bytes = upper.as_bytes();
    bytes.len() == 4
        && (upper.starts_with("COM") || upper.starts_with("LPT"))
        && bytes[3].is_ascii_digit()
        && bytes[3] != b'0'
}

// ---------------------------------------------------------------------------
// SFTP transfer seam
// ---------------------------------------------------------------------------

/// Download seam: production code uses [`RusshSftpFetch`]; tests inject fakes
/// that record the connect target and script failures.
#[async_trait]
pub trait SftpFetch: Send + Sync {
    /// Download `/<remote_name>` from the router and return its bytes.
    async fn fetch_file(
        &self,
        host: &str,
        port: u16,
        username: &str,
        password: &str,
        remote_name: &str,
    ) -> Result<Vec<u8>, BackupError>;
}

/// Accept-unknown-host-key policy: an accepted risk for LAN routers,
/// documented in the README.
struct AcceptUnknownHostKey;

impl russh::client::Handler for AcceptUnknownHostKey {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// Classify a russh connect failure: refusal/timeout → `SshUnreachable`
/// ("enable the SSH service on the router"), anything else → `Sftp`.
fn classify_connect_error(host: &str, message: String) -> BackupError {
    let lower = message.to_lowercase();
    if lower.contains("refused")
        || lower.contains("timed out")
        || lower.contains("timeout")
        || lower.contains("unreachable")
        || lower.contains("no connection could be made")
    {
        BackupError::SshUnreachable { host: host.to_owned(), message }
    } else {
        BackupError::Sftp(message)
    }
}

/// Production [`SftpFetch`] over russh + russh-sftp: password auth, port 22.
pub struct RusshSftpFetch;

#[async_trait]
impl SftpFetch for RusshSftpFetch {
    async fn fetch_file(
        &self,
        host: &str,
        port: u16,
        username: &str,
        password: &str,
        remote_name: &str,
    ) -> Result<Vec<u8>, BackupError> {
        let config = russh::client::Config::default();
        let mut session = russh::client::connect(
            Arc::new(config),
            (host, port),
            AcceptUnknownHostKey,
        )
        .await
        .map_err(|err| classify_connect_error(host, err.to_string()))?;

        let auth = session
            .authenticate_password(username, password)
            .await
            .map_err(|err| BackupError::SshAuth(err.to_string()))?;
        if !auth.success() {
            return Err(BackupError::SshAuth(format!(
                "password rejected for user {username:?}"
            )));
        }

        let channel = session
            .channel_open_session()
            .await
            .map_err(|err| BackupError::Sftp(err.to_string()))?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .map_err(|err| BackupError::Sftp(err.to_string()))?;
        let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
            .await
            .map_err(|err| BackupError::Sftp(err.to_string()))?;

        sftp.read(format!("/{remote_name}"))
            .await
            .map_err(|err| BackupError::Sftp(err.to_string()))
    }
}

// ---------------------------------------------------------------------------
// Backup orchestration
// ---------------------------------------------------------------------------

/// Inputs for [`run_backup`], assembled by the Tauri command (or tests).
pub struct BackupParams<'a> {
    /// REST client for the profile (Basic auth already wired).
    pub client: &'a MikrotikClient,
    /// SFTP transfer seam (production: [`RusshSftpFetch`]).
    pub sftp: &'a dyn SftpFetch,
    /// Router host for the SFTP connection (the profile's host).
    pub sftp_host: &'a str,
    /// Router username for SSH password auth.
    pub sftp_username: &'a str,
    /// Keyring-fetched profile password for SSH password auth.
    pub sftp_password: &'a str,
    /// User-chosen destination DIRECTORY (frontend dialog pick).
    pub destination_dir: &'a Path,
    /// Validated-later backup base name (no extension).
    pub backup_name: &'a str,
    /// Optional backup ENCRYPTION password for `/rest/system/backup/save`
    /// (never the SSH password).
    pub password: Option<&'a str>,
    /// Also export `<name>.rsc` and download it.
    pub include_rsc: bool,
    /// Replace existing local output files.
    pub overwrite: bool,
    /// `/rest/file` poll cadence ([`FilePoll::default`] in production).
    pub file_poll: FilePoll,
}

/// Map a download failure to its typed outcome: connect/auth failures keep
/// their variant; anything else after a successful REST save is a
/// `PartialDownload` (the router has files the local disk does not).
fn classify_download_error(err: BackupError) -> BackupError {
    match err {
        BackupError::SshUnreachable { .. } | BackupError::SshAuth(_) => err,
        other => BackupError::PartialDownload(other.to_string()),
    }
}

/// Overwrite guard: an existing output without `overwrite` → `OutputExists`.
/// If the existing entry's canonical path escapes the canonical destination
/// directory (symlink), → `DestinationInvalid` instead of writing through it.
fn check_output(path: &Path, overwrite: bool) -> Result<(), BackupError> {
    if !path.exists() || overwrite {
        return Ok(());
    }
    if let Ok(canonical) = path.canonicalize() {
        let expected = path
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .and_then(|parent| path.file_name().map(|name| parent.join(name)));
        if expected.as_deref() != Some(canonical.as_path()) {
            return Err(BackupError::DestinationInvalid(format!(
                "output path {} resolves to {} outside the destination directory",
                path.display(),
                canonical.display()
            )));
        }
    }
    Err(BackupError::OutputExists(path.display().to_string()))
}

/// Run the full backup flow. Validation (`InvalidName`, `DestinationInvalid`,
/// `OutputExists`) happens BEFORE any router call; the cleanup guard (best-
/// effort router-side `delete_file` of EVERY file created this run, with
/// warnings) runs on ALL exits after the REST save — including `LocalWrite`.
pub async fn run_backup(params: &BackupParams<'_>) -> Result<BackupResultDto, BackupError> {
    validate_backup_name(params.backup_name)?;

    // Canonicalize the destination dir and REQUIRE it to exist and be a
    // directory; JOIN the validated basename — never canonicalize the output
    // file itself (it may not exist yet).
    let dir = params
        .destination_dir
        .canonicalize()
        .map_err(|err| BackupError::DestinationInvalid(format!("{}: {err}", params.destination_dir.display())))?;
    if !dir.is_dir() {
        return Err(BackupError::DestinationInvalid(format!(
            "{} is not a directory",
            dir.display()
        )));
    }
    let backup_path = dir.join(format!("{}.backup", params.backup_name));
    let export_path = dir.join(format!("{}.rsc", params.backup_name));
    check_output(&backup_path, params.overwrite)?;
    if params.include_rsc {
        check_output(&export_path, params.overwrite)?;
    }

    // Router-side creation. Everything from here on MUST attempt cleanup.
    params.client.backup_save(params.backup_name, params.password).await?;

    let created_files = if params.include_rsc {
        vec![
            format!("{}.backup", params.backup_name),
            format!("{}.rsc", params.backup_name),
        ]
    } else {
        vec![format!("{}.backup", params.backup_name)]
    };

    let outcome = download_and_write(params, &backup_path, &export_path).await;

    // Cleanup guard: delete EVERY file created this run (best-effort). A
    // FileNotFound means nothing to clean; any other failure is a warning.
    let mut cleanup_warnings = Vec::new();
    for name in &created_files {
        if let Err(err) = params.client.delete_file(name).await {
            if !matches!(err, MikrotikError::FileNotFound(_)) {
                cleanup_warnings.push(format!(
                    "saved locally; router temp file {name} may remain: {err}"
                ));
            }
        }
    }

    let mut dto = outcome?;
    dto.cleanup_warnings = cleanup_warnings;
    Ok(dto)
}

/// Wait for `name` to appear on `/rest/file` (2s interval, 60s deadline),
/// then return. A never-appearing file → `BackupTimeout`.
async fn wait_for_file(client: &MikrotikClient, name: &str, poll: FilePoll) -> Result<(), BackupError> {
    let deadline = tokio::time::Instant::now() + poll.timeout;
    loop {
        let files = client.list_files().await?;
        if files.iter().any(|f| f.name.as_deref() == Some(name)) {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(BackupError::BackupTimeout(format!(
                "/rest/file never listed {name} within {}s",
                poll.timeout.as_secs()
            )));
        }
        tokio::time::sleep(poll.interval).await;
    }
}

/// Write downloaded bytes to a local path; failures → typed `LocalWrite`.
fn write_output(path: &Path, bytes: &[u8]) -> Result<(), BackupError> {
    std::fs::write(path, bytes).map_err(|err| BackupError::LocalWrite {
        path: path.display().to_string(),
        message: err.to_string(),
    })
}

/// Download phase: poll for `.backup` (+ optional `.rsc`), fetch each over
/// SFTP (PORT LOCK: exactly [`SFTP_PORT`]), write both locally.
async fn download_and_write(
    params: &BackupParams<'_>,
    backup_path: &Path,
    export_path: &PathBuf,
) -> Result<BackupResultDto, BackupError> {
    let backup_remote = format!("{}.backup", params.backup_name);
    wait_for_file(params.client, &backup_remote, params.file_poll).await?;

    let export_remote = if params.include_rsc {
        params.client.export_rsc(params.backup_name).await?;
        let remote = format!("{}.rsc", params.backup_name);
        wait_for_file(params.client, &remote, params.file_poll).await?;
        Some(remote)
    } else {
        None
    };

    async fn fetch(remote: &str, params: &BackupParams<'_>) -> Result<Vec<u8>, BackupError> {
        params
            .sftp
            .fetch_file(
                params.sftp_host,
                SFTP_PORT,
                params.sftp_username,
                params.sftp_password,
                remote,
            )
            .await
    }

    let backup_bytes = fetch(&backup_remote, params)
        .await
        .map_err(classify_download_error)?;
    write_output(backup_path, &backup_bytes)?;

    let export_path = match export_remote {
        Some(remote) => {
            let bytes = fetch(&remote, params).await.map_err(classify_download_error)?;
            write_output(export_path, &bytes)?;
            Some(export_path.display().to_string())
        }
        None => None,
    };

    Ok(BackupResultDto {
        backup_path: backup_path.display().to_string(),
        export_path,
        cleanup_warnings: Vec::new(),
    })
}

// ---------------------------------------------------------------------------
// Tauri command
// ---------------------------------------------------------------------------

/// Managed state for the backup command: the SQLite handle (profile lookup)
/// and the OS-keyring secret store. Todo 5's manager state is separate; this
/// command never needs the polling runtime.
pub struct MikrotikBackupState {
    pub db: Arc<Database>,
    pub secrets: Arc<KeyringStore>,
}

impl MikrotikBackupState {
    pub fn new(db: Database) -> Self {
        Self {
            db: Arc::new(db),
            secrets: Arc::new(KeyringStore::new()),
        }
    }
}

/// `mikrotik_backup(profile_id, destination_dir, backup_name, password?,
/// include_rsc, overwrite)` — destination is a frontend-picked DIRECTORY.
#[tauri::command]
pub async fn mikrotik_backup(
    state: tauri::State<'_, MikrotikBackupState>,
    profile_id: i64,
    destination_dir: String,
    backup_name: String,
    password: Option<String>,
    include_rsc: bool,
    overwrite: bool,
) -> Result<BackupResultDto, BackupError> {
    let profile = state
        .db
        .load_mikrotik_profile(profile_id)
        .await?
        .ok_or(BackupError::ProfileNotFound(profile_id))?;
    // Keyring-fetched password: Basic auth for REST AND SSH password auth.
    let ssh_password = state.secrets.get(&profile.secret_key).await?;
    let port = u16::try_from(profile.port)
        .map_err(|_| BackupError::DestinationInvalid(format!("profile port {}", profile.port)))?;
    let conn = MikrotikConnection {
        host: profile.host.clone(),
        port,
        use_tls: profile.use_tls,
        allow_invalid_certs: profile.allow_invalid_certs,
        username: profile.username.clone(),
        password: ssh_password.clone(),
    };
    let client = MikrotikClient::new(&conn)?;
    let destination = PathBuf::from(&destination_dir);
    run_backup(&BackupParams {
        client: &client,
        sftp: &RusshSftpFetch,
        sftp_host: &profile.host,
        sftp_username: &profile.username,
        sftp_password: &ssh_password,
        destination_dir: &destination,
        backup_name: &backup_name,
        password: password.as_deref(),
        include_rsc,
        overwrite,
        file_poll: FilePoll::default(),
    })
    .await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    use serde_json::json;
    use russh::server::Server as _;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    const DEMO_BACKUP: &[u8] = b"routeros-backup-bytes\x00\x01\x02";
    const DEMO_RSC: &[u8] = b"/interface export compact\n";

    static DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

    /// Unique temp directory per test (no tempfile crate in dev-deps).
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vk-mikrotik-backup-{}-{}-{}",
            tag,
            std::process::id(),
            DIR_SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wiremock_port(server: &MockServer) -> u16 {
        server
            .uri()
            .trim_start_matches("http://")
            .rsplit(':')
            .next()
            .expect("port in uri")
            .parse()
            .expect("numeric port")
    }

    fn conn_for(server: &MockServer) -> MikrotikConnection {
        MikrotikConnection {
            host: "127.0.0.1".to_owned(),
            port: wiremock_port(server),
            use_tls: false,
            allow_invalid_certs: false,
            username: "admin".to_owned(),
            password: "s3cr3t".to_owned(),
        }
    }

    /// `/rest/file` payload listing the two demo router-side files.
    fn file_list() -> serde_json::Value {
        json!([
            {".id": "*1", "name": "demo.backup", "size": DEMO_BACKUP.len()},
            {".id": "*2", "name": "demo.rsc", "size": DEMO_RSC.len()},
        ])
    }

    /// Wiremock REST fixture: save + export succeed (empty bodies), file
    /// listing returns `files`, DELETEs succeed (asserted via `expect`).
    async fn rest_server(
        files: serde_json::Value,
        backup_delete_expect: u64,
        rsc_delete_expect: u64,
    ) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/system/backup/save"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/rest/export"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/rest/file"))
            .respond_with(ResponseTemplate::new(200).set_body_json(files))
            .mount(&server)
            .await;
        for (id, expect) in [("*1", backup_delete_expect), ("*2", rsc_delete_expect)] {
            Mock::given(method("DELETE"))
                .and(path(format!("/rest/file/{id}")))
                .respond_with(ResponseTemplate::new(200))
                .expect(expect)
                .mount(&server)
                .await;
        }
        server
    }

    /// Scripted SFTP fake: records connect targets, serves canned bytes,
    /// optionally fails every fetch.
    #[derive(Default)]
    struct FakeSftp {
        fetches: Mutex<Vec<(String, u16, String)>>,
        fail_with: Option<BackupError>,
    }

    #[async_trait]
    impl SftpFetch for FakeSftp {
        async fn fetch_file(
            &self,
            host: &str,
            port: u16,
            _username: &str,
            _password: &str,
            remote_name: &str,
        ) -> Result<Vec<u8>, BackupError> {
            self.fetches
                .lock()
                .unwrap()
                .push((host.to_owned(), port, remote_name.to_owned()));
            if let Some(err) = &self.fail_with {
                return Err(match err {
                    BackupError::SshUnreachable { host, message } => BackupError::SshUnreachable {
                        host: host.clone(),
                        message: message.clone(),
                    },
                    BackupError::Sftp(m) => BackupError::Sftp(m.clone()),
                    other => BackupError::Sftp(other.to_string()),
                });
            }
            match remote_name {
                "demo.backup" => Ok(DEMO_BACKUP.to_vec()),
                "demo.rsc" => Ok(DEMO_RSC.to_vec()),
                other => Err(BackupError::Sftp(format!("no such file {other}"))),
            }
        }
    }

    fn params<'a>(
        client: &'a MikrotikClient,
        sftp: &'a dyn SftpFetch,
        dir: &'a Path,
        include_rsc: bool,
        overwrite: bool,
    ) -> BackupParams<'a> {
        BackupParams {
            client,
            sftp,
            sftp_host: "192.0.2.7",
            sftp_username: "admin",
            sftp_password: "s3cr3t",
            destination_dir: dir,
            backup_name: "demo",
            password: None,
            include_rsc,
            overwrite,
            file_poll: FilePoll::default(),
        }
    }

    async fn received(server: &MockServer) -> Vec<Request> {
        server.received_requests().await.unwrap_or_default()
    }

    // -- Name validation (before any router call) --------------------------

    #[test]
    fn mikrotik_backup_name_validation_table_reserved_set() {
        // Full Windows device set: CON, PRN, AUX, NUL, COM1-9, LPT1-9 —
        // mixed case and dotted extensions must all reject.
        let mut reserved: Vec<String> = vec![
            "CON".into(), "con.txt".into(), "Con".into(), "cOn.bAk".into(),
            "PRN".into(), "prn.log".into(), "AUX".into(), "aux.txt".into(),
            "NUL".into(), "nul.rsc".into(),
        ];
        for n in 1..=9 {
            reserved.push(format!("COM{n}"));
            reserved.push(format!("Com{n}.backup"));
            reserved.push(format!("lpt{n}"));
            reserved.push(format!("LPT{n}.x"));
        }
        for name in reserved {
            assert!(
                matches!(validate_backup_name(&name), Err(BackupError::InvalidName(_))),
                "{name:?} must be rejected"
            );
        }
        // COM0/LPT0/COM10 are NOT reserved; ordinary names are fine.
        for name in [
            "demo", "verkkokyyla-20260101-120000", "a.b.c", "COM0", "LPT0",
            "COM10", "LPT10", "conx", "console", "ok_1.2",
        ] {
            assert!(validate_backup_name(name).is_ok(), "{name:?} must be accepted");
        }
        // Regex violations: empty, bad first char, illegal chars, >64 chars.
        // `_ok-1.2` starts with `_`: the spec regex requires an alphanumeric
        // FIRST char (`^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`), so it rejects too.
        for name in ["", ".hidden", "-dash", "_ok-1.2", "a b", "a/b", "a\\b", &"x".repeat(65)] {
            assert!(
                matches!(validate_backup_name(name), Err(BackupError::InvalidName(_))),
                "{name:?} must be rejected"
            );
        }
    }

    #[tokio::test]
    async fn mikrotik_backup_invalid_name_hits_no_router_call() {
        let server = MockServer::start().await; // no mocks: any call would error
        let client = MikrotikClient::new(&conn_for(&server)).unwrap();
        let dir = temp_dir("invalid-name");
        let fake = FakeSftp::default();
        let mut p = params(&client, &fake, &dir, true, false);
        p.backup_name = "CON";
        let err = run_backup(&p).await.unwrap_err();
        assert!(matches!(err, BackupError::InvalidName(_)));
        assert_eq!(received(&server).await.len(), 0, "no router call allowed");
    }

    #[tokio::test]
    async fn mikrotik_backup_destination_invalid_before_router_call() {
        let server = MockServer::start().await;
        let client = MikrotikClient::new(&conn_for(&server)).unwrap();
        let fake = FakeSftp::default();

        // Missing directory.
        let missing = std::env::temp_dir().join(format!(
            "vk-mikrotik-backup-missing-{}-{}",
            std::process::id(),
            DIR_SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        let err = run_backup(&params(&client, &fake, &missing, true, false))
            .await
            .unwrap_err();
        assert!(matches!(err, BackupError::DestinationInvalid(_)));

        // destination_dir pointing at a FILE.
        let dir = temp_dir("dest-is-file");
        let file_path = dir.join("not-a-dir");
        std::fs::write(&file_path, b"x").unwrap();
        let err = run_backup(&params(&client, &fake, &file_path, true, false))
            .await
            .unwrap_err();
        assert!(matches!(err, BackupError::DestinationInvalid(_)));

        assert_eq!(received(&server).await.len(), 0, "no router call allowed");
    }

    // -- Happy paths ---------------------------------------------------------

    #[tokio::test]
    async fn mikrotik_backup_happy_path_with_rsc_downloads_both_files() {
        let server = rest_server(file_list(), 1, 1).await; // both DELETEs expected
        let client = MikrotikClient::new(&conn_for(&server)).unwrap();
        let dir = temp_dir("happy-rsc");
        let fake = FakeSftp::default();

        let dto = run_backup(&params(&client, &fake, &dir, true, false))
            .await
            .unwrap();

        // canonicalize() yields verbatim (\\?\) paths on Windows — compare
        // canonical-to-canonical.
        let cdir = dir.canonicalize().unwrap();
        assert_eq!(dto.backup_path, cdir.join("demo.backup").display().to_string());
        assert_eq!(dto.export_path.as_deref(), Some(cdir.join("demo.rsc").display().to_string().as_str()));
        assert_eq!(std::fs::read(dir.join("demo.backup")).unwrap(), DEMO_BACKUP);
        assert_eq!(std::fs::read(dir.join("demo.rsc")).unwrap(), DEMO_RSC);
        assert!(dto.cleanup_warnings.is_empty());

        // SFTP fetched both files, in order.
        let fetches = fake.fetches.lock().unwrap();
        assert_eq!(
            fetches.iter().map(|(_, _, name)| name.as_str()).collect::<Vec<_>>(),
            vec!["demo.backup", "demo.rsc"]
        );
    }

    #[tokio::test]
    async fn mikrotik_backup_happy_path_without_rsc_skips_export() {
        let server = rest_server(file_list(), 1, 0).await; // only *1 (.backup) expected
        let client = MikrotikClient::new(&conn_for(&server)).unwrap();
        let dir = temp_dir("happy-no-rsc");
        let fake = FakeSftp::default();

        let dto = run_backup(&params(&client, &fake, &dir, false, false))
            .await
            .unwrap();

        assert_eq!(dto.export_path, None);
        assert!(dir.join("demo.backup").exists());
        assert!(!dir.join("demo.rsc").exists());
        assert_eq!(fake.fetches.lock().unwrap().len(), 1);
        // The export POST must never fire.
        let export_calls = received(&server)
            .await
            .into_iter()
            .filter(|r| r.method.as_str() == "POST" && r.url.path() == "/rest/export")
            .count();
        assert_eq!(export_calls, 0);
    }

    #[tokio::test]
    async fn mikrotik_backup_sftp_port_lock_always_22() {
        assert_eq!(SFTP_PORT, 22);
        let server = rest_server(file_list(), 1, 0).await;
        // The profile's REST port is the wiremock port — never 22. The fake
        // records the SFTP connect target; every recorded port must be 22.
        let client = MikrotikClient::new(&conn_for(&server)).unwrap();
        let dir = temp_dir("port-lock");
        let fake = FakeSftp::default();
        run_backup(&params(&client, &fake, &dir, false, false))
            .await
            .unwrap();
        let fetches = fake.fetches.lock().unwrap();
        assert_eq!(fetches.len(), 1);
        assert_eq!(fetches[0].1, 22, "SFTP must connect on EXACTLY port 22");
        assert_eq!(fetches[0].0, "192.0.2.7");
    }

    #[tokio::test]
    async fn mikrotik_backup_output_exists_requires_overwrite() {
        let server = rest_server(file_list(), 1, 0).await; // one delete expected
        let client = MikrotikClient::new(&conn_for(&server)).unwrap();
        let dir = temp_dir("exists");
        std::fs::write(dir.join("demo.backup"), b"old").unwrap();
        let fake = FakeSftp::default();

        let err = run_backup(&params(&client, &fake, &dir, false, false))
            .await
            .unwrap_err();
        assert!(matches!(err, BackupError::OutputExists(_)));
        assert_eq!(std::fs::read(dir.join("demo.backup")).unwrap(), b"old");

        // overwrite=true replaces the file and completes the flow.
        let dto = run_backup(&params(&client, &fake, &dir, false, true))
            .await
            .unwrap();
        assert_eq!(std::fs::read(dir.join("demo.backup")).unwrap(), DEMO_BACKUP);
        assert!(dto.cleanup_warnings.is_empty());
    }

    // -- Cleanup guard (mid-operation interrupts / stale router state) -------

    #[tokio::test]
    async fn mikrotik_backup_partial_download_still_cleans_router_files() {
        let server = rest_server(file_list(), 1, 1).await; // BOTH deletes expected
        let client = MikrotikClient::new(&conn_for(&server)).unwrap();
        let dir = temp_dir("partial");
        let fake = FakeSftp {
            fail_with: Some(BackupError::Sftp("transfer aborted".to_owned())),
            ..Default::default()
        };

        let err = run_backup(&params(&client, &fake, &dir, true, false))
            .await
            .unwrap_err();
        assert!(
            matches!(err, BackupError::PartialDownload(_)),
            "unexpected error: {err}"
        );
        // Cleanup guard deletes .backup AND .rsc (wiremock expect(1) on both).
    }

    #[tokio::test]
    async fn mikrotik_backup_local_write_failure_still_cleans_router_files() {
        let server = rest_server(file_list(), 1, 1).await; // deletes still attempted
        let client = MikrotikClient::new(&conn_for(&server)).unwrap();
        let dir = temp_dir("local-write");
        // A DIRECTORY at the output path makes the final write fail even
        // with overwrite=true (write to a directory → OS error).
        std::fs::create_dir(dir.join("demo.backup")).unwrap();
        let fake = FakeSftp::default();

        let err = run_backup(&params(&client, &fake, &dir, true, true))
            .await
            .unwrap_err();
        assert!(
            matches!(err, BackupError::LocalWrite { .. }),
            "unexpected error: {err}"
        );
        // Delete attempted for every file created this run (.backup + .rsc).
    }

    #[tokio::test]
    async fn mikrotik_backup_cleanup_failure_warns_but_still_returns_paths() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/system/backup/save"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/rest/export"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/rest/file"))
            .respond_with(ResponseTemplate::new(200).set_body_json(file_list()))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/rest/file/*1"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        // The .rsc delete fails router-side (mounted last: takes precedence).
        Mock::given(method("DELETE"))
            .and(path("/rest/file/*2"))
            .respond_with(ResponseTemplate::new(500).set_body_string("disk busy"))
            .expect(1)
            .mount(&server)
            .await;
        let client = MikrotikClient::new(&conn_for(&server)).unwrap();
        let dir = temp_dir("cleanup-warn");
        let fake = FakeSftp::default();

        let dto = run_backup(&params(&client, &fake, &dir, true, false))
            .await
            .unwrap();

        assert_eq!(std::fs::read(dir.join("demo.backup")).unwrap(), DEMO_BACKUP);
        assert!(!dto.cleanup_warnings.is_empty(), "delete failure must warn");
        assert!(dto.cleanup_warnings.iter().any(|w| w.contains("demo.rsc")));
        // Paths are still returned so the UI can show the saved files.
        assert!(dto.backup_path.ends_with("demo.backup"));
        let cdir = dir.canonicalize().unwrap();
        assert_eq!(dto.export_path.as_deref(), Some(cdir.join("demo.rsc").display().to_string().as_str()));
    }

    #[tokio::test]
    async fn mikrotik_backup_timeout_when_file_never_listed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/system/backup/save"))
            .respond_with(ResponseTemplate::new(200).set_body_string(""))
            .mount(&server)
            .await;
        // /rest/file NEVER lists the backup file.
        Mock::given(method("GET"))
            .and(path("/rest/file"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .mount(&server)
            .await;
        let client = MikrotikClient::new(&conn_for(&server)).unwrap();
        let dir = temp_dir("timeout");
        let fake = FakeSftp::default();

        // Shrink the poll cadence: asserting the never-listed path must not
        // burn the production 60s. (start_paused is unusable here — real
        // wiremock I/O would race the paused clock's auto-advance.)
        let mut p = params(&client, &fake, &dir, false, false);
        p.file_poll = FilePoll {
            interval: Duration::from_millis(10),
            timeout: Duration::from_millis(120),
        };

        let err = run_backup(&p).await.unwrap_err();
        assert!(
            matches!(err, BackupError::BackupTimeout(_)),
            "unexpected error: {err}"
        );
    }

    // -- Slow commands (legitimately >10s, must fit the 60s command timeout) -

    #[tokio::test]
    async fn mikrotik_backup_slow_save_succeeds_under_command_timeout() {
        let server = MockServer::start().await;
        // 11s > the 10s client default: only the 60s command timeout allows this.
        Mock::given(method("POST"))
            .and(path("/rest/system/backup/save"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("")
                    .set_delay(Duration::from_secs(11)),
            )
            .mount(&server)
            .await;
        let client = MikrotikClient::new(&conn_for(&server)).unwrap();
        client
            .backup_save("demo", Some("enc-password"))
            .await
            .expect("backup/save must succeed under the 60s command timeout");
    }

    #[tokio::test]
    async fn mikrotik_backup_slow_export_succeeds_under_command_timeout() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/rest/export"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string("")
                    .set_delay(Duration::from_secs(11)),
            )
            .mount(&server)
            .await;
        let client = MikrotikClient::new(&conn_for(&server)).unwrap();
        client
            .export_rsc("demo")
            .await
            .expect("/export must succeed under the 60s command timeout");
    }

    // -- Real russh/russh-sftp in-process roundtrip (dependency wiring) ------

    #[derive(Debug)]
    enum FixtureError {
        Russh(russh::Error),
        Message(String),
    }

    impl std::fmt::Display for FixtureError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                FixtureError::Russh(err) => write!(f, "{err}"),
                FixtureError::Message(msg) => write!(f, "{msg}"),
            }
        }
    }

    impl std::error::Error for FixtureError {}

    impl From<russh::Error> for FixtureError {
        fn from(err: russh::Error) -> Self {
            FixtureError::Russh(err)
        }
    }

    /// Unencrypted ed25519 host key (RFC 8410 §10.3 test vector, also used
    /// by russh's own key tests) — a fixture identity, never a real secret.
    const FIXTURE_HOST_KEY: &str = "-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEINTuctv5E1hK1bbY8fdp+K06/nwoy/HU++CXqI9EdVhC
-----END PRIVATE KEY-----";

    #[derive(Clone)]
    struct FixtureServer {
        file_name: Arc<String>,
        file_bytes: Arc<Vec<u8>>,
    }

    impl russh::server::Server for FixtureServer {
        type Handler = FixtureSession;

        fn new_client(&mut self, _peer: Option<std::net::SocketAddr>) -> Self::Handler {
            FixtureSession {
                clients: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
                file_name: self.file_name.clone(),
                file_bytes: self.file_bytes.clone(),
            }
        }
    }

    struct FixtureSession {
        clients: Arc<
            tokio::sync::Mutex<std::collections::HashMap<russh::ChannelId, russh::Channel<russh::server::Msg>>>,
        >,
        file_name: Arc<String>,
        file_bytes: Arc<Vec<u8>>,
    }

    impl russh::server::Handler for FixtureSession {
        type Error = FixtureError;

        async fn auth_password(
            &mut self,
            _user: &str,
            _password: &str,
        ) -> Result<russh::server::Auth, Self::Error> {
            Ok(russh::server::Auth::Accept)
        }

        async fn channel_open_session(
            &mut self,
            channel: russh::Channel<russh::server::Msg>,
            reply: russh::server::ChannelOpenHandle,
            _session: &mut russh::server::Session,
        ) -> Result<(), Self::Error> {
            self.clients.lock().await.insert(channel.id(), channel);
            reply.accept().await;
            Ok(())
        }

        async fn subsystem_request(
            &mut self,
            channel_id: russh::ChannelId,
            name: &str,
            session: &mut russh::server::Session,
        ) -> Result<(), Self::Error> {
            if name != "sftp" {
                session.channel_failure(channel_id)?;
                return Ok(());
            }
            let channel = self
                .clients
                .lock()
                .await
                .remove(&channel_id)
                .ok_or_else(|| FixtureError::Message("unknown channel".to_owned()))?;
            session.channel_success(channel_id)?;
            russh_sftp::server::run(
                channel.into_stream(),
                FixtureSftp {
                    file_name: self.file_name.clone(),
                    file_bytes: self.file_bytes.clone(),
                },
            )
            .await;
            Ok(())
        }
    }

    /// SFTP subsystem fixture serving exactly one file (read-only).
    struct FixtureSftp {
        file_name: Arc<String>,
        file_bytes: Arc<Vec<u8>>,
    }

    impl russh_sftp::server::Handler for FixtureSftp {
        type Error = russh_sftp::protocol::StatusCode;

        fn unimplemented(&self) -> Self::Error {
            russh_sftp::protocol::StatusCode::OpUnsupported
        }

        async fn init(
            &mut self,
            _version: u32,
            _extensions: std::collections::HashMap<String, String>,
        ) -> Result<russh_sftp::protocol::Version, Self::Error> {
            Ok(russh_sftp::protocol::Version::new())
        }

        async fn open(
            &mut self,
            id: u32,
            filename: String,
            _pflags: russh_sftp::protocol::OpenFlags,
            _attrs: russh_sftp::protocol::FileAttributes,
        ) -> Result<russh_sftp::protocol::Handle, Self::Error> {
            if filename.trim_start_matches('/') == self.file_name.as_str() {
                Ok(russh_sftp::protocol::Handle { id, handle: "h".to_owned() })
            } else {
                Err(russh_sftp::protocol::StatusCode::NoSuchFile)
            }
        }

        async fn close(
            &mut self,
            id: u32,
            _handle: String,
        ) -> Result<russh_sftp::protocol::Status, Self::Error> {
            Ok(russh_sftp::protocol::Status {
                id,
                status_code: russh_sftp::protocol::StatusCode::Ok,
                error_message: "Ok".to_owned(),
                language_tag: "en-US".to_owned(),
            })
        }

        async fn read(
            &mut self,
            id: u32,
            _handle: String,
            offset: u64,
            len: u32,
        ) -> Result<russh_sftp::protocol::Data, Self::Error> {
            if offset >= self.file_bytes.len() as u64 {
                return Err(russh_sftp::protocol::StatusCode::Eof);
            }
            let start = offset as usize;
            let end = (start + len as usize).min(self.file_bytes.len());
            Ok(russh_sftp::protocol::Data { id, data: self.file_bytes[start..end].to_vec() })
        }
    }

    #[tokio::test]
    async fn mikrotik_backup_real_russh_sftp_roundtrip() {
        let payload: Vec<u8> = (0u8..=255).cycle().take(300_000).collect();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let config = russh::server::Config {
            keys: vec![russh::keys::decode_secret_key(FIXTURE_HOST_KEY, None).unwrap()],
            auth_rejection_time: Duration::from_millis(100),
            auth_rejection_time_initial: Some(Duration::from_millis(0)),
            ..Default::default()
        };
        let file_name = Arc::new("demo.backup".to_owned());
        let file_bytes = Arc::new(payload.clone());
        let server_task = tokio::spawn({
            let listener = listener;
            let file_name = file_name.clone();
            let file_bytes = file_bytes.clone();
            async move {
                let mut server = FixtureServer { file_name, file_bytes };
                server
                    .run_on_socket(Arc::new(config), &listener)
                    .await
                    .map_err(|err| err.to_string())
            }
        });

        // Production client path: password auth + accept-unknown-host-key.
        let fetch = RusshSftpFetch;
        let bytes = fetch
            .fetch_file("127.0.0.1", port, "admin", "s3cr3t", "demo.backup")
            .await
            .expect("SFTP roundtrip must succeed");
        assert_eq!(bytes, payload, "downloaded bytes must be identical");

        // Unknown remote file → typed Sftp error (not a panic/abort).
        let err = fetch
            .fetch_file("127.0.0.1", port, "admin", "s3cr3t", "missing.backup")
            .await
            .unwrap_err();
        assert!(matches!(err, BackupError::Sftp(_)), "unexpected error: {err}");

        server_task.abort();
    }

    #[tokio::test]
    async fn mikrotik_backup_sftp_connect_refused_is_ssh_unreachable() {
        // Grab a port, drop the listener, and connect to the dead port.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);

        let err = RusshSftpFetch
            .fetch_file("127.0.0.1", port, "admin", "s3cr3t", "demo.backup")
            .await
            .unwrap_err();
        assert!(
            matches!(err, BackupError::SshUnreachable { .. }),
            "unexpected error: {err}"
        );
        assert!(err.to_string().contains("enable the SSH service"));
    }

    #[test]
    fn mikrotik_backup_result_dto_serializes_camel_case() {
        let dto = BackupResultDto {
            backup_path: "D:/backups/demo.backup".to_owned(),
            export_path: Some("D:/backups/demo.rsc".to_owned()),
            cleanup_warnings: vec!["w".to_owned()],
        };
        let value = serde_json::to_value(&dto).unwrap();
        assert!(value.get("backupPath").is_some());
        assert!(value.get("exportPath").is_some());
        assert!(value.get("cleanupWarnings").is_some());
        let err = BackupError::OutputExists("x".to_owned());
        let value = serde_json::to_value(&err).unwrap();
        assert_eq!(value["kind"], "OutputExists");
        assert!(value["message"].as_str().unwrap().contains("x"));
    }
}

