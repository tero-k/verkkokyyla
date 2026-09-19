//! Interactive SSH terminals to RouterOS devices: one russh shell channel
//! with a PTY per terminal, keyed by an app-side terminal id. Terminals are
//! independent of monitoring — a terminal can be open to a device that is
//! not being monitored, and multiple devices can have terminals at once.
//!
//! Output reaches the UI as base64 chunks over a Tauri Channel (the IPC
//! layer is JSON; SSH output is arbitrary bytes). Input travels the same
//! way in reverse. Lifecycle: `open` → `write`/`resize` → `close`; a server
//! that drops the channel cleans the slot up on its own (reader exit).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use russh::client::Msg as ClientMsg;
use russh::{ChannelMsg, ChannelWriteHalf};
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use super::backup::AcceptUnknownHostKey;
use super::manager::{MikrotikManager, MAX_CONCURRENT_SESSIONS};
use super::types::MikrotikManagerError;
use crate::mikrotik::error::MikrotikError;

/// SSH port for interactive shells — the backup flow locks SFTP to port 22
/// (`mikrotik_backup_sftp_port_lock_always_22`); terminals follow suit.
const SSH_PORT: u16 = 22;
const TERM: &str = "xterm-256color";
/// One keystroke batch from xterm is tiny; this only bounds a hostile or
/// broken caller from pushing giant frames through decode + SSH.
const MAX_WRITE_BYTES: usize = 64 * 1024;

/// Base64-encoded chunk of terminal output (or input, on the way down).
/// The sink returns `false` when the receiving channel is gone (webview
/// reload, a close that never landed) so the reader can end the session
/// instead of draining the router into a void.
pub type TerminalDataSink = Arc<dyn Fn(String) -> bool + Send + Sync>;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOpenDto {
    pub terminal_id: u64,
    pub profile_id: i64,
}

#[derive(Clone)]
pub struct TerminalManager {
    manager: MikrotikManager,
    inner: Arc<Mutex<TerminalInner>>,
    next_id: Arc<AtomicU64>,
    /// SSH port; `SSH_PORT` in production, an ephemeral port in tests.
    ssh_port: u16,
}

#[derive(Default)]
struct TerminalInner {
    active: HashMap<u64, ActiveTerminal>,
    /// Opens that passed the cap check and are mid-handshake. Counted toward
    /// `MAX_CONCURRENT_SESSIONS` so concurrent opens can't overshoot the cap
    /// while their SSH handshakes are still running; released exactly once
    /// when the terminal is inserted or the open fails.
    starting: usize,
}

struct ActiveTerminal {
    /// Owned write half of the split channel (russh 0.63 `Channel::split`).
    /// Behind `Arc` so `write`/`resize` can clone it out and drop the map
    /// lock before awaiting the send.
    write_half: Arc<ChannelWriteHalf<ClientMsg>>,
    /// Kept to disconnect cleanly on `close`; dropping it would close the
    /// session too, but an explicit disconnect is tidier.
    session: russh::client::Handle<AcceptUnknownHostKey>,
    /// Reader task; exits when the server closes the channel or the
    /// connection drops.
    join: JoinHandle<()>,
}

fn connect_err(message: impl Into<String>) -> MikrotikManagerError {
    MikrotikManagerError::Api(MikrotikError::Connect(message.into()))
}

impl TerminalManager {
    pub fn new(manager: MikrotikManager) -> Self {
        Self::with_ssh_port(manager, SSH_PORT)
    }

    /// Test seam: the echo-shell fixture listens on an ephemeral port.
    fn with_ssh_port(manager: MikrotikManager, ssh_port: u16) -> Self {
        Self {
            manager,
            inner: Arc::new(Mutex::new(TerminalInner::default())),
            next_id: Arc::new(AtomicU64::new(1)),
            ssh_port,
        }
    }

    pub async fn open(
        &self,
        profile_id: i64,
        cols: u32,
        rows: u32,
        on_data: TerminalDataSink,
    ) -> Result<TerminalOpenDto, MikrotikManagerError> {
        {
            let mut inner = self.inner.lock().await;
            if inner.active.len() + inner.starting >= MAX_CONCURRENT_SESSIONS {
                return Err(MikrotikManagerError::TooManySessions);
            }
            inner.starting += 1;
        }
        let result = self.connect_terminal(profile_id, cols, rows, on_data).await;
        // Release the reservation on every path; connect_terminal inserts the
        // terminal into `active` before returning Ok, keeping the invariant
        // active + starting <= MAX at all times.
        self.inner.lock().await.starting -= 1;
        result
    }

    /// Handshake + slot insertion. Runs outside the lifecycle lock so a slow
    /// connect never blocks close/write of unrelated terminals.
    async fn connect_terminal(
        &self,
        profile_id: i64,
        cols: u32,
        rows: u32,
        on_data: TerminalDataSink,
    ) -> Result<TerminalOpenDto, MikrotikManagerError> {
        let profile = self.manager.require_profile(profile_id).await?;
        let conn = self.manager.connection_for(&profile).await?;

        let config = russh::client::Config::default();
        let mut session = russh::client::connect(
            Arc::new(config),
            (conn.host.as_str(), self.ssh_port),
            AcceptUnknownHostKey,
        )
        .await
        .map_err(|err| connect_err(format!("ssh connect to {}: {err}", conn.host)))?;
        let auth = session
            .authenticate_password(&conn.username, &conn.password)
            .await
            .map_err(|err| connect_err(format!("ssh auth: {err}")))?;
        if !auth.success() {
            return Err(connect_err(format!(
                "password rejected for user {:?}",
                conn.username
            )));
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

        let terminal_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        // Split into owned halves: the reader task keeps the read half, the
        // manager keeps the write half — no shared-channel locking.
        let (mut read_half, write_half) = channel.split();
        // The slot must exist BEFORE the reader can run: a server that closes
        // the channel instantly would otherwise let the reader's cleanup
        // no-op and then leak a dead terminal into the map.
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
                    Some(encoded) => {
                        if !on_data(encoded) {
                            // Frontend channel is dead: end the session rather
                            // than draining the router into a void.
                            break;
                        }
                    }
                    None => break,
                }
            }
            // Server ended the session (or the sink died): free the slot so
            // the cap doesn't leak. Dropping the entry closes the connection.
            manager.remove(terminal_id).await;
        });
        self.inner.lock().await.active.insert(
            terminal_id,
            ActiveTerminal {
                write_half: Arc::new(write_half),
                session,
                join,
            },
        );
        let _ = gate_tx.send(());
        Ok(TerminalOpenDto {
            terminal_id,
            profile_id,
        })
    }

    pub async fn write(
        &self,
        terminal_id: u64,
        data_b64: &str,
    ) -> Result<(), MikrotikManagerError> {
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
            // NoActiveSession instead of a zombie that errors forever.
            self.remove(terminal_id).await;
        }
        result
    }

    pub async fn resize(
        &self,
        terminal_id: u64,
        cols: u32,
        rows: u32,
    ) -> Result<(), MikrotikManagerError> {
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

    pub async fn close(&self, terminal_id: u64) -> Result<(), MikrotikManagerError> {
        let entry = self.inner.lock().await.active.remove(&terminal_id);
        let Some(terminal) = entry else {
            return Err(MikrotikManagerError::NoActiveSession);
        };
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

    /// Reader-exit cleanup: drop the slot if it is still ours. (A `close`
    /// may have already removed it, or replaced it — ids are never reused.)
    async fn remove(&self, terminal_id: u64) {
        self.inner.lock().await.active.remove(&terminal_id);
    }

    async fn write_half_for(
        &self,
        terminal_id: u64,
    ) -> Result<Arc<ChannelWriteHalf<ClientMsg>>, MikrotikManagerError> {
        let inner = self.inner.lock().await;
        let terminal = inner
            .active
            .get(&terminal_id)
            .ok_or(MikrotikManagerError::NoActiveSession)?;
        Ok(terminal.write_half.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{now_rfc3339, Database, NewMikrotikProfile};
    use crate::mikrotik::secrets::{MemoryStore, SecretStore};
    use crate::mikrotik::types::MikrotikApiFactory;
    use russh::server::{Auth, Server as _};
    use russh::Channel;
    use std::sync::atomic::AtomicUsize;

    /// Same shape as backup.rs's `FixtureError` — russh 0.63 has no public
    /// `server::ServerError`, so the fixture carries its own error type.
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
            "vk-mikrotik-terminal-{}-{}-{}",
            tag,
            std::process::id(),
            DIR_SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Echo-shell fixture: accepts any password, answers PTY + shell
    /// requests, and echoes every input byte back as output.
    #[derive(Clone, Default)]
    struct EchoServer {
        outputs: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    }

    impl russh::server::Server for EchoServer {
        type Handler = EchoSession;

        fn new_client(&mut self, _peer: Option<std::net::SocketAddr>) -> Self::Handler {
            EchoSession {
                outputs: self.outputs.clone(),
            }
        }
    }

    struct EchoSession {
        outputs: Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
    }

    impl russh::server::Handler for EchoSession {
        type Error = EchoError;

        async fn auth_password(
            &mut self,
            _user: &str,
            _password: &str,
        ) -> Result<Auth, Self::Error> {
            Ok(Auth::Accept)
        }

        async fn channel_open_session(
            &mut self,
            channel: Channel<russh::server::Msg>,
            reply: russh::server::ChannelOpenHandle,
            _session: &mut russh::server::Session,
        ) -> Result<(), Self::Error> {
            let mut channel = channel;
            reply.accept().await;
            let outputs = self.outputs.clone();
            tokio::spawn(async move {
                while let Some(msg) = channel.wait().await {
                    match msg {
                        ChannelMsg::Data { data } => {
                            outputs.lock().unwrap().push(data.to_vec());
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

    async fn manager_with_profile(dir: &std::path::Path) -> (MikrotikManager, i64) {
        let db = Database::connect(&dir.join("term.db")).await.expect("db");
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
        let profile = db
            .create_mikrotik_profile(&NewMikrotikProfile {
                name: "lab".to_owned(),
                host: "127.0.0.1".to_owned(),
                port: 443,
                use_tls: true,
                allow_invalid_certs: false,
                username: "admin".to_owned(),
                created_at: now_rfc3339(),
            })
            .await
            .expect("profile");
        store
            .set(&profile.secret_key, "s3cr3t")
            .await
            .expect("password");
        // Terminals never touch the REST API; any factory works.
        let factory: MikrotikApiFactory = Arc::new(move |_conn| {
            Box::pin(async move {
                Err(MikrotikError::Connect(
                    "no rest api in terminal tests".to_owned(),
                ))
            })
        });
        (MikrotikManager::new(db, store, factory), profile.id)
    }

    /// Unencrypted ed25519 host key (RFC 8410 §10.3 test vector, same one
    /// russh's own key tests use) — a fixture identity, never a real secret.
    const FIXTURE_HOST_KEY: &str = "-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEINTuctv5E1hK1bbY8fdp+K06/nwoy/HU++CXqI9EdVhC
-----END PRIVATE KEY-----";

    async fn echo_server(outputs: Arc<std::sync::Mutex<Vec<Vec<u8>>>>) -> u16 {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let config = russh::server::Config {
            keys: vec![russh::keys::decode_secret_key(FIXTURE_HOST_KEY, None).unwrap()],
            auth_rejection_time: std::time::Duration::from_millis(100),
            auth_rejection_time_initial: Some(std::time::Duration::from_millis(0)),
            ..Default::default()
        };
        tokio::spawn(async move {
            let mut server = EchoServer { outputs };
            server
                .run_on_socket(Arc::new(config), &listener)
                .await
                .map_err(|err| err.to_string())
        });
        port
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

    #[tokio::test]
    async fn mikrotik_terminal_echo_roundtrip_and_close() {
        let dir = temp_dir("echo");
        let (manager, profile_id) = manager_with_profile(&dir).await;
        let outputs = Arc::new(std::sync::Mutex::new(Vec::new()));
        let port = echo_server(outputs.clone()).await;
        let terminals = TerminalManager::with_ssh_port(manager, port);

        let received = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink_received = received.clone();
        let on_data: TerminalDataSink = Arc::new(move |chunk| {
            sink_received.lock().unwrap().push(chunk);
            true
        });

        let opened = terminals
            .open(profile_id, 120, 40, on_data)
            .await
            .expect("terminal opens against the echo fixture");
        assert_eq!(opened.profile_id, profile_id);

        let payload = "interface print\n";
        terminals
            .write(opened.terminal_id, &BASE64.encode(payload.as_bytes()))
            .await
            .expect("write succeeds");

        wait_for(|| !received.lock().unwrap().is_empty()).await;
        let echoed: String = received
            .lock()
            .unwrap()
            .iter()
            .map(|chunk| String::from_utf8(BASE64.decode(chunk).unwrap()).unwrap())
            .collect();
        assert_eq!(echoed, payload, "echo fixture must return the input bytes");

        // Resize is a no-op against the fixture but must not error.
        terminals
            .resize(opened.terminal_id, 100, 30)
            .await
            .expect("resize succeeds");

        terminals
            .close(opened.terminal_id)
            .await
            .expect("close succeeds");
        // Double close → typed error, not a panic.
        let err = terminals.close(opened.terminal_id).await.unwrap_err();
        assert!(
            matches!(err, MikrotikManagerError::NoActiveSession),
            "unexpected error: {err}"
        );
        // Writes and resizes against the closed id are typed errors too.
        let err = terminals
            .resize(opened.terminal_id, 100, 30)
            .await
            .unwrap_err();
        assert!(
            matches!(err, MikrotikManagerError::NoActiveSession),
            "unexpected error: {err}"
        );
        // Unknown id on write → typed error too.
        let err = terminals
            .write(9_999, &BASE64.encode("x"))
            .await
            .unwrap_err();
        assert!(
            matches!(err, MikrotikManagerError::NoActiveSession),
            "unexpected error: {err}"
        );

        drop(dir);
    }

    #[tokio::test]
    async fn mikrotik_terminal_unknown_profile_is_typed_error() {
        let dir = temp_dir("unknown");
        let (manager, _profile_id) = manager_with_profile(&dir).await;
        let terminals = TerminalManager::with_ssh_port(manager, 1);
        let on_data: TerminalDataSink = Arc::new(|_chunk| true);

        let err = terminals.open(9_999, 120, 40, on_data).await.unwrap_err();
        assert!(
            matches!(err, MikrotikManagerError::ProfileNotFound(9_999)),
            "unexpected error: {err}"
        );

        drop(dir);
    }

    #[tokio::test]
    async fn mikrotik_terminal_cap_counts_inflight_opens() {
        let dir = temp_dir("cap");
        let (manager, profile_id) = manager_with_profile(&dir).await;
        let outputs = Arc::new(std::sync::Mutex::new(Vec::new()));
        let port = echo_server(outputs.clone()).await;
        let terminals = TerminalManager::with_ssh_port(manager, port);

        let mut ids = Vec::new();
        for _ in 0..MAX_CONCURRENT_SESSIONS {
            let on_data: TerminalDataSink = Arc::new(|_chunk| true);
            let opened = terminals
                .open(profile_id, 120, 40, on_data)
                .await
                .expect("terminal opens against the echo fixture");
            ids.push(opened.terminal_id);
        }

        // One over the cap; in-flight opens count toward it as well.
        let on_data: TerminalDataSink = Arc::new(|_chunk| true);
        let err = terminals
            .open(profile_id, 120, 40, on_data)
            .await
            .unwrap_err();
        assert!(
            matches!(err, MikrotikManagerError::TooManySessions),
            "unexpected error: {err}"
        );

        for id in ids {
            terminals.close(id).await.expect("close succeeds");
        }
        drop(dir);
    }

    #[tokio::test]
    async fn mikrotik_terminal_dead_sink_ends_the_session() {
        let dir = temp_dir("dead-sink");
        let (manager, profile_id) = manager_with_profile(&dir).await;
        let outputs = Arc::new(std::sync::Mutex::new(Vec::new()));
        let port = echo_server(outputs.clone()).await;
        let terminals = TerminalManager::with_ssh_port(manager, port);

        // The sink reports the frontend channel dead on the first chunk: the
        // reader must end the session and free the slot.
        let on_data: TerminalDataSink = Arc::new(|_chunk| false);
        let opened = terminals
            .open(profile_id, 120, 40, on_data)
            .await
            .expect("terminal opens against the echo fixture");

        terminals
            .write(opened.terminal_id, &BASE64.encode("ls\n".as_bytes()))
            .await
            .expect("write succeeds");

        // Cleanup runs in the reader task; poll until the slot is gone.
        wait_for(|| {
            terminals
                .inner
                .try_lock()
                .map(|inner| !inner.active.contains_key(&opened.terminal_id))
                .unwrap_or(false)
        })
        .await;
        let err = terminals.close(opened.terminal_id).await.unwrap_err();
        assert!(
            matches!(err, MikrotikManagerError::NoActiveSession),
            "unexpected error: {err}"
        );
        drop(dir);
    }
}
