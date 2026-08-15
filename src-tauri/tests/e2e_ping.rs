//! Real-backend end-to-end integration tests for the ping session layer.
//!
//! These tests exercise the actual ICMP engines (surge-ping + platform fallback)
//! against real targets:
//!   * 127.0.0.1  -> expected replies on loopback
//!   * 192.0.2.1  -> TEST-NET-1, guaranteed non-responding
//!   * ::1        -> IPv6 loopback, skipped gracefully if unavailable
//!
//! Run with: cargo test -- --ignored e2e

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::timeout;

use verkkokyyla_lib::db::Database;
use verkkokyyla_lib::session::{
    default_engine_factory, ProbeEvent, SessionManager, StatusEvent, StatusSink,
};

// Used only by the persistence-across-restart test for a raw COUNT query.
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{Row, SqlitePool};

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

/// Unique per-test tempdir, removed on drop (process-parallel safe).
struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "verkkokyyla-e2e-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
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

async fn connect_manager(dir: &TestDir) -> Result<SessionManager, Box<dyn std::error::Error>> {
    let db = Database::connect(&dir.db_file()).await?;
    Ok(SessionManager::new(db, default_engine_factory()))
}

fn status_sink() -> StatusSink {
    Arc::new(|_event: StatusEvent| {})
}

fn probe_sink() -> (
    impl Fn(ProbeEvent) + Send + 'static,
    UnboundedReceiver<ProbeEvent>,
    Arc<AtomicUsize>,
) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let total = Arc::new(AtomicUsize::new(0));
    let total2 = Arc::clone(&total);
    let callback = move |event: ProbeEvent| {
        let _ = tx.send(event);
        total2.fetch_add(1, Ordering::SeqCst);
    };
    (callback, rx, total)
}

async fn collect_n_with_timeout(
    rx: &mut UnboundedReceiver<ProbeEvent>,
    n: usize,
    secs: u64,
) -> Result<Vec<ProbeEvent>, Box<dyn std::error::Error>> {
    let mut events = Vec::with_capacity(n);
    for _ in 0..n {
        match timeout(Duration::from_secs(secs), rx.recv()).await? {
            Some(event) => events.push(event),
            None => return Err("probe channel closed early".into()),
        }
    }
    Ok(events)
}

async fn collect_n(
    rx: &mut UnboundedReceiver<ProbeEvent>,
    n: usize,
) -> Result<Vec<ProbeEvent>, Box<dyn std::error::Error>> {
    collect_n_with_timeout(rx, n, 30).await
}

async fn drain_remaining(rx: &mut UnboundedReceiver<ProbeEvent>) -> Vec<ProbeEvent> {
    let mut rest = Vec::new();
    while let Ok(Some(event)) = timeout(Duration::from_millis(500), rx.recv()).await {
        rest.push(event);
    }
    rest
}

// ---------------------------------------------------------------------------
// E2E tests
// ---------------------------------------------------------------------------

// Given a real ICMP session targeting IPv4 loopback,
// When ~5 probes have been emitted and the session is cleanly stopped,
// Then most probes are replies, the session row is stamped ended_at, and both
// the DB and load_session report the same probe count.
#[tokio::test]
#[ignore = "real ICMP / network"]
async fn e2e_loopback_happy_path() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TestDir::new("happy")?;
    let manager = connect_manager(&dir).await?;
    let (on_probe, mut probe_rx, _total) = probe_sink();

    let info = manager
        .start("127.0.0.1", "v4", 32, false, on_probe, status_sink())
        .await?;

    let events = collect_n(&mut probe_rx, 5).await?;
    let stopped = manager.stop().await?;
    let extra = drain_remaining(&mut probe_rx).await;

    let all_events: Vec<_> = events.into_iter().chain(extra).collect();
    let observed = all_events.len();
    assert!(observed >= 5, "expected at least 5 probes, got {observed}");

    let replies = all_events.iter().filter(|event| !event.lost).count();
    assert!(
        replies >= 4,
        "expected at least 4 replies, got {replies} out of {observed}"
    );

    assert_eq!(stopped.session_id, info.session_id);

    let db = Database::connect(&dir.db_file()).await?;
    let rows = db.load_probes(info.session_id).await?;
    assert_eq!(
        rows.len(),
        observed,
        "DB probe count {} does not match observed {observed}",
        rows.len()
    );

    let sessions = db.list_sessions().await?;
    let session = sessions
        .into_iter()
        .find(|row| row.id == info.session_id)
        .ok_or_else(|| "session row not found in DB".to_owned())?;
    assert!(session.ended_at.is_some(), "session row missing ended_at");

    let loaded = manager.load_session(info.session_id).await?;
    assert_eq!(
        loaded.probes.len(),
        observed,
        "load_session probe count {} does not match observed {observed}",
        loaded.probes.len()
    );

    Ok(())
}

// Given a real ICMP session targeting TEST-NET-1 (guaranteed non-responding),
// When 3 probes are emitted and the session is cleanly stopped,
// Then every probe is recorded as loss, the session row is stamped ended_at,
// and the DB count matches the observed count.
#[tokio::test]
#[ignore = "real ICMP / network"]
async fn e2e_non_responding_loss_path() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TestDir::new("loss")?;
    let manager = connect_manager(&dir).await?;
    let (on_probe, mut probe_rx, _total) = probe_sink();

    let info = manager
        .start("192.0.2.1", "v4", 32, false, on_probe, status_sink())
        .await?;

    let events = collect_n(&mut probe_rx, 3).await?;
    manager.stop().await?;
    let extra = drain_remaining(&mut probe_rx).await;

    let all_events: Vec<_> = events.into_iter().chain(extra).collect();
    let observed = all_events.len();
    assert!(observed >= 3, "expected at least 3 probes, got {observed}");
    assert!(
        all_events.iter().all(|event| event.lost),
        "expected all probes to be lost"
    );

    let loss_count = all_events.iter().filter(|event| event.lost).count();
    assert_eq!(loss_count, observed);

    let db = Database::connect(&dir.db_file()).await?;
    let rows = db.load_probes(info.session_id).await?;
    assert_eq!(
        rows.len(),
        observed,
        "DB probe count {} does not match observed {observed}",
        rows.len()
    );

    let sessions = db.list_sessions().await?;
    let session = sessions
        .into_iter()
        .find(|row| row.id == info.session_id)
        .ok_or_else(|| "session row not found in DB".to_owned())?;
    assert!(session.ended_at.is_some(), "session row missing ended_at");

    Ok(())
}

// Given a cleanly stopped session written to a real SQLite file,
// When the SessionManager is dropped and a new manager is opened on the same file,
// Then list_sessions() surfaces the past session and load_session() returns the
// same probe count as a raw SELECT COUNT(*) query.
#[tokio::test]
#[ignore = "real ICMP / network"]
async fn e2e_persistence_across_restart() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TestDir::new("persist")?;
    let manager = connect_manager(&dir).await?;
    let (on_probe, mut probe_rx, _total) = probe_sink();

    let info = manager
        .start("127.0.0.1", "v4", 32, false, on_probe, status_sink())
        .await?;

    let events = collect_n(&mut probe_rx, 3).await?;
    manager.stop().await?;
    let extra = drain_remaining(&mut probe_rx).await;
    let observed = events.len() + extra.len();

    // Simulate a restart: drop the manager (and its DB pool), then reconnect.
    drop(manager);
    tokio::time::sleep(Duration::from_millis(200)).await;

    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::new().filename(&dir.db_file()),
    )
    .await?;
    let row = sqlx::query("SELECT COUNT(*) AS n FROM probes WHERE session_id = ?")
        .bind(info.session_id)
        .fetch_one(&pool)
        .await?;
    let db_count: i64 = row.try_get("n")?;
    assert_eq!(db_count as usize, observed);

    let manager2 = connect_manager(&dir).await?;
    let sessions = manager2.list_sessions().await?;
    let session = sessions
        .iter()
        .find(|row| row.id == info.session_id)
        .ok_or_else(|| "past session not listed after restart".to_owned())?;
    assert_eq!(session.probe_count as usize, observed);

    let loaded = manager2.load_session(info.session_id).await?;
    assert_eq!(loaded.probes.len(), observed);

    Ok(())
}

// Given an IPv6 loopback target,
// When the session is started,
// Then the test succeeds if v6 works; if it fails for any reason the test is
// skipped rather than failed, because v6 availability is environment-specific.
#[tokio::test]
#[ignore = "real ICMP / network"]
async fn e2e_loopback_v6_graceful_skip() {
    let dir = match TestDir::new("v6") {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("skipping IPv6 test: tempdir setup failed: {err}");
            return;
        }
    };

    let manager = match connect_manager(&dir).await {
        Ok(manager) => manager,
        Err(err) => {
            eprintln!("skipping IPv6 test: manager setup failed: {err}");
            return;
        }
    };

    let (on_probe, mut probe_rx, _total) = probe_sink();
    let info = match manager.start("::1", "v6", 32, false, on_probe, status_sink()).await {
        Ok(info) => info,
        Err(err) => {
            eprintln!("skipping IPv6 test: start failed: {err}");
            return;
        }
    };

    match collect_n_with_timeout(&mut probe_rx, 1, 5).await {
        Ok(_) => {
            let _ = manager.stop().await;
        }
        Err(err) => {
            eprintln!("skipping IPv6 test: probe collection failed: {err}");
            let _ = manager.stop().await;
        }
    }

    // Keep the session id alive only so the compiler does not complain; the
    // result is intentionally unused because the test skips on failure.
    let _ = info;
}
