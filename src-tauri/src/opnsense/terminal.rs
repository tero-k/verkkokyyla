//! Interactive SSH terminals to OPNsense firewalls: one russh shell channel
//! with a PTY per terminal, keyed by a random app-side terminal id.
//!
//! Terminals authenticate with a SEPARATE SSH user/password entered by the
//! user each time — never the API key/secret, which is only valid for
//! read-only HTTP. The password is used for this one login and never
//! stored, logged, or written to SQLite.
//!
//! Host trust is explicit — MikroTik's accept-anything policy is NOT
//! copied: the SHA-256 host-key fingerprint (OpenSSH `SHA256:...` form)
//! must be confirmed by the user on first connect and is then pinned in
//! `opnsense_profiles.ssh_host_key`. A changed key is a hard
//! `HostKeyMismatch` with no one-click override. Key rejection happens
//! BEFORE any password authentication, so a wrong or unconfirmed key can
//! never leak credentials to an unexpected host.
//!
//! Output reaches the UI as base64 chunks over a Tauri Channel (the IPC
//! layer is JSON; SSH output is arbitrary bytes). Input travels the same
//! way in reverse. Lifecycle: `open` → `write`/`resize` → `close`; a server
//! that drops the channel cleans the slot up on its own (reader exit).

use std::collections::HashMap;
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use parking_lot::Mutex as SyncMutex;
use rand::RngCore;
use russh::client::Msg as ClientMsg;
use russh::keys::{HashAlg, PublicKeyOrCertificate};
use russh::{ChannelMsg, ChannelWriteHalf};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use super::manager::{OpnsenseActivityRegistry, OpnsenseManager};
use super::types::{OpnsenseManagerError, OpnsenseTerminalOpenDto};
use crate::db::Database;
use crate::opnsense::client::OpnsenseError;

const TERM: &str = "xterm-256color";
/// The 8-terminal concurrency cap, matching the MikroTik terminal manager.
const MAX_TERMINALS: usize = 8;
/// One keystroke batch from xterm is tiny; this only bounds a hostile or
/// broken caller from pushing giant frames through decode + SSH.
const MAX_WRITE_BYTES: usize = 64 * 1024;

/// Base64-encoded chunk of terminal output, forwarded to the UI channel.
pub type OpnsenseTerminalDataSink = Arc<dyn Fn(String) + Send + Sync>;

/// What the host-key check decided during the SSH handshake. Recorded
/// before russh is told to accept/reject so `open()` can map a failed
/// handshake onto the exact typed error (the handshake itself only yields
/// a generic `UnknownKey`).
#[derive(Clone, Debug, PartialEq, Eq)]
enum HostKeyOutcome {
    /// Fingerprint matched (pinned or explicitly confirmed).
    Accepted(String),
    /// No pin yet and the presented key was not confirmed by the user.
    Unknown(String),
    /// Presented key differs from the pinned one.
    Mismatch { expected: String, actual: String },
}

/// russh client handler implementing the OPNsense host-trust policy.
///
/// A pinned key must match exactly; without a pin the presented fingerprint
/// must equal the user-confirmed one. Anything else rejects the key, which
/// aborts the handshake before authentication ever starts.
struct VerifyHostKey {
    outcome: Arc<SyncMutex<Option<HostKeyOutcome>>>,
    pinned: Option<String>,
    confirmed: Option<String>,
}

impl russh::client::Handler for VerifyHostKey {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let fingerprint = server_public_key
            .public_key()
            .fingerprint(HashAlg::Sha256)
            .to_string();
        let accepted = match (&self.pinned, &self.confirmed) {
            (Some(pinned), _) if pinned == &fingerprint => true,
            (Some(pinned), _) => {
                *self.outcome.lock() = Some(HostKeyOutcome::Mismatch {
                    expected: pinned.clone(),
                    actual: fingerprint.clone(),
                });
                false
            }
            (None, Some(confirmed)) if confirmed == &fingerprint => true,
            (None, _) => {
                *self.outcome.lock() = Some(HostKeyOutcome::Unknown(fingerprint.clone()));
                false
            }
        };
        if accepted {
            *self.outcome.lock() = Some(HostKeyOutcome::Accepted(fingerprint));
        }
        Ok(accepted)
    }
}

/// The terminal manager owns the active-terminal map; the activity
/// registry (cloned from `OpnsenseManager`) marks profiles with a live
/// terminal so profile deletion can be refused while one is open.
#[derive(Clone)]
pub struct OpnsenseTerminalManager {
    manager: OpnsenseManager,
    db: Arc<Database>,
    activity: OpnsenseActivityRegistry,
    inner: Arc<Mutex<TerminalInner>>,
    /// Test seam: force a specific SSH port instead of the profile's.
    ssh_port_override: Option<u16>,
}

#[derive(Default)]
struct TerminalInner {
    active: HashMap<String, ActiveTerminal>,
    /// Opens that passed the cap check and are mid-handshake. Counted toward
    /// `MAX_TERMINALS` so concurrent opens can't overshoot the cap while
    /// their SSH handshakes are still running; released exactly once when
    /// the terminal is inserted or the open fails.
    starting: usize,
}

struct ActiveTerminal {
    profile_id: i64,
    /// Owned write half of the split channel (russh 0.63 `Channel::split`).
    /// Behind `Arc` so `write`/`resize` can clone it out and drop the map
    /// lock before awaiting the send.
    write_half: Arc<ChannelWriteHalf<ClientMsg>>,
    /// Kept to disconnect cleanly on `close`; dropping it would close the
    /// session too, but an explicit disconnect is tidier.
    session: russh::client::Handle<VerifyHostKey>,
    /// Reader task; exits when the server closes the channel or the
    /// connection drops.
    join: JoinHandle<()>,
}

fn connect_err(message: impl Into<String>) -> OpnsenseManagerError {
    OpnsenseManagerError::Api(OpnsenseError::Connect(message.into()))
}

/// Map a recorded host-key outcome onto its typed error, if there is one.
fn host_key_error(
    outcome: &Arc<SyncMutex<Option<HostKeyOutcome>>>,
) -> Option<OpnsenseManagerError> {
    match outcome.lock().as_ref() {
        Some(HostKeyOutcome::Unknown(fingerprint)) => Some(OpnsenseManagerError::UnknownHostKey {
            fingerprint: fingerprint.clone(),
        }),
        Some(HostKeyOutcome::Mismatch { expected, actual }) => {
            Some(OpnsenseManagerError::HostKeyMismatch {
                expected: expected.clone(),
                actual: actual.clone(),
            })
        }
        _ => None,
    }
}

fn accepted_fingerprint(outcome: &Arc<SyncMutex<Option<HostKeyOutcome>>>) -> Option<String> {
    match outcome.lock().as_ref() {
        Some(HostKeyOutcome::Accepted(fingerprint)) => Some(fingerprint.clone()),
        _ => None,
    }
}

/// Random 128-bit hex id; terminal ids are never reused, so a reader-exit
/// cleanup can never remove a slot opened after an earlier close.
fn new_terminal_id() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut id = String::with_capacity(32);
    for byte in bytes {
        id.push_str(&format!("{byte:02x}"));
    }
    id
}

impl OpnsenseTerminalManager {
    pub fn new(manager: OpnsenseManager, db: Database) -> Self {
        Self {
            activity: manager.activity(),
            manager,
            db: Arc::new(db),
            inner: Arc::new(Mutex::new(TerminalInner::default())),
            ssh_port_override: None,
        }
    }

    /// Test seam: the echo-shell fixture listens on an ephemeral port.
    #[cfg(test)]
    fn with_ssh_port(manager: OpnsenseManager, db: Database, ssh_port: u16) -> Self {
        Self {
            ssh_port_override: Some(ssh_port),
            ..Self::new(manager, db)
        }
    }

    /// Arity is fixed by the IPC command contract (`opnsense_terminal_open`
    /// passes these straight through).
    #[allow(clippy::too_many_arguments)]
    pub async fn open(
        &self,
        profile_id: i64,
        username: &str,
        password: &str,
        cols: u32,
        rows: u32,
        confirmed_fingerprint: Option<String>,
        on_data: OpnsenseTerminalDataSink,
    ) -> Result<OpnsenseTerminalOpenDto, OpnsenseManagerError> {
        {
            let mut inner = self.inner.lock().await;
            if inner.active.len() + inner.starting >= MAX_TERMINALS {
                return Err(OpnsenseManagerError::TooManyTerminals);
            }
            inner.starting += 1;
        }
        let result = self
            .connect_terminal(
                profile_id,
                username,
                password,
                cols,
                rows,
                confirmed_fingerprint,
                on_data,
            )
            .await;
        // Release the reservation on every path; connect_terminal inserts
        // the terminal into `active` before returning Ok, keeping the
        // invariant active + starting <= MAX at all times.
        self.inner.lock().await.starting -= 1;
        result
    }

    /// Handshake + host-key verification + password auth + PTY/shell + slot
    /// insertion. Runs outside the lifecycle lock so a slow connect never
    /// blocks close/write of unrelated terminals. Arity mirrors `open`.
    #[allow(clippy::too_many_arguments)]
    async fn connect_terminal(
        &self,
        profile_id: i64,
        username: &str,
        password: &str,
        cols: u32,
        rows: u32,
        confirmed_fingerprint: Option<String>,
        on_data: OpnsenseTerminalDataSink,
    ) -> Result<OpnsenseTerminalOpenDto, OpnsenseManagerError> {
        let profile = self.manager.require_profile(profile_id).await?;
        let ssh_port = match self.ssh_port_override {
            Some(port) => port,
            None => u16::try_from(profile.ssh_port)
                .map_err(|_| OpnsenseManagerError::InvalidSshPort(profile.ssh_port))?,
        };

        let outcome = Arc::new(SyncMutex::new(None));
        let handler = VerifyHostKey {
            outcome: Arc::clone(&outcome),
            pinned: profile.ssh_host_key.clone(),
            confirmed: confirmed_fingerprint,
        };
        let config = russh::client::Config::default();
        let mut session = match russh::client::connect(
            Arc::new(config),
            (profile.host.as_str(), ssh_port),
            handler,
        )
        .await
        {
            Ok(session) => session,
            // A rejected host key surfaces here as a generic UnknownKey;
            // translate it into the exact typed error. Any other failure is
            // a plain connect error. Either way no authentication ran.
            Err(err) => {
                return Err(host_key_error(&outcome).unwrap_or_else(|| {
                    connect_err(format!("ssh connect to {}: {err}", profile.host))
                }));
            }
        };

        // Password authentication happens ONLY after the host key was
        // accepted — the API key/secret are never used here, and a
        // rejected/unconfirmed key never lets a password leave the app.
        let auth = session
            .authenticate_password(username, password)
            .await
            .map_err(|err| connect_err(format!("ssh auth: {err}")))?;
        if !auth.success() {
            return Err(OpnsenseManagerError::SshAuthFailed);
        }

        // First successful login with a user-confirmed key: pin it. A
        // changed host/SSH port already cleared the pin in profile editing,
        // so reaching here with no pin means a verified first connect.
        if profile.ssh_host_key.is_none() {
            if let Some(fingerprint) = accepted_fingerprint(&outcome) {
                self.db
                    .set_opnsense_ssh_host_key(profile.id, Some(&fingerprint))
                    .await?;
            }
        }

        let channel = session
            .channel_open_session()
            .await
            .map_err(|err| connect_err(format!("ssh channel: {err}")))?;
        channel
            .request_pty(true, TERM, cols, rows, 0, 0, &[])
            .await
            .map_err(|err| connect_err(format!("ssh pty: {err}")))?;
        channel
            .request_shell(true)
            .await
            .map_err(|err| connect_err(format!("ssh shell: {err}")))?;

        let terminal_id = new_terminal_id();
        let reader_id = terminal_id.clone();
        // Split into owned halves: the reader task keeps the read half, the
        // manager keeps the write half — no shared-channel locking.
        let (mut read_half, write_half) = channel.split();
        // The slot must exist BEFORE the reader can run: a server that
        // closes the channel instantly would otherwise let the reader's
        // cleanup no-op and then leak a dead terminal into the map.
        let (gate_tx, gate_rx) = tokio::sync::oneshot::channel::<()>();
        let manager = self.clone();
        let join = tokio::spawn(async move {
            // Opened after insertion; a close() that wins the race simply
            // never opens the gate (the receiver is dropped instead).
            let _ = gate_rx.await;
            loop {
                let chunk = match read_half.wait().await {
                    Some(ChannelMsg::Data { data }) => Some(BASE64.encode(&data[..])),
                    Some(ChannelMsg::ExtendedData { data, ext: 1 }) => {
                        Some(BASE64.encode(&data[..]))
                    }
                    Some(ChannelMsg::Close) | None => None,
                    _ => continue,
                };
                match chunk {
                    Some(encoded) => on_data(encoded),
                    None => break,
                }
            }
            // Server ended the session: free the slot (and the activity
            // mark) so the cap doesn't leak. Dropping the entry closes the
            // connection.
            manager.remove(&reader_id).await;
        });
        self.inner.lock().await.active.insert(
            terminal_id.clone(),
            ActiveTerminal {
                profile_id,
                write_half: Arc::new(write_half),
                session,
                join,
            },
        );
        self.activity.lock().await.terminal_enter(profile_id);
        let _ = gate_tx.send(());
        Ok(OpnsenseTerminalOpenDto { terminal_id })
    }

    pub async fn write(
        &self,
        terminal_id: &str,
        data_b64: &str,
    ) -> Result<(), OpnsenseManagerError> {
        let bytes = BASE64
            .decode(data_b64)
            .map_err(|err| connect_err(format!("terminal write: bad base64: {err}")))?;
        if bytes.len() > MAX_WRITE_BYTES {
            return Err(connect_err(format!(
                "terminal write: {} bytes exceeds the {MAX_WRITE_BYTES} byte limit",
                bytes.len()
            )));
        }
        let write_half = self.write_half_for(terminal_id).await?;
        let result = write_half
            .data(&bytes[..])
            .await
            .map_err(|err| connect_err(format!("terminal write: {err}")));
        if result.is_err() {
            // The session is dead: drop the slot so later calls get a typed
            // TerminalNotFound instead of a zombie that errors forever.
            self.remove(terminal_id).await;
        }
        result
    }

    pub async fn resize(
        &self,
        terminal_id: &str,
        cols: u32,
        rows: u32,
    ) -> Result<(), OpnsenseManagerError> {
        let write_half = self.write_half_for(terminal_id).await?;
        let result = write_half
            .window_change(cols, rows, 0, 0)
            .await
            .map_err(|err| connect_err(format!("terminal resize: {err}")));
        if result.is_err() {
            self.remove(terminal_id).await;
        }
        result
    }

    pub async fn close(&self, terminal_id: &str) -> Result<(), OpnsenseManagerError> {
        let entry = self.inner.lock().await.active.remove(terminal_id);
        let Some(terminal) = entry else {
            return Err(OpnsenseManagerError::TerminalNotFound(
                terminal_id.to_owned(),
            ));
        };
        self.activity
            .lock()
            .await
            .terminal_exit(terminal.profile_id);
        let _ = terminal
            .session
            .disconnect(
                russh::Disconnect::ByApplication,
                "terminal closed",
                "verkkokyyla",
            )
            .await;
        // The reader is usually already exiting on the closed channel, but
        // abort covers the case where the disconnect hasn't landed yet.
        terminal.join.abort();
        Ok(())
    }

    /// Clears the pinned SSH host key so the next connect requires explicit
    /// user verification again (intentional rekey, host rebuilt, ...).
    pub async fn forget_ssh_host_key(&self, profile_id: i64) -> Result<(), OpnsenseManagerError> {
        self.manager.require_profile(profile_id).await?;
        self.db.set_opnsense_ssh_host_key(profile_id, None).await?;
        Ok(())
    }

    /// Reader-exit / dead-session cleanup: drop the slot if it is still
    /// ours (a `close` may have beaten us to it) and release the activity
    /// mark exactly once per removed terminal.
    async fn remove(&self, terminal_id: &str) {
        let removed = self.inner.lock().await.active.remove(terminal_id);
        if let Some(terminal) = removed {
            self.activity
                .lock()
                .await
                .terminal_exit(terminal.profile_id);
        }
    }

    async fn write_half_for(
        &self,
        terminal_id: &str,
    ) -> Result<Arc<ChannelWriteHalf<ClientMsg>>, OpnsenseManagerError> {
        let inner = self.inner.lock().await;
        let terminal = inner
            .active
            .get(terminal_id)
            .ok_or_else(|| OpnsenseManagerError::TerminalNotFound(terminal_id.to_owned()))?;
        Ok(terminal.write_half.clone())
    }
}

// ---------------------------------------------------------------------------
// Tauri commands (central registration in lib.rs)
// ---------------------------------------------------------------------------

/// `opnsense_terminal_open(...)` — the username/password are the SSH
/// user's interactive credentials, entered each time; the API key/secret
/// never authenticates SSH and the password is never stored. Arity is
/// fixed by the IPC contract.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn opnsense_terminal_open(
    manager: tauri::State<'_, OpnsenseTerminalManager>,
    profile_id: i64,
    username: String,
    password: String,
    cols: u32,
    rows: u32,
    confirmed_fingerprint: Option<String>,
    on_data: tauri::ipc::Channel<String>,
) -> Result<OpnsenseTerminalOpenDto, OpnsenseManagerError> {
    let sink: OpnsenseTerminalDataSink = Arc::new(move |chunk| {
        let _ = on_data.send(chunk);
    });
    manager
        .open(
            profile_id,
            &username,
            &password,
            cols,
            rows,
            confirmed_fingerprint,
            sink,
        )
        .await
}

#[tauri::command]
pub async fn opnsense_terminal_write(
    manager: tauri::State<'_, OpnsenseTerminalManager>,
    terminal_id: String,
    data_b64: String,
) -> Result<(), OpnsenseManagerError> {
    manager.write(&terminal_id, &data_b64).await
}

#[tauri::command]
pub async fn opnsense_terminal_resize(
    manager: tauri::State<'_, OpnsenseTerminalManager>,
    terminal_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), OpnsenseManagerError> {
    manager.resize(&terminal_id, cols, rows).await
}

#[tauri::command]
pub async fn opnsense_terminal_close(
    manager: tauri::State<'_, OpnsenseTerminalManager>,
    terminal_id: String,
) -> Result<(), OpnsenseManagerError> {
    manager.close(&terminal_id).await
}

/// Clears the pinned SSH host key for a profile; the next terminal connect
/// asks for explicit fingerprint verification again.
#[tauri::command]
pub async fn opnsense_forget_ssh_host_key(
    manager: tauri::State<'_, OpnsenseTerminalManager>,
    profile_id: i64,
) -> Result<(), OpnsenseManagerError> {
    manager.forget_ssh_host_key(profile_id).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{now_rfc3339, NewOpnsenseProfile};
    use crate::opnsense::types::OpnsenseApiFactory;
    use crate::secrets::{MemoryStore, SecretStore};
    use russh::server::{Auth, Server as _};
    use russh::Channel;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Same shape as mikrotik/terminal.rs's `EchoError` — russh 0.63 has no
    /// public `server::ServerError`, so the fixture carries its own type.
    #[derive(Debug)]
    enum EchoError {
        Russh(russh::Error),
    }

    impl std::fmt::Display for EchoError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                EchoError::Russh(err) => write!(f, "{err}"),
            }
        }
    }

    impl std::error::Error for EchoError {}

    impl From<russh::Error> for EchoError {
        fn from(err: russh::Error) -> Self {
            EchoError::Russh(err)
        }
    }

    static DIR_SEQ: AtomicUsize = AtomicUsize::new(0);

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vk-opnsense-terminal-{}-{}-{}",
            tag,
            std::process::id(),
            DIR_SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Echo-shell fixture: accepts one expected password (any password when
    /// none is set), answers PTY + shell requests, echoes every input byte
    /// back as output, and counts auth attempts and opened shell channels
    /// so tests can prove host-key rejection happens before authentication
    /// and that a failed login never opens a shell.
    #[derive(Clone)]
    struct EchoServer {
        outputs: Arc<SyncMutex<Vec<Vec<u8>>>>,
        auth_attempts: Arc<AtomicUsize>,
        channels_opened: Arc<AtomicUsize>,
        expected_password: Option<String>,
    }

    impl russh::server::Server for EchoServer {
        type Handler = EchoSession;

        fn new_client(&mut self, _peer: Option<std::net::SocketAddr>) -> Self::Handler {
            EchoSession {
                outputs: self.outputs.clone(),
                auth_attempts: self.auth_attempts.clone(),
                channels_opened: self.channels_opened.clone(),
                expected_password: self.expected_password.clone(),
            }
        }
    }

    struct EchoSession {
        outputs: Arc<SyncMutex<Vec<Vec<u8>>>>,
        auth_attempts: Arc<AtomicUsize>,
        channels_opened: Arc<AtomicUsize>,
        expected_password: Option<String>,
    }

    impl russh::server::Handler for EchoSession {
        type Error = EchoError;

        async fn auth_password(
            &mut self,
            _user: &str,
            password: &str,
        ) -> Result<Auth, Self::Error> {
            self.auth_attempts.fetch_add(1, Ordering::SeqCst);
            match &self.expected_password {
                Some(expected) if expected != password => Ok(Auth::Reject {
                    proceed_with_methods: None,
                    partial_success: false,
                }),
                _ => Ok(Auth::Accept),
            }
        }

        async fn channel_open_session(
            &mut self,
            channel: Channel<russh::server::Msg>,
            reply: russh::server::ChannelOpenHandle,
            _session: &mut russh::server::Session,
        ) -> Result<(), Self::Error> {
            self.channels_opened.fetch_add(1, Ordering::SeqCst);
            let mut channel = channel;
            reply.accept().await;
            let outputs = self.outputs.clone();
            tokio::spawn(async move {
                while let Some(msg) = channel.wait().await {
                    match msg {
                        ChannelMsg::Data { data } => {
                            outputs.lock().push(data.to_vec());
                            let _ = channel.data(&data[..]).await;
                        }
                        ChannelMsg::Close | ChannelMsg::Eof => break,
                        _ => {}
                    }
                }
            });
            Ok(())
        }

        /// PTY and shell requests are answered with explicit success — the
        /// client asks with `want_reply = true` and would hang otherwise.
        async fn pty_request(
            &mut self,
            channel: russh::ChannelId,
            _term: &str,
            _col_width: u32,
            _row_height: u32,
            _pix_width: u32,
            _pix_height: u32,
            _modes: &[(russh::Pty, u32)],
            session: &mut russh::server::Session,
        ) -> Result<(), Self::Error> {
            session.channel_success(channel)?;
            Ok(())
        }

        async fn shell_request(
            &mut self,
            channel: russh::ChannelId,
            session: &mut russh::server::Session,
        ) -> Result<(), Self::Error> {
            session.channel_success(channel)?;
            Ok(())
        }
    }

    struct FixtureCounts {
        auth_attempts: Arc<AtomicUsize>,
        channels_opened: Arc<AtomicUsize>,
    }

    async fn manager_with_profile(dir: &std::path::Path) -> (OpnsenseManager, i64) {
        let db = Database::connect(&dir.join("term.db")).await.expect("db");
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
        let profile = db
            .create_opnsense_profile(&NewOpnsenseProfile {
                name: "lab".to_owned(),
                host: "127.0.0.1".to_owned(),
                port: 443,
                allow_invalid_certs: false,
                ssh_port: 22,
                created_at: now_rfc3339(),
            })
            .await
            .expect("profile");
        // Terminals never touch the HTTP API; any factory works.
        let factory: OpnsenseApiFactory = Arc::new(move |_conn| {
            Box::pin(async move {
                Err(OpnsenseManagerError::Api(OpnsenseError::Connect(
                    "no http api in terminal tests".to_owned(),
                )))
            })
        });
        (OpnsenseManager::new(db, store, factory), profile.id)
    }

    /// The terminal manager owns a separate connection to the SAME database
    /// file (`Database` is not `Clone`; lib.rs gives every manager its own
    /// `Database::connect`, so tests mirror that).
    async fn terminal_db(dir: &std::path::Path) -> Database {
        Database::connect(&dir.join("term.db")).await.expect("db")
    }

    /// Unencrypted ed25519 host key (RFC 8410 §10.3 test vector, same one
    /// russh's own key tests and the MikroTik terminal fixture use) — a
    /// fixture identity, never a real secret.
    const FIXTURE_HOST_KEY: &str = "-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEINTuctv5E1hK1bbY8fdp+K06/nwoy/HU++CXqI9EdVhC
-----END PRIVATE KEY-----";

    /// A second, unrelated ed25519 identity so a "changed server key" test
    /// can run a genuinely different host. Fixture-only, never a secret.
    const FIXTURE_HOST_KEY_2: &str = "-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACCCl5nylo8ZMb2AfV6VtGBxuskhO6weIjbfvrekuJQLPwAAAJA2+E5SNvhO
UgAAAAtzc2gtZWQyNTUxOQAAACCCl5nylo8ZMb2AfV6VtGBxuskhO6weIjbfvrekuJQLPw
AAAEDOHKRRvHBNXFYg3CQBoTSPkh62IhIjNkC6SSRgZL2l+IKXmfKWjxkxvYB9XpW0YHG6
ySE7rB4iNt++t6S4lAs/AAAAC3Rlcm9rQGp1aHRhAQI=
-----END OPENSSH PRIVATE KEY-----";

    async fn echo_server(key_pem: &str, expected_password: Option<String>) -> (u16, FixtureCounts) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let config = russh::server::Config {
            keys: vec![russh::keys::decode_secret_key(key_pem, None).unwrap()],
            auth_rejection_time: std::time::Duration::from_millis(100),
            auth_rejection_time_initial: Some(std::time::Duration::from_millis(0)),
            ..Default::default()
        };
        let counts = FixtureCounts {
            auth_attempts: Arc::new(AtomicUsize::new(0)),
            channels_opened: Arc::new(AtomicUsize::new(0)),
        };
        let server = EchoServer {
            outputs: Arc::new(SyncMutex::new(Vec::new())),
            auth_attempts: counts.auth_attempts.clone(),
            channels_opened: counts.channels_opened.clone(),
            expected_password,
        };
        tokio::spawn(async move {
            let mut server = server;
            server
                .run_on_socket(Arc::new(config), &listener)
                .await
                .map_err(|err| err.to_string())
        });
        (port, counts)
    }

    /// Poll until `cond` holds or the deadline passes.
    async fn wait_for(mut cond: impl FnMut() -> bool) {
        for _ in 0..100 {
            if cond() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("condition was not met within the deadline");
    }

    fn collecting_sink(received: Arc<SyncMutex<Vec<String>>>) -> OpnsenseTerminalDataSink {
        Arc::new(move |chunk| received.lock().push(chunk))
    }

    /// Learn the fixture's fingerprint via a first (rejected) connect, then
    /// confirm it — the same flow the UI drives.
    async fn learn_fingerprint(terminals: &OpnsenseTerminalManager, profile_id: i64) -> String {
        let err = terminals
            .open(
                profile_id,
                "admin",
                "x",
                120,
                40,
                None,
                collecting_sink(Default::default()),
            )
            .await
            .unwrap_err();
        match err {
            OpnsenseManagerError::UnknownHostKey { fingerprint } => fingerprint,
            other => panic!("expected UnknownHostKey, got {other}"),
        }
    }

    #[tokio::test]
    async fn opnsense_terminal_unknown_host_key_rejects_before_auth() {
        let dir = temp_dir("unknown");
        let (manager, profile_id) = manager_with_profile(&dir).await;
        let db = terminal_db(&dir).await;
        let (port, counts) = echo_server(FIXTURE_HOST_KEY, None).await;
        let terminals = OpnsenseTerminalManager::with_ssh_port(manager, db, port);

        let err = terminals
            .open(
                profile_id,
                "admin",
                "s3cr3t",
                120,
                40,
                None,
                collecting_sink(Default::default()),
            )
            .await
            .unwrap_err();
        let OpnsenseManagerError::UnknownHostKey { fingerprint } = err else {
            panic!("unexpected error: {err}");
        };
        assert!(
            fingerprint.starts_with("SHA256:"),
            "fingerprint must be OpenSSH SHA256 form: {fingerprint}"
        );
        assert_eq!(
            counts.auth_attempts.load(Ordering::SeqCst),
            0,
            "password auth must never fire when the host key is rejected"
        );
        assert_eq!(counts.channels_opened.load(Ordering::SeqCst), 0);

        // Nothing was pinned: the profile stays unverified.
        let profile = terminals.manager.require_profile(profile_id).await.unwrap();
        assert_eq!(profile.ssh_host_key, None);
        drop(dir);
    }

    #[tokio::test]
    async fn opnsense_terminal_wrong_confirmed_fingerprint_rejects_before_auth() {
        let dir = temp_dir("wrong-confirm");
        let (manager, profile_id) = manager_with_profile(&dir).await;
        let db = terminal_db(&dir).await;
        let (port, counts) = echo_server(FIXTURE_HOST_KEY, None).await;
        let terminals = OpnsenseTerminalManager::with_ssh_port(manager, db, port);

        let err = terminals
            .open(
                profile_id,
                "admin",
                "s3cr3t",
                120,
                40,
                Some("SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned()),
                collecting_sink(Default::default()),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, OpnsenseManagerError::UnknownHostKey { .. }),
            "unexpected error: {err}"
        );
        assert_eq!(
            counts.auth_attempts.load(Ordering::SeqCst),
            0,
            "a mismatched confirmation must not reach password auth"
        );
        drop(dir);
    }

    #[tokio::test]
    async fn opnsense_terminal_confirmed_key_pins_and_echoes() {
        let dir = temp_dir("confirm");
        let (manager, profile_id) = manager_with_profile(&dir).await;
        let db = terminal_db(&dir).await;
        let (port, counts) = echo_server(FIXTURE_HOST_KEY, None).await;
        let terminals = OpnsenseTerminalManager::with_ssh_port(manager, db, port);

        let fingerprint = learn_fingerprint(&terminals, profile_id).await;
        let received = Arc::new(SyncMutex::new(Vec::new()));
        let opened = terminals
            .open(
                profile_id,
                "admin",
                "s3cr3t",
                120,
                40,
                Some(fingerprint.clone()),
                collecting_sink(received.clone()),
            )
            .await
            .expect("confirmed fingerprint opens a shell");
        assert!(!opened.terminal_id.is_empty());

        // First successful login pins the fingerprint in the database.
        let profile = terminals.manager.require_profile(profile_id).await.unwrap();
        assert_eq!(profile.ssh_host_key.as_deref(), Some(fingerprint.as_str()));

        // The pinned profile counts as active while the terminal is open.
        assert!(terminals
            .manager
            .activity()
            .lock()
            .await
            .is_active(profile_id));

        let payload = "pfctl -s info\n";
        terminals
            .write(&opened.terminal_id, &BASE64.encode(payload.as_bytes()))
            .await
            .expect("write succeeds");
        wait_for(|| !received.lock().is_empty()).await;
        let echoed: String = received
            .lock()
            .iter()
            .map(|chunk| String::from_utf8(BASE64.decode(chunk).unwrap()).unwrap())
            .collect();
        assert_eq!(echoed, payload, "echo fixture must return the input bytes");

        // Resize is a no-op against the fixture but must not error.
        terminals
            .resize(&opened.terminal_id, 100, 30)
            .await
            .expect("resize succeeds");

        terminals
            .close(&opened.terminal_id)
            .await
            .expect("close succeeds");
        assert!(
            !terminals
                .manager
                .activity()
                .lock()
                .await
                .is_active(profile_id),
            "close releases the activity mark"
        );
        assert_eq!(counts.channels_opened.load(Ordering::SeqCst), 1);
        drop(dir);
    }

    #[tokio::test]
    async fn opnsense_terminal_pinned_key_reconnects_without_confirmation() {
        let dir = temp_dir("pinned");
        let (manager, profile_id) = manager_with_profile(&dir).await;
        let db = terminal_db(&dir).await;
        let (port, _counts) = echo_server(FIXTURE_HOST_KEY, None).await;
        let terminals = OpnsenseTerminalManager::with_ssh_port(manager, db, port);

        let fingerprint = learn_fingerprint(&terminals, profile_id).await;
        let first = terminals
            .open(
                profile_id,
                "admin",
                "s3cr3t",
                120,
                40,
                Some(fingerprint),
                collecting_sink(Default::default()),
            )
            .await
            .expect("first connect pins the key");
        terminals.close(&first.terminal_id).await.expect("close");

        // Second connect: the pin is trusted, no confirmation needed.
        let second = terminals
            .open(
                profile_id,
                "admin",
                "s3cr3t",
                120,
                40,
                None,
                collecting_sink(Default::default()),
            )
            .await
            .expect("pinned key reconnects without confirmation");
        terminals.close(&second.terminal_id).await.expect("close");
        drop(dir);
    }

    #[tokio::test]
    async fn opnsense_terminal_changed_server_key_is_mismatch() {
        let dir = temp_dir("changed");
        let (manager, profile_id) = manager_with_profile(&dir).await;
        let db = terminal_db(&dir).await;
        let (port, _counts) = echo_server(FIXTURE_HOST_KEY, None).await;
        let terminals = OpnsenseTerminalManager::with_ssh_port(manager, db, port);

        let fingerprint = learn_fingerprint(&terminals, profile_id).await;
        let first = terminals
            .open(
                profile_id,
                "admin",
                "s3cr3t",
                120,
                40,
                Some(fingerprint.clone()),
                collecting_sink(Default::default()),
            )
            .await
            .expect("first connect pins the key");
        terminals.close(&first.terminal_id).await.expect("close");

        // The host now presents a DIFFERENT key: hard rejection, no bypass.
        let (port2, counts2) = echo_server(FIXTURE_HOST_KEY_2, None).await;
        let terminals = OpnsenseTerminalManager::with_ssh_port(
            terminals.manager.clone(),
            terminal_db(&dir).await,
            port2,
        );

        let err = terminals
            .open(
                profile_id,
                "admin",
                "s3cr3t",
                120,
                40,
                None,
                collecting_sink(Default::default()),
            )
            .await
            .unwrap_err();
        match err {
            OpnsenseManagerError::HostKeyMismatch { expected, actual } => {
                assert_eq!(expected, fingerprint);
                assert!(
                    actual.starts_with("SHA256:") && actual != expected,
                    "actual must be the NEW key's fingerprint: {actual}"
                );
            }
            other => panic!("expected HostKeyMismatch, got {other}"),
        }
        assert_eq!(
            counts2.auth_attempts.load(Ordering::SeqCst),
            0,
            "a changed key must not reach password auth"
        );
        drop(dir);
    }

    #[tokio::test]
    async fn opnsense_terminal_bad_password_is_auth_failed_with_no_shell() {
        let dir = temp_dir("bad-password");
        let (manager, profile_id) = manager_with_profile(&dir).await;
        let db = terminal_db(&dir).await;
        let (port, counts) = echo_server(FIXTURE_HOST_KEY, Some("good".to_owned())).await;
        let terminals = OpnsenseTerminalManager::with_ssh_port(manager, db, port);

        // Confirm the host key first so auth rejection is the only failure.
        let fingerprint = learn_fingerprint(&terminals, profile_id).await;
        let err = terminals
            .open(
                profile_id,
                "admin",
                "bad",
                120,
                40,
                Some(fingerprint),
                collecting_sink(Default::default()),
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, OpnsenseManagerError::SshAuthFailed),
            "unexpected error: {err}"
        );
        assert_eq!(
            counts.channels_opened.load(Ordering::SeqCst),
            0,
            "a rejected login must never open a shell channel"
        );
        // The wrong password must not pin the host key either.
        let profile = terminals.manager.require_profile(profile_id).await.unwrap();
        assert_eq!(profile.ssh_host_key, None);
        drop(dir);
    }

    #[tokio::test]
    async fn opnsense_terminal_close_releases_the_slot() {
        let dir = temp_dir("slot");
        let (manager, profile_id) = manager_with_profile(&dir).await;
        let db = terminal_db(&dir).await;
        let (port, _counts) = echo_server(FIXTURE_HOST_KEY, None).await;
        let terminals = OpnsenseTerminalManager::with_ssh_port(manager, db, port);

        let fingerprint = learn_fingerprint(&terminals, profile_id).await;
        let first = terminals
            .open(
                profile_id,
                "admin",
                "s3cr3t",
                120,
                40,
                Some(fingerprint),
                collecting_sink(Default::default()),
            )
            .await
            .expect("first terminal opens");
        terminals
            .close(&first.terminal_id)
            .await
            .expect("close succeeds");

        // The slot is free again: opening another terminal works.
        let second = terminals
            .open(
                profile_id,
                "admin",
                "s3cr3t",
                120,
                40,
                None,
                collecting_sink(Default::default()),
            )
            .await
            .expect("slot was released");
        terminals
            .close(&second.terminal_id)
            .await
            .expect("close succeeds");

        // Double close and unknown ids are typed errors, not panics.
        let err = terminals.close(&first.terminal_id).await.unwrap_err();
        assert!(
            matches!(err, OpnsenseManagerError::TerminalNotFound(_)),
            "unexpected error: {err}"
        );
        let err = terminals
            .write("deadbeef", &BASE64.encode("x"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, OpnsenseManagerError::TerminalNotFound(_)),
            "unexpected error: {err}"
        );
        let err = terminals.resize("deadbeef", 100, 30).await.unwrap_err();
        assert!(
            matches!(err, OpnsenseManagerError::TerminalNotFound(_)),
            "unexpected error: {err}"
        );
        drop(dir);
    }

    #[tokio::test]
    async fn opnsense_terminal_forget_host_key_requires_reverification() {
        let dir = temp_dir("forget");
        let (manager, profile_id) = manager_with_profile(&dir).await;
        let db = terminal_db(&dir).await;
        let (port, _counts) = echo_server(FIXTURE_HOST_KEY, None).await;
        let terminals = OpnsenseTerminalManager::with_ssh_port(manager, db, port);

        let fingerprint = learn_fingerprint(&terminals, profile_id).await;
        let first = terminals
            .open(
                profile_id,
                "admin",
                "s3cr3t",
                120,
                40,
                Some(fingerprint.clone()),
                collecting_sink(Default::default()),
            )
            .await
            .expect("first connect pins the key");
        terminals.close(&first.terminal_id).await.expect("close");

        terminals
            .forget_ssh_host_key(profile_id)
            .await
            .expect("forget succeeds");
        let profile = terminals.manager.require_profile(profile_id).await.unwrap();
        assert_eq!(profile.ssh_host_key, None, "pin is cleared");

        // The next connect demands explicit verification again.
        let err = terminals
            .open(
                profile_id,
                "admin",
                "s3cr3t",
                120,
                40,
                None,
                collecting_sink(Default::default()),
            )
            .await
            .unwrap_err();
        match err {
            OpnsenseManagerError::UnknownHostKey { fingerprint: fp } => {
                assert_eq!(fp, fingerprint, "same key, but verification was forgotten");
            }
            other => panic!("expected UnknownHostKey, got {other}"),
        }
        drop(dir);
    }
}
