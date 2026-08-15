//! Session lifecycle layer: single-active-session guard, engine selection
//! with privilege-probe fallback, probe-loop + consumer tasks, and the
//! batched SQLite writer (flush every [`FLUSH_BATCH_SIZE`] probes OR
//! [`FLUSH_INTERVAL_MS`] ms, plus a final flush on clean stop).
//!
//! Durability contract: a hard crash may lose at most the current unflushed
//! batch window; every previously flushed probe survives.
//!
//! The manager is Tauri-free: it is constructed with a [`Database`] and an
//! injectable [`EngineFactory`], and events leave through plain sink
//! closures. The Tauri commands in `lib.rs` are thin wrappers that adapt
//! `tauri::ipc::Channel`s into those sinks.

use std::fmt;
use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::db::{now_rfc3339, Database, DbError, NewSession, ProbeRow, SessionSummary};
use crate::engine::{
    resolve_target, run_probe_loop, EngineError, Family, LoopProbe, PingEngine, ProbeResult,
    SurgePinger, PING_INTERVAL_MS, PING_TIMEOUT_MS,
};
#[cfg(unix)]
use crate::engine::OsPinger;
#[cfg(windows)]
use crate::engine::WinIcmpPinger;
use crate::stats::{ProbeOutcome, StatsEngine, StatsSnapshot};

/// Flush the write-behind batch once it reaches this many probes.
const FLUSH_BATCH_SIZE: usize = 50;
/// Flush the write-behind batch at least this often.
const FLUSH_INTERVAL_MS: u64 = 500;
/// Buffer between the probe loop and the consumer task.
const PROBE_CHANNEL_CAPACITY: usize = 256;

// ---------------------------------------------------------------------------
// IPC DTOs (serde wire shapes; the frontend consumes these).
// ---------------------------------------------------------------------------

/// Live per-probe event streamed over the `on_probe` channel.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeEvent {
    pub seq: u64,
    pub rtt_ms: Option<f64>,
    pub lost: bool,
    /// RFC 3339 timestamp of the probe.
    pub at: String,
}

/// Lifecycle event streamed over the `on_status` channel.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum StatusEvent {
    /// Engine chosen at session start; `fallback` is true when the primary
    /// (surge) engine was denied and the platform fallback was selected.
    EngineSelected { engine: String, fallback: bool },
    /// Non-fatal session error (e.g. a failed batch flush).
    Error { message: String },
    /// The session stopped cleanly and all probes were persisted.
    SessionStopped {
        session_id: i64,
        probe_count: u64,
        loss_count: u64,
    },
}

/// Wire mirror of [`StatsSnapshot`] (stats.rs is serde-free).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDto {
    pub count: u64,
    pub loss_count: u64,
    pub loss_fraction: f64,
    pub min_ms: Option<f64>,
    pub avg_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub stddev_ms: Option<f64>,
    pub jitter_ms: Option<f64>,
}

impl From<StatsSnapshot> for SnapshotDto {
    fn from(snap: StatsSnapshot) -> Self {
        Self {
            count: snap.count,
            loss_count: snap.loss_count,
            loss_fraction: snap.loss_fraction,
            min_ms: snap.min_ms,
            avg_ms: snap.avg_ms,
            max_ms: snap.max_ms,
            stddev_ms: snap.stddev_ms,
            jitter_ms: snap.jitter_ms,
        }
    }
}

/// Wire mirror of [`SessionSummary`] (db.rs is serde-free).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummaryDto {
    pub id: i64,
    pub target_input: String,
    pub resolved_ip: String,
    pub family: String,
    pub engine: String,
    pub interval_ms: i64,
    pub timeout_ms: i64,
    pub payload_size: i64,
    pub dont_fragment: bool,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub probe_count: i64,
    pub loss_count: i64,
    pub loss_percent: f64,
}

impl From<SessionSummary> for SessionSummaryDto {
    fn from(row: SessionSummary) -> Self {
        Self {
            id: row.id,
            target_input: row.target_input,
            resolved_ip: row.resolved_ip,
            family: row.family,
            engine: row.engine,
            interval_ms: row.interval_ms,
            timeout_ms: row.timeout_ms,
            payload_size: row.payload_size,
            dont_fragment: row.dont_fragment,
            started_at: row.started_at,
            ended_at: row.ended_at,
            probe_count: row.probe_count,
            loss_count: row.loss_count,
            loss_percent: row.loss_percent,
        }
    }
}

/// Wire mirror of [`ProbeRow`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeRowDto {
    pub seq: i64,
    pub rtt_ms: Option<f64>,
    pub loss: bool,
    pub at: String,
}

impl From<ProbeRow> for ProbeRowDto {
    fn from(row: ProbeRow) -> Self {
        Self {
            seq: row.seq,
            rtt_ms: row.rtt_ms,
            loss: row.loss,
            at: row.at,
        }
    }
}

/// A past session with its full probe history (`load_session` payload).
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedSessionDto {
    pub session: SessionSummaryDto,
    pub probes: Vec<ProbeRowDto>,
}

/// `start_session` return payload.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartInfoDto {
    pub session_id: i64,
    pub engine: String,
    pub fallback: bool,
    pub resolved_ip: String,
    /// All resolver answers (the selected address is `resolved_ip`).
    pub answers: Vec<String>,
    pub payload_size: i64,
    pub dont_fragment: bool,
}

/// `stop_session` return payload.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoppedSessionDto {
    pub session_id: i64,
    pub probe_count: u64,
    pub loss_count: u64,
    pub ended_at: String,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Typed session-layer errors; serialized to the frontend as
/// `{ kind, message }`.
#[derive(Debug)]
pub enum SessionError {
    /// A session is already running (single-active-session guard).
    AlreadyRunning,
    /// Stop/snapshot requested while no session is running.
    NotRunning,
    /// `family` was not one of "auto" | "v4" | "v6".
    InvalidFamily(String),
    /// `load_session`/`delete_session` referenced an unknown id.
    SessionNotFound(i64),
    /// Target resolution or engine creation failed.
    Engine(EngineError),
    /// Persistence failure.
    Db(DbError),
}

impl SessionError {
    fn kind(&self) -> &'static str {
        match self {
            Self::AlreadyRunning => "already-running",
            Self::NotRunning => "not-running",
            Self::InvalidFamily(_) => "invalid-family",
            Self::SessionNotFound(_) => "session-not-found",
            Self::Engine(_) => "engine",
            Self::Db(_) => "db",
        }
    }
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyRunning => write!(f, "a ping session is already running"),
            Self::NotRunning => write!(f, "no ping session is running"),
            Self::InvalidFamily(input) => {
                write!(f, "invalid family {input:?}; expected auto|v4|v6")
            }
            Self::SessionNotFound(id) => write!(f, "no session with id {id}"),
            Self::Engine(err) => write!(f, "{err}"),
            Self::Db(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Engine(err) => Some(err),
            Self::Db(err) => Some(err),
            _ => None,
        }
    }
}

impl Serialize for SessionError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("SessionError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

impl From<EngineError> for SessionError {
    fn from(err: EngineError) -> Self {
        Self::Engine(err)
    }
}

impl From<DbError> for SessionError {
    fn from(err: DbError) -> Self {
        Self::Db(err)
    }
}

// ---------------------------------------------------------------------------
// Engine selection
// ---------------------------------------------------------------------------

/// Which engine slot the factory is asked to build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineChoice {
    /// surge-ping (preferred on all platforms).
    Primary,
    /// Platform fallback: WinIcmp on Windows, OS ping binary on unix.
    Fallback,
}

/// Persisted/reported name for each engine slot.
pub fn engine_name(choice: EngineChoice) -> &'static str {
    match choice {
        EngineChoice::Primary => "surge",
        #[cfg(windows)]
        EngineChoice::Fallback => "winicmp",
        #[cfg(unix)]
        EngineChoice::Fallback => "osping",
    }
}

type EngineFactoryFuture =
    Pin<Box<dyn Future<Output = Result<PingEngine, EngineError>> + Send>>;

/// Injectable engine constructor. Tests substitute a Mock-scripted factory
/// (including one that simulates a permission-denied primary).
pub type EngineFactory =
    Arc<dyn Fn(IpAddr, u32, EngineChoice, usize, bool) -> EngineFactoryFuture + Send + Sync>;

/// Production factory: real surge / WinIcmp / OsPinger engines.
pub fn default_engine_factory() -> EngineFactory {
    Arc::new(|addr, scope_id, choice, payload_size, dont_fragment| {
        Box::pin(async move {
            match choice {
                EngineChoice::Primary => SurgePinger::new(addr, scope_id, payload_size, dont_fragment)
                    .await
                    .map(PingEngine::Surge),
                #[cfg(windows)]
                EngineChoice::Fallback => Ok(PingEngine::WinIcmp(WinIcmpPinger::new(
                    addr,
                    scope_id,
                    payload_size,
                ))),
                #[cfg(unix)]
                EngineChoice::Fallback => {
                    OsPinger::new(addr, payload_size, dont_fragment).map(PingEngine::OsPosix)
                }
            }
        })
    })
}

fn parse_family(input: &str) -> Result<Family, SessionError> {
    match input {
        "auto" => Ok(Family::Auto),
        "v4" => Ok(Family::V4),
        "v6" => Ok(Family::V6),
        other => Err(SessionError::InvalidFamily(other.to_owned())),
    }
}

fn family_str(family: Family) -> &'static str {
    match family {
        Family::Auto => "auto",
        Family::V4 => "v4",
        Family::V6 => "v6",
    }
}

/// Sink for lifecycle events; shared between the consumer task and `stop`.
pub type StatusSink = Arc<dyn Fn(StatusEvent) + Send + Sync>;

// ---------------------------------------------------------------------------
// Session manager
// ---------------------------------------------------------------------------

struct ActiveSession {
    session_id: i64,
    stop_tx: watch::Sender<bool>,
    loop_handle: JoinHandle<()>,
    consumer_handle: JoinHandle<()>,
    stats: Arc<Mutex<StatsEngine>>,
    on_status: StatusSink,
}

struct Inner {
    active: std::sync::Mutex<HashMap<i64, ActiveSession>>,
}

/// Owns the (at most one) active ping session. Clone-cheap (all Arc).
#[derive(Clone)]
pub struct SessionManager {
    db: Arc<Database>,
    factory: EngineFactory,
    inner: Arc<Inner>,
}

impl SessionManager {
    pub fn new(db: Database, factory: EngineFactory) -> Self {
        Self {
            db: Arc::new(db),
            factory,
            inner: Arc::new(Inner {
                active: std::sync::Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Resolve the target, select an engine (privilege probe with fallback),
    /// persist the session row, and spawn the loop + consumer tasks.
    ///
    /// The `active` mutex is held for the whole setup so concurrent starts
    /// are serialized and exactly one can win.
    pub async fn start<P>(
        &self,
        target: &str,
        family: &str,
        payload_size: usize,
        dont_fragment: bool,
        on_probe: P,
        on_status: StatusSink,
    ) -> Result<StartInfoDto, SessionError>
    where
        P: Fn(ProbeEvent) + Send + 'static,
    {
        let family = parse_family(family)?;
        let resolved = resolve_target(target, family).await?;
        let (mut engine, choice) = self
            .select_engine(resolved.selected, resolved.scope_id, payload_size, dont_fragment)
            .await?;
        let engine_label = engine_name(choice).to_owned();

        let session_id = self
            .db
            .create_session(&NewSession {
                target_input: target.to_owned(),
                resolved_ip: resolved.selected.to_string(),
                family: family_str(family).to_owned(),
                engine: engine_label.clone(),
                interval_ms: i64::try_from(PING_INTERVAL_MS).unwrap_or(i64::MAX),
                timeout_ms: i64::try_from(PING_TIMEOUT_MS).unwrap_or(i64::MAX),
                payload_size: i64::try_from(payload_size).unwrap_or(i64::MAX),
                dont_fragment,
                started_at: now_rfc3339(),
            })
            .await?;

        let stats = Arc::new(Mutex::new(StatsEngine::new()));
        on_status(StatusEvent::EngineSelected {
            engine: engine_label.clone(),
            fallback: choice == EngineChoice::Fallback,
        });

        let (stop_tx, mut stop_rx) = watch::channel(false);
        let (probe_tx, probe_rx) = mpsc::channel(PROBE_CHANNEL_CAPACITY);
        let loop_handle = tokio::spawn(async move {
            run_probe_loop(&mut engine, &mut stop_rx, &probe_tx).await;
        });
        let consumer_handle = tokio::spawn(consume_probes(
            probe_rx,
            Arc::clone(&self.db),
            session_id,
            Arc::clone(&stats),
            on_probe,
            Arc::clone(&on_status),
        ));

        let active = ActiveSession {
            session_id,
            stop_tx,
            loop_handle,
            consumer_handle,
            stats,
            on_status,
        };
        self.inner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session_id, active);

        Ok(StartInfoDto {
            session_id,
            engine: engine_label,
            fallback: choice == EngineChoice::Fallback,
            resolved_ip: resolved.selected.to_string(),
            answers: resolved.answers.iter().map(ToString::to_string).collect(),
            payload_size: i64::try_from(payload_size).unwrap_or(i64::MAX),
            dont_fragment,
        })
    }

    /// Privilege probe: try surge first; only a permission-denied socket
    /// error triggers the platform fallback. Any other failure propagates.
    async fn select_engine(
        &self,
        addr: IpAddr,
        scope_id: u32,
        payload_size: usize,
        dont_fragment: bool,
    ) -> Result<(PingEngine, EngineChoice), SessionError> {
        match (self.factory)(addr, scope_id, EngineChoice::Primary, payload_size, dont_fragment).await
        {
            Ok(engine) => Ok((engine, EngineChoice::Primary)),
            Err(EngineError::Socket(err))
                if err.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                let engine = (self.factory)(
                    addr,
                    scope_id,
                    EngineChoice::Fallback,
                    payload_size,
                    dont_fragment,
                )
                .await?;
                Ok((engine, EngineChoice::Fallback))
            }
            Err(other) => Err(SessionError::Engine(other)),
        }
    }

    /// Clean stop: signal the loop, join both tasks (the consumer performs
    /// the final flush before exiting), stamp `ended_at`, and announce
    /// `session-stopped`. A clean stop loses nothing.
    pub async fn stop(&self, session_id: i64) -> Result<StoppedSessionDto, SessionError> {
        let active = self
            .inner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&session_id)
            .ok_or(SessionError::NotRunning)?;

        let _ = active.stop_tx.send(true);
        // Join errors mean a task panicked; stopping must still finish the
        // session row, so they are deliberately not propagated.
        let _ = active.loop_handle.await;
        let _ = active.consumer_handle.await;

        let ended_at = now_rfc3339();
        self.db.finish_session(active.session_id, &ended_at).await?;

        let snap = lock_stats(&active.stats).snapshot();
        (active.on_status)(StatusEvent::SessionStopped {
            session_id: active.session_id,
            probe_count: snap.count,
            loss_count: snap.loss_count,
        });
        Ok(StoppedSessionDto {
            session_id: active.session_id,
            probe_count: snap.count,
            loss_count: snap.loss_count,
            ended_at,
        })
    }

    /// Aggregates of an active session.
    pub fn snapshot(&self, session_id: i64) -> SnapshotDto {
        let guard = self
            .inner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let snap = match guard.get(&session_id) {
            Some(session) => lock_stats(&session.stats).snapshot(),
            None => StatsEngine::new().snapshot(),
        };
        SnapshotDto::from(snap)
    }

    pub fn active_session_ids(&self) -> Vec<i64> {
        let guard = self
            .inner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        guard.keys().copied().collect()
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionSummaryDto>, SessionError> {
        let rows = self.db.list_sessions().await?;
        Ok(rows.into_iter().map(SessionSummaryDto::from).collect())
    }

    pub async fn load_session(&self, id: i64) -> Result<LoadedSessionDto, SessionError> {
        let session = self
            .db
            .list_sessions()
            .await?
            .into_iter()
            .find(|row| row.id == id)
            .ok_or(SessionError::SessionNotFound(id))?;
        let probes = self.db.load_probes(id).await?;
        Ok(LoadedSessionDto {
            session: SessionSummaryDto::from(session),
            probes: probes.into_iter().map(ProbeRowDto::from).collect(),
        })
    }

    pub async fn delete_session(&self, id: i64) -> Result<(), SessionError> {
        self.db.delete_session(id).await?;
        Ok(())
    }

    /// Test-only crash simulation: abort both tasks mid-flight without a
    /// final flush or `ended_at` stamp, then release the session slot.
    #[cfg(test)]
    async fn simulate_crash(&self, session_id: i64) {
        let active = self
            .inner
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&session_id);
        if let Some(active) = active {
            active.loop_handle.abort();
            active.consumer_handle.abort();
        }
    }
}

fn lock_stats(stats: &Mutex<StatsEngine>) -> MutexGuard<'_, StatsEngine> {
    stats.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Maps one engine probe onto the stats outcome, the persisted row, and the
/// live event (the three consumers of a single probe).
fn map_probe(seq: u64, result: &ProbeResult) -> (ProbeOutcome, ProbeRow, ProbeEvent) {
    let (outcome, rtt_ms, lost) = match result {
        ProbeResult::Rtt(rtt) => (
            ProbeOutcome::Rtt(*rtt),
            Some(rtt.as_secs_f64() * 1000.0),
            false,
        ),
        ProbeResult::Timeout => (ProbeOutcome::Timeout, None, true),
        ProbeResult::Error(msg) => (ProbeOutcome::Error(msg.clone()), None, true),
    };
    let at = now_rfc3339();
    let row = ProbeRow {
        seq: i64::try_from(seq).unwrap_or(i64::MAX),
        rtt_ms,
        loss: lost,
        at: at.clone(),
    };
    let event = ProbeEvent {
        seq,
        rtt_ms,
        lost,
        at,
    };
    (outcome, row, event)
}

/// Consumer task: feed stats, emit live events, and batch probes for the
/// write-behind DB flusher. Ends (with a final flush) when the loop task
/// finishes and drops its sender.
async fn consume_probes<P>(
    mut rx: mpsc::Receiver<LoopProbe>,
    db: Arc<Database>,
    session_id: i64,
    stats: Arc<Mutex<StatsEngine>>,
    on_probe: P,
    on_status: StatusSink,
) where
    P: Fn(ProbeEvent) + Send,
{
    let mut batch: Vec<ProbeRow> = Vec::with_capacity(FLUSH_BATCH_SIZE);
    let mut ticker = tokio::time::interval(Duration::from_millis(FLUSH_INTERVAL_MS));
    loop {
        tokio::select! {
            maybe = rx.recv() => {
                let Some(probe) = maybe else {
                    flush_batch(&db, session_id, &mut batch, &on_status).await;
                    break;
                };
                let (outcome, row, event) = map_probe(probe.seq, &probe.result);
                lock_stats(&stats).feed(outcome);
                on_probe(event);
                batch.push(row);
                if batch.len() >= FLUSH_BATCH_SIZE {
                    flush_batch(&db, session_id, &mut batch, &on_status).await;
                }
            }
            _ = ticker.tick() => {
                flush_batch(&db, session_id, &mut batch, &on_status).await;
            }
        }
    }
}

/// One flush of the write-behind batch. On DB error the batch is dropped
/// (bounded memory beats unbounded retry) and an error status is emitted.
async fn flush_batch(
    db: &Database,
    session_id: i64,
    batch: &mut Vec<ProbeRow>,
    on_status: &StatusSink,
) {
    if batch.is_empty() {
        return;
    }
    if let Err(err) = db.insert_probes_batch(session_id, batch).await {
        on_status(StatusEvent::Error {
            message: format!("failed to persist probe batch: {err}"),
        });
    }
    batch.clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::MockPinger;
    use std::path::PathBuf;
    use std::time::SystemTime;
    use tokio::sync::mpsc::UnboundedReceiver;

    /// Unique per-test tempdir, removed on drop (process-parallel safe).
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let dir = std::env::temp_dir().join(format!(
                "verkkokyyla-session-{name}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }

        fn db_file(&self) -> PathBuf {
            self.0.join("test.db")
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Factory whose primary (and fallback) slots build a scripted Mock.
    fn mock_factory(script: Vec<ProbeResult>) -> EngineFactory {
        Arc::new(move |_, _, _choice, _payload_size, _dont_fragment| {
            let script = script.clone();
            Box::pin(async move { Ok(PingEngine::Mock(MockPinger::new(script))) })
        })
    }

    /// Factory whose primary slot fails with permission-denied; the fallback
    /// slot builds a scripted Mock.
    fn denied_factory(script: Vec<ProbeResult>) -> EngineFactory {
        Arc::new(move |_, _, choice, _payload_size, _dont_fragment| {
            let script = script.clone();
            Box::pin(async move {
                match choice {
                    EngineChoice::Primary => Err(EngineError::Socket(
                        std::io::Error::from(std::io::ErrorKind::PermissionDenied),
                    )),
                    EngineChoice::Fallback => Ok(PingEngine::Mock(MockPinger::new(script))),
                }
            })
        })
    }

    fn rtt_script(count: usize) -> Vec<ProbeResult> {
        vec![ProbeResult::Rtt(Duration::from_millis(10)); count]
    }

    async fn test_manager(dir: &TestDir, factory: EngineFactory) -> SessionManager {
        let db = Database::connect(&dir.db_file()).await.expect("connect test db");
        SessionManager::new(db, factory)
    }

    /// Probe sink + receiver pair for a test.
    fn probe_sink() -> (
        impl Fn(ProbeEvent) + Send + 'static,
        UnboundedReceiver<ProbeEvent>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        (move |event| {
            let _ = tx.send(event);
        }, rx)
    }

    fn status_sink() -> (StatusSink, UnboundedReceiver<StatusEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Arc::new(move |event: StatusEvent| {
            let _ = tx.send(event);
        }), rx)
    }

    async fn next_probe(rx: &mut UnboundedReceiver<ProbeEvent>) -> ProbeEvent {
        match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) => panic!("probe channel closed early"),
            Err(_) => panic!("timed out waiting for probe event"),
        }
    }

    async fn next_status(rx: &mut UnboundedReceiver<StatusEvent>) -> StatusEvent {
        match tokio::time::timeout(Duration::from_secs(15), rx.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) => panic!("status channel closed early"),
            Err(_) => panic!("timed out waiting for status event"),
        }
    }

    // Given a mock-scripted session,
    // When 5 probes are observed and the session is cleanly stopped,
    // Then the DB holds exactly those 5 probes (clean stop loses nothing),
    // the session row records the primary engine name and a stamped end, and
    // the status channel carried engine-selected + session-stopped.
    #[tokio::test]
    async fn clean_stop_after_five_probes_persists_everything() {
        let dir = TestDir::new("clean-stop");
        let manager = test_manager(&dir, mock_factory(rtt_script(5))).await;
        let (on_probe, mut probe_rx) = probe_sink();
        let (on_status, mut status_rx) = status_sink();

        let info = manager
            .start("127.0.0.1", "v4", 32, false, on_probe, on_status)
            .await
            .expect("start");
        assert_eq!(info.engine, "surge");
        assert!(!info.fallback);
        assert_eq!(info.resolved_ip, "127.0.0.1");

        for expected_seq in 1..=5u64 {
            let event = next_probe(&mut probe_rx).await;
            assert_eq!(event.seq, expected_seq);
            assert!(!event.lost);
            assert_eq!(event.rtt_ms, Some(10.0));
        }
        // The consumer feeds stats before emitting the event, so the
        // snapshot already reflects all 5 probes.
        let snap = manager.snapshot(info.session_id);
        assert_eq!(snap.count, 5);
        assert_eq!(snap.loss_count, 0);
        assert_eq!(snap.min_ms, Some(10.0));

        let stopped = manager.stop(info.session_id).await.expect("stop");
        assert_eq!(stopped.session_id, info.session_id);
        assert_eq!(stopped.probe_count, 5);
        assert_eq!(stopped.loss_count, 0);

        let mut events = Vec::new();
        while let Ok(event) = status_rx.try_recv() {
            events.push(event);
        }
        assert_eq!(
            events.first(),
            Some(&StatusEvent::EngineSelected {
                engine: "surge".to_owned(),
                fallback: false,
            })
        );
        assert!(events.iter().any(|event| matches!(
            event,
            StatusEvent::SessionStopped { session_id, .. } if *session_id == info.session_id
        )));
        assert!(!events.iter().any(|event| matches!(event, StatusEvent::Error { .. })));

        // Direct DB verification.
        let probes = manager
            .db
            .load_probes(info.session_id)
            .await
            .expect("load probes");
        assert_eq!(probes.len(), 5);
        assert!(probes.iter().all(|row| row.rtt_ms == Some(10.0) && !row.loss));
        assert_eq!(probes.first().map(|row| row.seq), Some(1));
        assert_eq!(probes.last().map(|row| row.seq), Some(5));

        let sessions = manager.db.list_sessions().await.expect("list sessions");
        assert_eq!(sessions.len(), 1);
        let row = &sessions[0];
        assert_eq!(row.engine, "surge");
        assert_eq!(row.family, "v4");
        assert_eq!(row.interval_ms, i64::try_from(PING_INTERVAL_MS).unwrap_or(0));
        assert_eq!(row.timeout_ms, i64::try_from(PING_TIMEOUT_MS).unwrap_or(0));
        assert_eq!(row.probe_count, 5);
        assert!(row.ended_at.is_some());
    }

    // Given a running session (and none),
    // When lifecycle commands arrive out of order,
    // Then stop-without-start and double-start produce typed errors
    // (no panic), and an unknown family string is rejected.
    #[tokio::test]
    async fn guard_errors_are_typed_and_never_panic() {
        let dir = TestDir::new("guards");
        let manager = test_manager(&dir, mock_factory(rtt_script(60))).await;

        assert!(matches!(
            manager.stop(1234).await,
            Err(SessionError::NotRunning)
        ));

        let (on_probe, _pr) = probe_sink();
        let (on_status, _sr) = status_sink();
        assert!(matches!(
            manager.start("127.0.0.1", "bogus", 32, false, on_probe, on_status).await,
            Err(SessionError::InvalidFamily(_))
        ));

        let (on_probe, _pr2) = probe_sink();
        let (on_status, _sr2) = status_sink();
        let info1 = manager
            .start("127.0.0.1", "auto", 32, false, on_probe, on_status)
            .await
            .expect("first start");

        let (on_probe2, _pr3) = probe_sink();
        let (on_status2, _sr3) = status_sink();
        let info2 = manager
            .start("127.0.0.1", "auto", 32, false, on_probe2, on_status2)
            .await
            .expect("second start succeeds");

        manager.stop(info1.session_id).await.expect("stop first");
        manager.stop(info2.session_id).await.expect("stop second");
        assert!(matches!(
            manager.stop(info1.session_id).await,
            Err(SessionError::NotRunning)
        ));
    }

    // Given a factory whose primary engine is denied (EPERM),
    // When a session starts,
    // Then the platform fallback engine is selected, recorded in the session
    // row, and announced via an engine-selected status event with
    // fallback = true.
    #[tokio::test]
    async fn permission_denied_primary_selects_platform_fallback() {
        let dir = TestDir::new("fallback");
        let manager = test_manager(&dir, denied_factory(rtt_script(2))).await;
        let (on_probe, _pr) = probe_sink();
        let (on_status, mut status_rx) = status_sink();

        let info = manager
            .start("127.0.0.1", "auto", 32, false, on_probe, on_status)
            .await
            .expect("start via fallback");
        let expected = engine_name(EngineChoice::Fallback);
        assert_eq!(info.engine, expected);
        assert!(info.fallback);

        let first = next_status(&mut status_rx).await;
        assert_eq!(
            first,
            StatusEvent::EngineSelected {
                engine: expected.to_owned(),
                fallback: true,
            }
        );

        manager.stop(info.session_id).await.expect("stop");
        let sessions = manager.db.list_sessions().await.expect("list sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].engine, expected);
    }

    // Given a session crashed (tasks aborted) mid-window after 4 probes,
    // When the DB is inspected,
    // Then all probes flushed before the crash survive, at most the
    // unflushed window (here: at most 1 probe, since the 500 ms timer
    // flushes between 1 s probe ticks) is lost, `ended_at` was never
    // stamped, and the manager accepts a new session afterwards.
    #[tokio::test]
    async fn crash_between_flushes_loses_at_most_unflushed_window() {
        let dir = TestDir::new("crash");
        let manager = test_manager(&dir, mock_factory(rtt_script(10))).await;
        let (on_probe, mut probe_rx) = probe_sink();
        let (on_status, _sr) = status_sink();

        let info = manager
            .start("127.0.0.1", "v4", 32, false, on_probe, on_status)
            .await
            .expect("start");
        let mut received = 0u64;
        for _ in 0..4 {
            next_probe(&mut probe_rx).await;
            received += 1;
        }

        manager.simulate_crash(info.session_id).await;

        let persisted = u64::try_from(
            manager
                .db
                .load_probes(info.session_id)
                .await
                .expect("load probes")
                .len(),
        )
        .unwrap_or(0);
        assert!(persisted <= received, "persisted {persisted} > received {received}");
        assert!(
            received - persisted <= 1,
            "lost {} probes; the unflushed window allows at most 1",
            received - persisted
        );

        let sessions = manager.db.list_sessions().await.expect("list sessions");
        assert_eq!(sessions.len(), 1);
        assert!(sessions[0].ended_at.is_none(), "crash must not stamp ended_at");

        // The session slot is released after a crash.
        let (on_probe2, _pr2) = probe_sink();
        let (on_status2, _sr2) = status_sink();
        let info2 = manager
            .start("127.0.0.1", "v4", 32, false, on_probe2, on_status2)
            .await
            .expect("restart after crash");
        manager.stop(info2.session_id).await.expect("stop after restart");
        assert_ne!(info.session_id, info2.session_id);
    }

    // Given a persisted past session,
    // When listed / loaded / deleted through the manager,
    // Then the DTOs mirror the DB shapes and unknown ids are typed errors.
    #[tokio::test]
    async fn list_load_delete_round_trip_past_sessions() {
        let dir = TestDir::new("history");
        let manager = test_manager(&dir, mock_factory(rtt_script(1))).await;
        let (on_probe, mut probe_rx) = probe_sink();
        let (on_status, _sr) = status_sink();

        let info = manager
            .start("localhost", "auto", 32, false, on_probe, on_status)
            .await
            .expect("start");
        next_probe(&mut probe_rx).await;
        manager.stop(info.session_id).await.expect("stop");

        let list = manager.list_sessions().await.expect("list");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, info.session_id);
        assert_eq!(list[0].target_input, "localhost");
        assert_eq!(list[0].probe_count, 1);

        let loaded = manager.load_session(info.session_id).await.expect("load");
        assert_eq!(loaded.session.id, info.session_id);
        assert_eq!(loaded.probes.len(), 1);
        assert_eq!(loaded.probes[0].seq, 1);
        assert_eq!(loaded.probes[0].rtt_ms, Some(10.0));

        assert!(matches!(
            manager.load_session(999_999).await,
            Err(SessionError::SessionNotFound(999_999))
        ));

        manager.delete_session(info.session_id).await.expect("delete");
        assert!(manager.list_sessions().await.expect("list").is_empty());
    }
}
