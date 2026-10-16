//! Encrypted backup of an existing saved OPNsense configuration revision.
//!
//! Both device calls are read-only HTTP GET (`/api/core/backup/backups/this`
//! lists already-stored revisions; `/api/core/backup/download/this/<id>`
//! downloads one). The app NEVER creates remote revisions, reverts or
//! deletes anything on the firewall.
//!
//! The downloaded XML is validated (non-empty, not an HTML error page, root
//! element `opnsense`) and written to disk ONLY in passphrase-encrypted age
//! format (`<name>.xml.age`): plaintext XML never touches disk or SQLite.
//! Decrypt outside the app with `age -d -o config.xml <file>.xml.age` and
//! import the XML in the OPNsense GUI.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use thiserror::Error;

use crate::db::DbError;
use crate::opnsense::client::{OpnsenseApi, OpnsenseError};
use crate::opnsense::manager::OpnsenseManager;
use crate::opnsense::types::{
    OpnsenseApiFactory, OpnsenseBackupResultDto, OpnsenseManagerError, OpnsenseRevisionDto,
};
use crate::secrets::SecretError;

// ---------------------------------------------------------------------------
// Typed errors
// ---------------------------------------------------------------------------

/// Typed errors for the saved-revision backup flow; serialized to the
/// frontend as `{ kind, message }` (same contract as
/// `mikrotik::backup::BackupError`).
#[derive(Debug, Error)]
pub enum OpnsenseBackupError {
    /// `backup_name` violates the shared filename rule.
    #[error(
        "invalid backup name {0:?}: must match ^[A-Za-z0-9][A-Za-z0-9._-]{{0,63}}$ \
         and must not be a Windows device name (CON, PRN, AUX, NUL, COM1-9, LPT1-9)"
    )]
    InvalidName(String),

    /// `destination_dir` does not exist or is not a directory.
    #[error("destination directory invalid: {0}")]
    DestinationInvalid(String),

    /// The output `<name>.xml.age` already exists.
    #[error("output already exists: {0}")]
    OutputExists(String),

    /// Writing or encrypting the output file locally failed.
    #[error("local write failed for {path}: {message}")]
    LocalWrite { path: String, message: String },

    /// The passphrase was empty or whitespace-only.
    #[error("backup passphrase must not be blank")]
    PassphraseBlank,

    /// The passphrase failed the strength rule (see
    /// [`validate_passphrase`]). Unencrypted downloads are not offered at
    /// all, so a weak passphrase would be the only protection failure mode.
    #[error(
        "weak backup passphrase: use at least 12 characters with a mix of \
         letters, digits or symbols"
    )]
    PassphraseWeak,

    /// The firewall reported no saved configuration revisions.
    #[error("no saved configuration revision on the firewall")]
    NoRevisions,

    /// A revision id was not a single `config-*.xml` filename.
    #[error("invalid revision id {0:?}: expected a single config-*.xml filename")]
    InvalidRevisionId(String),

    /// The downloaded revision was empty, an HTML error page, or XML whose
    /// root element is not `opnsense`.
    #[error("downloaded configuration is not a valid OPNsense config XML: {0}")]
    InvalidConfig(String),

    /// `profile_id` did not match a stored profile.
    #[error("no OPNsense profile with id {0}")]
    ProfileNotFound(i64),

    /// Read-only API client failure (reuses the typed client errors).
    #[error("OPNsense API error: {0}")]
    Api(#[from] OpnsenseError),

    /// Keyring lookup for the API credentials failed.
    #[error("credential store error: {0}")]
    Secret(#[from] SecretError),

    /// SQLite failure while loading the profile.
    #[error("database error: {0}")]
    Db(#[from] DbError),
}

impl OpnsenseBackupError {
    /// Stable machine-readable discriminator for the frontend.
    pub fn kind(&self) -> &'static str {
        match self {
            OpnsenseBackupError::InvalidName(_) => "InvalidName",
            OpnsenseBackupError::DestinationInvalid(_) => "DestinationInvalid",
            OpnsenseBackupError::OutputExists(_) => "OutputExists",
            OpnsenseBackupError::LocalWrite { .. } => "LocalWrite",
            OpnsenseBackupError::PassphraseBlank => "PassphraseBlank",
            OpnsenseBackupError::PassphraseWeak => "PassphraseWeak",
            OpnsenseBackupError::NoRevisions => "NoRevisions",
            OpnsenseBackupError::InvalidRevisionId(_) => "InvalidRevisionId",
            OpnsenseBackupError::InvalidConfig(_) => "InvalidConfig",
            OpnsenseBackupError::ProfileNotFound(_) => "ProfileNotFound",
            OpnsenseBackupError::Api(err) => match err {
                OpnsenseError::Connect(_) => "Connect",
                OpnsenseError::Timeout(_) => "Timeout",
                OpnsenseError::Tls(_) => "Tls",
                OpnsenseError::Unauthorized => "Unauthorized",
                OpnsenseError::Forbidden => "Forbidden",
                OpnsenseError::NotFound(_) => "NotFound",
                OpnsenseError::Http { .. } => "Http",
                OpnsenseError::Parse(_) => "Parse",
                OpnsenseError::TooLarge(_) => "TooLarge",
            },
            OpnsenseBackupError::Secret(err) => match err {
                SecretError::NotStored => "CredentialsNotStored",
                SecretError::Keyring(_) => "Keyring",
            },
            OpnsenseBackupError::Db(_) => "Db",
        }
    }
}

impl Serialize for OpnsenseBackupError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("OpnsenseBackupError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Validate `backup_name` BEFORE any device call, mapping the shared
/// validator's error into this flow's typed error.
fn validate_backup_name(name: &str) -> Result<(), OpnsenseBackupError> {
    crate::backup_filename::validate_backup_name(name)
        .map_err(|err| OpnsenseBackupError::InvalidName(err.0))
}

/// Passphrase policy: at least 12 characters covering at least two
/// character classes (lower/upper/digit/symbol). Blank and weak secrets
/// are rejected before any device call; the frontend mirrors this rule in
/// `OpnsenseView.tsx` (`passphraseStrength`).
fn validate_passphrase(passphrase: &str) -> Result<(), OpnsenseBackupError> {
    if passphrase.trim().is_empty() {
        return Err(OpnsenseBackupError::PassphraseBlank);
    }
    let classes = [
        passphrase.chars().any(|c| c.is_ascii_lowercase()),
        passphrase.chars().any(|c| c.is_ascii_uppercase()),
        passphrase.chars().any(|c| c.is_ascii_digit()),
        passphrase.chars().any(|c| !c.is_alphanumeric()),
    ];
    let class_count = classes.iter().filter(|present| **present).count();
    if passphrase.chars().count() < 12 || class_count < 2 {
        return Err(OpnsenseBackupError::PassphraseWeak);
    }
    Ok(())
}

/// A revision id must be a single `config-*.xml` filename: no path
/// separators, no traversal, non-empty stem of `[A-Za-z0-9._-]`.
fn valid_revision_id(id: &str) -> bool {
    let Some(rest) = id.strip_prefix("config-") else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".xml") else {
        return false;
    };
    !stem.is_empty()
        && stem
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// The trimmed start of an HTML error page (case-insensitive).
fn looks_like_html(bytes: &[u8]) -> bool {
    let head: String = bytes
        .iter()
        .take(128)
        .map(|&b| char::from(b))
        .collect::<String>()
        .to_lowercase();
    let trimmed = head.trim_start();
    trimmed.starts_with("<!doctype") || trimmed.starts_with("<html")
}

/// Reject anything that is not a non-empty, non-HTML XML document whose
/// root element is `opnsense`.
fn validate_config_xml(bytes: &[u8]) -> Result<(), OpnsenseBackupError> {
    if bytes.is_empty() {
        return Err(OpnsenseBackupError::InvalidConfig(
            "the download was empty".to_owned(),
        ));
    }
    if looks_like_html(bytes) {
        return Err(OpnsenseBackupError::InvalidConfig(
            "the response looks like an HTML error page, not XML".to_owned(),
        ));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| {
        OpnsenseBackupError::InvalidConfig("the bytes are not valid UTF-8".to_owned())
    })?;
    let doc = roxmltree::Document::parse(text)
        .map_err(|err| OpnsenseBackupError::InvalidConfig(format!("XML parse failed: {err}")))?;
    let root = doc.root_element().tag_name().name();
    if root != "opnsense" {
        return Err(OpnsenseBackupError::InvalidConfig(format!(
            "root element is {root:?}, expected \"opnsense\""
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Local encryption
// ---------------------------------------------------------------------------

/// Build the typed `LocalWrite` for `path` from any IO error text.
fn local_write(path: &Path, err: impl std::fmt::Display) -> OpnsenseBackupError {
    OpnsenseBackupError::LocalWrite {
        path: path.display().to_string(),
        message: err.to_string(),
    }
}

/// Encrypt `bytes` to `<path>` (passphrase-protected age format,
/// `.xml.age`). The output file is created with `create_new` (an existing
/// file → `OutputExists`) and removed on EVERY failure so no incomplete
/// ciphertext is left behind. Plaintext never touches the disk.
fn encrypt_to_file(path: &Path, bytes: &[u8], passphrase: &str) -> Result<(), OpnsenseBackupError> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::AlreadyExists {
                OpnsenseBackupError::OutputExists(path.display().to_string())
            } else {
                local_write(path, err)
            }
        })?;

    let result = (|| {
        let encryptor = age::Encryptor::with_user_passphrase(age::secrecy::SecretString::new(
            passphrase.to_owned().into(),
        ));
        let mut writer = encryptor
            .wrap_output(file)
            .map_err(|err| local_write(path, err))?;
        writer
            .write_all(bytes)
            .map_err(|err| local_write(path, err))?;
        let mut file = writer.finish().map_err(|err| local_write(path, err))?;
        file.flush().map_err(|err| local_write(path, err))?;
        file.sync_all().map_err(|err| local_write(path, err))?;
        Ok(())
    })();

    if result.is_err() {
        // Never leave a partial `.xml.age` behind.
        let _ = std::fs::remove_file(path);
    }
    result
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

/// Saved-revision backup orchestration over an [`OpnsenseManager`]. Cheap
/// to construct; clones the manager handle and its API factory.
#[derive(Clone)]
pub struct OpnsenseBackupManager {
    manager: OpnsenseManager,
    factory: OpnsenseApiFactory,
}

impl OpnsenseBackupManager {
    pub fn new(manager: &OpnsenseManager) -> Self {
        Self {
            manager: manager.clone(),
            factory: manager.api_factory(),
        }
    }

    /// Build a per-profile read-only API handle, mapping profile-setup
    /// failures into this flow's typed error.
    async fn api_for(&self, profile_id: i64) -> Result<Arc<dyn OpnsenseApi>, OpnsenseBackupError> {
        let profile = self
            .manager
            .require_profile(profile_id)
            .await
            .map_err(setup_error)?;
        let conn = self
            .manager
            .connection_for(&profile)
            .await
            .map_err(setup_error)?;
        (self.factory)(conn).await.map_err(setup_error)
    }

    /// `GET /api/core/backup/backups/this` — saved config revisions,
    /// newest first by `time_iso` (entries without a timestamp sort last,
    /// stably).
    pub async fn list_revisions(
        &self,
        profile_id: i64,
    ) -> Result<Vec<OpnsenseRevisionDto>, OpnsenseManagerError> {
        let profile = self.manager.require_profile(profile_id).await?;
        let conn = self.manager.connection_for(&profile).await?;
        let api = (self.factory)(conn).await?;
        let mut revisions = api.backup_revisions().await?;
        sort_newest_first(&mut revisions);
        Ok(revisions)
    }

    /// Download a saved configuration revision and store it encrypted
    /// (`<backup_name>.xml.age`) in `destination_dir`. `revision_id` picks
    /// a specific saved revision; `None` takes the LATEST saved one.
    ///
    /// All local validation (passphrase, name, destination) happens BEFORE
    /// any device call. The app only reads existing remote revisions; it
    /// never creates one.
    pub async fn backup(
        &self,
        profile_id: i64,
        destination_dir: &str,
        backup_name: &str,
        passphrase: &str,
        revision_id: Option<&str>,
    ) -> Result<OpnsenseBackupResultDto, OpnsenseBackupError> {
        validate_passphrase(passphrase)?;
        validate_backup_name(backup_name)?;
        if let Some(id) = revision_id {
            if !valid_revision_id(id) {
                return Err(OpnsenseBackupError::InvalidRevisionId(id.to_owned()));
            }
        }
        let destination = PathBuf::from(destination_dir);
        if !destination.is_dir() {
            return Err(OpnsenseBackupError::DestinationInvalid(format!(
                "{destination_dir:?} does not exist or is not a directory"
            )));
        }
        let output_path = destination.join(format!("{backup_name}.xml.age"));

        // Device calls start here — read-only GETs only.
        let api = self.api_for(profile_id).await?;
        let mut revisions = api.backup_revisions().await?;
        if revisions.is_empty() {
            return Err(OpnsenseBackupError::NoRevisions);
        }
        sort_newest_first(&mut revisions);
        let source = match revision_id {
            Some(id) => revisions
                .iter()
                .find(|rev| rev.id == id)
                .ok_or_else(|| OpnsenseBackupError::InvalidRevisionId(id.to_owned()))?,
            None => &revisions[0],
        };
        if !valid_revision_id(&source.id) {
            return Err(OpnsenseBackupError::InvalidRevisionId(source.id.clone()));
        }
        let bytes = api.download_revision(&source.id).await?;
        validate_config_xml(&bytes)?;

        // Encrypt off the async runtime; the passphrase is moved in,
        // dropped when the task ends, and never stored anywhere.
        let revision_id = source.id.clone();
        let source_time_iso = source.time_iso.clone();
        let path_string = output_path.display().to_string();
        let passphrase = passphrase.to_owned();
        tokio::task::spawn_blocking(move || encrypt_to_file(&output_path, &bytes, &passphrase))
            .await
            .map_err(|err| OpnsenseBackupError::LocalWrite {
                path: path_string.clone(),
                message: format!("encryption task failed: {err}"),
            })??;

        Ok(OpnsenseBackupResultDto {
            path: path_string,
            source_revision_id: revision_id,
            source_time_iso,
        })
    }
}

/// Newest first by `time_iso`; entries without a timestamp sort last.
/// Stable, so same-timestamp (or timestamp-less) entries keep the
/// firewall's original relative order.
fn sort_newest_first(revisions: &mut [OpnsenseRevisionDto]) {
    revisions.sort_by(|a, b| b.time_iso.cmp(&a.time_iso));
}

/// Map profile-setup failures (profile load, credential read, factory
/// construction) into this flow's typed error. `Db`/`Secret`/`Api` keep
/// their fidelity; the rest are unreachable for a DB-loaded profile and
/// collapse into a parse-style `Api` error with the original message.
fn setup_error(err: OpnsenseManagerError) -> OpnsenseBackupError {
    match err {
        OpnsenseManagerError::ProfileNotFound(id) => OpnsenseBackupError::ProfileNotFound(id),
        OpnsenseManagerError::Db(err) => OpnsenseBackupError::Db(err),
        OpnsenseManagerError::NotStored => OpnsenseBackupError::Secret(SecretError::NotStored),
        OpnsenseManagerError::Api(err) => OpnsenseBackupError::Api(err),
        other => OpnsenseBackupError::Api(OpnsenseError::Parse(format!(
            "profile setup failed: {other}"
        ))),
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// `opnsense_list_revisions(profile_id)` — saved config revisions, newest
/// first. Read-only GET; purely remote state, nothing is created.
#[tauri::command]
pub async fn opnsense_list_revisions(
    manager: tauri::State<'_, OpnsenseManager>,
    profile_id: i64,
) -> Result<Vec<OpnsenseRevisionDto>, OpnsenseManagerError> {
    OpnsenseBackupManager::new(&manager)
        .list_revisions(profile_id)
        .await
}

/// `opnsense_backup(profile_id, destination_dir, backup_name, passphrase,
/// revision_id?)` — download a saved revision (the LATEST when
/// `revision_id` is None) via read-only GET and store it encrypted as
/// `<backup_name>.xml.age` in the user-picked existing directory. The
/// passphrase is strength-checked, used for this one encryption and never
/// stored; the result says the source is a saved revision, possibly older
/// than unsaved changes on the device.
#[tauri::command]
pub async fn opnsense_backup(
    manager: tauri::State<'_, OpnsenseManager>,
    profile_id: i64,
    destination_dir: String,
    backup_name: String,
    passphrase: String,
    revision_id: Option<String>,
) -> Result<OpnsenseBackupResultDto, OpnsenseBackupError> {
    OpnsenseBackupManager::new(&manager)
        .backup(
            profile_id,
            &destination_dir,
            &backup_name,
            &passphrase,
            revision_id.as_deref(),
        )
        .await
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use parking_lot::Mutex as StdMutex;

    use crate::db::Database;
    use crate::opnsense::client::{
        FirmwareInfoPayload, InterfaceInfoPage, PfRuleId, RuleSearchPage, RuleStat, SseByteStream,
        SystemTimePayload,
    };
    use crate::opnsense::types::{
        CreateOpnsenseProfileRequest, OpnsenseIpsecSessionDto, OpnsenseOpenvpnSessionDto,
        OpnsensePfStatesDto, OpnsenseResourcesDto, OpnsenseTailscaleStatusDto,
        OpnsenseWireguardStatusDto,
    };
    use crate::secrets::MemoryStore;

    const XML: &str = "<?xml version=\"1.0\"?>\n<opnsense version=\"24.7\">\n  <system>\n    <hostname>fw</hostname>\n  </system>\n</opnsense>\n";

    // -- Test filesystem harness -------------------------------------------

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "opnsense-backup-{tag}-{}-{:016x}",
                std::process::id(),
                rand::random::<u64>()
            ));
            std::fs::create_dir(&dir).expect("create unique temp dir");
            TempDir(dir)
        }

        /// Fresh empty subdirectory — used as the backup destination so the
        /// "no plaintext left behind" assertion scans only output files.
        fn subdir(&self, name: &str) -> PathBuf {
            let dir = self.0.join(name);
            std::fs::create_dir_all(&dir).expect("create subdir");
            dir
        }

        fn db_file(&self) -> PathBuf {
            self.0.join("db.sqlite")
        }

        fn entries(&self, subdir: &str) -> Vec<std::ffi::OsString> {
            std::fs::read_dir(self.0.join(subdir))
                .expect("read dir")
                .map(|entry| entry.expect("entry").file_name())
                .collect()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // -- Fake API ------------------------------------------------------------

    /// Injected fake: serves scripted revisions + download bytes and counts
    /// device calls so tests can assert local validation never touched the
    /// network. All monitoring endpoints are unused here.
    struct FakeApi {
        revisions: Vec<OpnsenseRevisionDto>,
        download: Vec<u8>,
        calls: StdMutex<Vec<String>>,
    }

    impl FakeApi {
        fn new(revisions: Vec<OpnsenseRevisionDto>, download: Vec<u8>) -> Self {
            Self {
                revisions,
                download,
                calls: StdMutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().len()
        }
    }

    #[async_trait::async_trait]
    impl OpnsenseApi for FakeApi {
        async fn backup_revisions(&self) -> Result<Vec<OpnsenseRevisionDto>, OpnsenseError> {
            self.calls.lock().push("backup_revisions".to_owned());
            Ok(self.revisions.clone())
        }

        async fn download_revision(&self, id: &str) -> Result<Vec<u8>, OpnsenseError> {
            self.calls.lock().push(format!("download_revision:{id}"));
            Ok(self.download.clone())
        }

        // Monitoring endpoints are unused by the backup flow: panic if the
        // manager ever calls one so tests catch scope creep.
        async fn get_system_status(&self) -> Result<(), OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn search_rules(&self) -> Result<RuleSearchPage, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn rule_stats(&self) -> Result<Vec<(String, RuleStat)>, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn list_rule_ids(&self) -> Result<Vec<PfRuleId>, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn interfaces_info(&self) -> Result<InterfaceInfoPage, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn interface_statistics(
            &self,
        ) -> Result<Vec<crate::opnsense::types::OpnsenseInterfaceDto>, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn pf_states(&self) -> Result<OpnsensePfStatesDto, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn system_resources(&self) -> Result<OpnsenseResourcesDto, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn system_time(&self) -> Result<SystemTimePayload, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn cpu_usage_sample(&self) -> Result<f64, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn wireguard_show(&self) -> Result<OpnsenseWireguardStatusDto, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn openvpn_sessions(&self) -> Result<Vec<OpnsenseOpenvpnSessionDto>, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn ipsec_phase1(&self) -> Result<Vec<OpnsenseIpsecSessionDto>, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn tailscale_status(&self) -> Result<OpnsenseTailscaleStatusDto, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn tailscale_service_status(&self) -> Result<Option<bool>, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn firmware_info(&self) -> Result<FirmwareInfoPayload, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }

        async fn open_log_stream(&self) -> Result<SseByteStream, OpnsenseError> {
            unimplemented!("FakeApi only serves backup endpoints")
        }
    }

    // -- Harness wiring --------------------------------------------------------

    fn factory_for(api: Arc<FakeApi>) -> OpnsenseApiFactory {
        Arc::new(move |_conn| {
            let api = Arc::clone(&api);
            Box::pin(async move { Ok(api as Arc<dyn OpnsenseApi>) })
        })
    }

    async fn manager_for(api: Arc<FakeApi>) -> (TempDir, OpnsenseManager) {
        let dir = TempDir::new("mgr");
        let db = Database::connect(&dir.db_file()).await.expect("db");
        let manager = OpnsenseManager::new(db, Arc::new(MemoryStore::new()), factory_for(api));
        (dir, manager)
    }

    async fn profile(manager: &OpnsenseManager) -> i64 {
        manager
            .create_profile(&CreateOpnsenseProfileRequest {
                name: "fw".to_owned(),
                host: "fw.example.com".to_owned(),
                port: 443,
                allow_invalid_certs: false,
                ssh_port: 22,
            })
            .await
            .expect("create profile")
            .id
    }

    fn revision(id: &str, time_iso: Option<&str>) -> OpnsenseRevisionDto {
        OpnsenseRevisionDto {
            id: id.to_owned(),
            time_iso: time_iso.map(str::to_owned),
            description: None,
            filesize: None,
        }
    }

    fn two_revisions() -> Vec<OpnsenseRevisionDto> {
        vec![
            revision("config-1735689600.xml", Some("2025-01-01T00:00:00Z")),
            revision("config-1735776000.xml", Some("2025-01-02T00:00:00Z")),
        ]
    }

    /// Decrypt a passphrase-protected age file and return the plaintext.
    fn decrypt_age(path: &Path, passphrase: &str) -> Vec<u8> {
        let file = std::fs::File::open(path).expect("open age file");
        let decryptor = age::Decryptor::new(std::io::BufReader::new(file)).expect("age header");
        let identity = age::scrypt::Identity::new(age::secrecy::SecretString::new(
            passphrase.to_owned().into(),
        ));
        let mut reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .expect("decrypt");
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut out).expect("read");
        out
    }

    // -- Tests ---------------------------------------------------------------

    #[tokio::test]
    async fn backup_happy_path_writes_only_encrypted_output() {
        let api = Arc::new(FakeApi::new(two_revisions(), XML.as_bytes().to_vec()));
        let (dir, manager) = manager_for(Arc::clone(&api)).await;
        let profile_id = profile(&manager).await;
        manager
            .set_credentials(profile_id, "key", "secret")
            .await
            .expect("credentials");
        let out = dir.subdir("out");

        let result = OpnsenseBackupManager::new(&manager)
            .backup(
                profile_id,
                out.to_str().unwrap(),
                "fw-backup",
                "correct horse",
                None,
            )
            .await
            .expect("backup");

        assert!(result.path.ends_with("fw-backup.xml.age"));
        assert_eq!(result.source_revision_id, "config-1735776000.xml");
        assert_eq!(
            result.source_time_iso.as_deref(),
            Some("2025-01-02T00:00:00Z")
        );

        // Exactly one file in the destination: the `.xml.age`. Decrypting
        // with the same passphrase yields the exact original XML bytes.
        let entries = dir.entries("out");
        assert_eq!(entries.len(), 1, "no plaintext or partial files allowed");
        let plaintext = decrypt_age(Path::new(&result.path), "correct horse");
        assert_eq!(plaintext, XML.as_bytes());
    }

    #[tokio::test]
    async fn local_validation_fails_before_any_device_call() {
        let api = Arc::new(FakeApi::new(two_revisions(), XML.as_bytes().to_vec()));
        let (dir, manager) = manager_for(Arc::clone(&api)).await;
        let profile_id = profile(&manager).await;
        let out = dir.subdir("out");
        let out_str = out.to_str().unwrap().to_owned();
        let backups = OpnsenseBackupManager::new(&manager);

        let err = backups
            .backup(profile_id, &out_str, "fw-backup", "   ", None)
            .await
            .expect_err("blank passphrase must fail");
        assert!(matches!(err, OpnsenseBackupError::PassphraseBlank));

        for weak in ["pw", "abcdefghijkl", "123456789012"] {
            let err = backups
                .backup(profile_id, &out_str, "fw-backup", weak, None)
                .await
                .expect_err("weak passphrase must fail");
            assert!(
                matches!(err, OpnsenseBackupError::PassphraseWeak),
                "{weak:?}: got {err:?}"
            );
        }

        let err = backups
            .backup(profile_id, &out_str, "bad name!", "correct horse", None)
            .await
            .expect_err("invalid name must fail");
        assert!(matches!(err, OpnsenseBackupError::InvalidName(_)));

        let err = backups
            .backup(
                profile_id,
                &dir.0.join("no-such-dir").to_string_lossy(),
                "fw-backup",
                "correct horse",
                None,
            )
            .await
            .expect_err("missing destination must fail");
        assert!(matches!(err, OpnsenseBackupError::DestinationInvalid(_)));

        assert_eq!(
            api.call_count(),
            0,
            "local validation must not touch the device"
        );
        assert!(dir.entries("out").is_empty());
    }

    #[tokio::test]
    async fn backup_refuses_to_overwrite_existing_output() {
        let api = Arc::new(FakeApi::new(two_revisions(), XML.as_bytes().to_vec()));
        let (dir, manager) = manager_for(Arc::clone(&api)).await;
        let profile_id = profile(&manager).await;
        manager
            .set_credentials(profile_id, "key", "secret")
            .await
            .expect("credentials");
        let out = dir.subdir("out");
        let output = out.join("fw-backup.xml.age");
        std::fs::write(&output, b"pre-existing").expect("pre-create output");

        let err = OpnsenseBackupManager::new(&manager)
            .backup(
                profile_id,
                out.to_str().unwrap(),
                "fw-backup",
                "correct horse",
                None,
            )
            .await
            .expect_err("existing output must fail");

        assert!(matches!(err, OpnsenseBackupError::OutputExists(_)));
        assert_eq!(
            std::fs::read(&output).expect("read"),
            b"pre-existing",
            "the pre-existing file must be untouched"
        );
    }

    #[tokio::test]
    async fn backup_rejects_profile_not_found() {
        let api = Arc::new(FakeApi::new(two_revisions(), XML.as_bytes().to_vec()));
        let (dir, manager) = manager_for(api).await;
        let out = dir.subdir("out");

        let err = OpnsenseBackupManager::new(&manager)
            .backup(
                9999,
                out.to_str().unwrap(),
                "fw-backup",
                "correct horse",
                None,
            )
            .await
            .expect_err("unknown profile must fail");
        assert!(matches!(err, OpnsenseBackupError::ProfileNotFound(9999)));
    }

    #[tokio::test]
    async fn backup_rejects_html_and_wrong_root_without_output() {
        for (tag, body) in [
            (
                "html",
                b"<!DOCTYPE html><html><body>login</body></html>".to_vec(),
            ),
            (
                "wrong-root",
                b"<?xml version=\"1.0\"?><notopnsense/>".to_vec(),
            ),
            ("empty", Vec::new()),
        ] {
            let api = Arc::new(FakeApi::new(two_revisions(), body));
            let (dir, manager) = manager_for(Arc::clone(&api)).await;
            let profile_id = profile(&manager).await;
            manager
                .set_credentials(profile_id, "key", "secret")
                .await
                .expect("credentials");
            let out = dir.subdir("out");

            let err = OpnsenseBackupManager::new(&manager)
                .backup(
                    profile_id,
                    out.to_str().unwrap(),
                    "fw-backup",
                    "correct horse",
                    None,
                )
                .await
                .expect_err("invalid config must fail");
            assert!(
                matches!(err, OpnsenseBackupError::InvalidConfig(_)),
                "{tag}: got {err:?}"
            );
            assert!(
                dir.entries("out").is_empty(),
                "{tag}: no output file may remain"
            );
        }
    }

    #[tokio::test]
    async fn backup_rejects_unsafe_revision_ids() {
        for id in ["../evil.xml", "foo.txt", "config-.xml", "config-a/b.xml"] {
            let api = Arc::new(FakeApi::new(
                vec![revision(id, Some("2025-01-02T00:00:00Z"))],
                XML.as_bytes().to_vec(),
            ));
            let (dir, manager) = manager_for(Arc::clone(&api)).await;
            let profile_id = profile(&manager).await;
            manager
                .set_credentials(profile_id, "key", "secret")
                .await
                .expect("credentials");
            let out = dir.subdir("out");

            let err = OpnsenseBackupManager::new(&manager)
                .backup(
                    profile_id,
                    out.to_str().unwrap(),
                    "fw-backup",
                    "correct horse",
                    None,
                )
                .await
                .expect_err("bad revision id must fail");
            assert!(
                matches!(&err, OpnsenseBackupError::InvalidRevisionId(bad) if bad == id),
                "{id:?}: got {err:?}"
            );
            assert!(dir.entries("out").is_empty());
        }
    }

    #[tokio::test]
    async fn backup_reports_no_revisions() {
        let api = Arc::new(FakeApi::new(Vec::new(), Vec::new()));
        let (dir, manager) = manager_for(Arc::clone(&api)).await;
        let profile_id = profile(&manager).await;
        manager
            .set_credentials(profile_id, "key", "secret")
            .await
            .expect("credentials");
        let out = dir.subdir("out");

        let err = OpnsenseBackupManager::new(&manager)
            .backup(
                profile_id,
                out.to_str().unwrap(),
                "fw-backup",
                "correct horse",
                None,
            )
            .await
            .expect_err("empty revision list must fail");
        assert!(matches!(err, OpnsenseBackupError::NoRevisions));
        assert!(dir.entries("out").is_empty());
    }

    #[tokio::test]
    async fn backup_downloads_the_selected_revision() {
        let api = Arc::new(FakeApi::new(two_revisions(), XML.as_bytes().to_vec()));
        let (dir, manager) = manager_for(Arc::clone(&api)).await;
        let profile_id = profile(&manager).await;
        manager
            .set_credentials(profile_id, "key", "secret")
            .await
            .expect("credentials");
        let out = dir.subdir("out");

        let result = OpnsenseBackupManager::new(&manager)
            .backup(
                profile_id,
                out.to_str().unwrap(),
                "fw-backup",
                "correct horse",
                Some("config-1735689600.xml"),
            )
            .await
            .expect("backup");
        assert_eq!(result.source_revision_id, "config-1735689600.xml");
        assert_eq!(
            result.source_time_iso.as_deref(),
            Some("2025-01-01T00:00:00Z")
        );
        let plaintext = decrypt_age(Path::new(&result.path), "correct horse");
        assert_eq!(plaintext, XML.as_bytes());

        let err = OpnsenseBackupManager::new(&manager)
            .backup(
                profile_id,
                out.to_str().unwrap(),
                "fw-backup-2",
                "correct horse",
                Some("config-9999999999.xml"),
            )
            .await
            .expect_err("unknown revision must fail");
        assert!(matches!(err, OpnsenseBackupError::InvalidRevisionId(_)));
    }

    #[tokio::test]
    async fn list_revisions_sorts_newest_first_timestampless_last() {
        let api = Arc::new(FakeApi::new(
            vec![
                revision("config-old.xml", Some("2025-01-01T00:00:00Z")),
                revision("config-undated.xml", None),
                revision("config-new.xml", Some("2025-01-03T00:00:00Z")),
                revision("config-mid.xml", Some("2025-01-02T00:00:00Z")),
            ],
            Vec::new(),
        ));
        let (_dir, manager) = manager_for(Arc::clone(&api)).await;
        let profile_id = profile(&manager).await;
        manager
            .set_credentials(profile_id, "key", "secret")
            .await
            .expect("credentials");

        let revisions = OpnsenseBackupManager::new(&manager)
            .list_revisions(profile_id)
            .await
            .expect("list revisions");

        let ids: Vec<&str> = revisions.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "config-new.xml",
                "config-mid.xml",
                "config-old.xml",
                "config-undated.xml"
            ]
        );
    }
}
