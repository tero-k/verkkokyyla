//! SQLite persistence layer (schema v1-v3: `sessions` + `probes` + `traces`).
//!
//! Owns the database file lifecycle and all SQL. Callers pass timestamps as
//! RFC 3339 strings (`now_rfc3339` / `system_time_to_rfc3339` are provided so
//! no chrono/time dependency is needed). This module is independent of the
//! stats/engine layers; the session layer (todo 7) maps domain types onto
//! [`NewSession`] / [`ProbeRow`].

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Row;

/// File name of the production database inside the Tauri app data dir.
const DB_FILE_NAME: &str = "verkkokyyla.db";

/// Resolves the production database path under the Tauri app data dir.
///
/// The caller (todo 7) obtains the dir via `app.path().app_data_dir()`;
/// keeping the dir as a parameter leaves this module free of Tauri deps and
/// lets tests point at tempdirs.
pub fn db_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join(DB_FILE_NAME)
}

/// Errors produced by the persistence layer.
#[derive(Debug)]
pub enum DbError {
    /// Filesystem failure while preparing the database file location.
    Io(std::io::Error),
    /// Failure from a SQL statement or pool operation.
    Sqlx(sqlx::Error),
    /// Migration failure (includes checksum mismatch on tampered DBs).
    Migrate(sqlx::migrate::MigrateError),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "database file error: {e}"),
            Self::Sqlx(e) => write!(f, "database query error: {e}"),
            Self::Migrate(e) => write!(f, "database migration error: {e}"),
        }
    }
}

impl std::error::Error for DbError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Sqlx(e) => Some(e),
            Self::Migrate(e) => Some(e),
        }
    }
}

impl From<std::io::Error> for DbError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<sqlx::Error> for DbError {
    fn from(e: sqlx::Error) -> Self {
        Self::Sqlx(e)
    }
}

impl From<sqlx::migrate::MigrateError> for DbError {
    fn from(e: sqlx::migrate::MigrateError) -> Self {
        Self::Migrate(e)
    }
}

/// Row shape for inserting / reading probes (persistence shape; the session
/// layer maps `ProbeOutcome` onto this).
#[derive(Debug, Clone, PartialEq)]
pub struct ProbeRow {
    pub seq: i64,
    pub rtt_ms: Option<f64>,
    pub loss: bool,
    /// RFC 3339 timestamp of the probe.
    pub at: String,
}

/// Parameters for starting a new session row.
#[derive(Debug, Clone)]
pub struct NewSession {
    pub target_input: String,
    pub resolved_ip: String,
    pub family: String,
    pub engine: String,
    pub interval_ms: i64,
    pub timeout_ms: i64,
    pub payload_size: i64,
    pub dont_fragment: bool,
    /// RFC 3339 session start timestamp.
    pub started_at: String,
}

/// Session row joined with aggregate probe stats (probe count, loss count,
/// loss percent computed in SQL).
#[derive(Debug, Clone, PartialEq)]
pub struct SessionSummary {
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

/// Parameters for starting a new trace row.
#[derive(Debug, Clone)]
pub struct NewTrace {
    pub target_input: String,
    pub resolved_ip: String,
    pub family: String,
    pub engine: String,
    pub max_hops: i64,
    /// RFC 3339 trace start timestamp.
    pub started_at: String,
}

/// Trace row joined with aggregate hop stats.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceSummary {
    pub id: i64,
    pub target_input: String,
    pub resolved_ip: String,
    pub family: String,
    pub engine: String,
    pub max_hops: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub reached_target: bool,
    pub hop_count: i64,
}

/// Hop row stored for a trace.
#[derive(Debug, Clone, PartialEq)]
pub struct TraceHopRow {
    pub trace_id: i64,
    pub hop: i64,
    pub address: Option<String>,
    pub hostname: Option<String>,
    pub rtt1_ms: Option<f64>,
    pub rtt2_ms: Option<f64>,
    pub rtt3_ms: Option<f64>,
    pub annotation: Option<String>,
    /// RFC 3339 timestamp of the hop.
    pub at: String,
}

/// Connection pool wrapper owning all database access.
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Opens (creating if missing) the SQLite file at `path`, creating parent
    /// directories as needed, and runs embedded migrations. Opening an
    /// already-migrated database is a no-op.
    pub async fn connect(path: &Path) -> Result<Self, DbError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePool::connect_with(options).await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    /// Inserts a session row and returns its id.
    pub async fn create_session(&self, session: &NewSession) -> Result<i64, DbError> {
        let result = sqlx::query(
            "INSERT INTO sessions \
             (target_input, resolved_ip, family, engine, interval_ms, timeout_ms, payload_size, dont_fragment, started_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&session.target_input)
        .bind(&session.resolved_ip)
        .bind(&session.family)
        .bind(&session.engine)
        .bind(session.interval_ms)
        .bind(session.timeout_ms)
        .bind(session.payload_size)
        .bind(session.dont_fragment)
        .bind(&session.started_at)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Inserts a trace row and returns its id.
    pub async fn create_trace(&self, trace: &NewTrace) -> Result<i64, DbError> {
        let result = sqlx::query(
            "INSERT INTO traces (target_input, resolved_ip, family, engine, max_hops, started_at) \
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&trace.target_input)
        .bind(&trace.resolved_ip)
        .bind(&trace.family)
        .bind(&trace.engine)
        .bind(trace.max_hops)
        .bind(&trace.started_at)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Finalizes a trace and writes its hops in one transaction.
    pub async fn complete_trace_with_hops(
        &self,
        id: i64,
        ended_at: &str,
        status: &str,
        reached_target: bool,
        hops: &[TraceHopRow],
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE traces SET ended_at = ?, status = ?, reached_target = ? WHERE id = ?")
            .bind(ended_at)
            .bind(status)
            .bind(reached_target)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for hop in hops {
            sqlx::query(
                "INSERT INTO trace_hops \
                 (trace_id, hop, address, hostname, rtt1_ms, rtt2_ms, rtt3_ms, annotation, at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(hop.trace_id)
            .bind(hop.hop)
            .bind(&hop.address)
            .bind(&hop.hostname)
            .bind(hop.rtt1_ms)
            .bind(hop.rtt2_ms)
            .bind(hop.rtt3_ms)
            .bind(&hop.annotation)
            .bind(&hop.at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Stamps `ended_at` (RFC 3339) on a session.
    pub async fn finish_session(&self, id: i64, ended_at: &str) -> Result<(), DbError> {
        sqlx::query("UPDATE sessions SET ended_at = ? WHERE id = ?")
            .bind(ended_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Inserts a batch of probe rows in a single transaction.
    pub async fn insert_probes_batch(
        &self,
        session_id: i64,
        probes: &[ProbeRow],
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        for probe in probes {
            sqlx::query(
                "INSERT INTO probes (session_id, seq, rtt_ms, loss, at) VALUES (?, ?, ?, ?, ?)",
            )
            .bind(session_id)
            .bind(probe.seq)
            .bind(probe.rtt_ms)
            .bind(probe.loss)
            .bind(&probe.at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Lists all sessions (newest first) with probe counts and loss percent.
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>, DbError> {
        let rows = sqlx::query(
            "SELECT s.id, s.target_input, s.resolved_ip, s.family, s.engine, \
                    s.interval_ms, s.timeout_ms, s.payload_size, s.dont_fragment, s.started_at, s.ended_at, \
                    COUNT(p.id) AS probe_count, \
                    COALESCE(SUM(p.loss), 0) AS loss_count, \
                    CASE WHEN COUNT(p.id) = 0 THEN 0.0 \
                         ELSE 100.0 * COALESCE(SUM(p.loss), 0) / COUNT(p.id) \
                    END AS loss_percent \
             FROM sessions s LEFT JOIN probes p ON p.session_id = s.id \
             GROUP BY s.id ORDER BY s.id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(SessionSummary {
                    id: row.try_get("id")?,
                    target_input: row.try_get("target_input")?,
                    resolved_ip: row.try_get("resolved_ip")?,
                    family: row.try_get("family")?,
                    engine: row.try_get("engine")?,
                    interval_ms: row.try_get("interval_ms")?,
                    timeout_ms: row.try_get("timeout_ms")?,
                    payload_size: row.try_get("payload_size")?,
                    dont_fragment: row.try_get("dont_fragment")?,
                    started_at: row.try_get("started_at")?,
                    ended_at: row.try_get("ended_at")?,
                    probe_count: row.try_get("probe_count")?,
                    loss_count: row.try_get("loss_count")?,
                    loss_percent: row.try_get("loss_percent")?,
                })
            })
            .collect()
    }

    /// Loads all probe rows of a session ordered by sequence number.
    pub async fn load_probes(&self, session_id: i64) -> Result<Vec<ProbeRow>, DbError> {
        let rows = sqlx::query(
            "SELECT seq, rtt_ms, loss, at FROM probes WHERE session_id = ? ORDER BY seq",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(ProbeRow {
                    seq: row.try_get("seq")?,
                    rtt_ms: row.try_get("rtt_ms")?,
                    loss: row.try_get("loss")?,
                    at: row.try_get("at")?,
                })
            })
            .collect()
    }

    /// Deletes a session; its probes are removed via ON DELETE CASCADE.
    pub async fn delete_session(&self, id: i64) -> Result<(), DbError> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Updates a trace hop hostname by trace id and hop number.
    pub async fn update_trace_hop_hostname(
        &self,
        trace_id: i64,
        hop: i64,
        hostname: Option<&str>,
    ) -> Result<(), DbError> {
        sqlx::query("UPDATE trace_hops SET hostname = ? WHERE trace_id = ? AND hop = ?")
            .bind(hostname)
            .bind(trace_id)
            .bind(hop)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Lists all traces (newest first) with hop counts.
    pub async fn list_traces(&self) -> Result<Vec<TraceSummary>, DbError> {
        let rows = sqlx::query(
            "SELECT t.id, t.target_input, t.resolved_ip, t.family, t.engine, t.max_hops, \
                    t.started_at, t.ended_at, t.status, t.reached_target, \
                    COALESCE((SELECT COUNT(*) FROM trace_hops h WHERE h.trace_id = t.id), 0) AS hop_count \
             FROM traces t ORDER BY t.id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(TraceSummary {
                    id: row.try_get("id")?,
                    target_input: row.try_get("target_input")?,
                    resolved_ip: row.try_get("resolved_ip")?,
                    family: row.try_get("family")?,
                    engine: row.try_get("engine")?,
                    max_hops: row.try_get("max_hops")?,
                    started_at: row.try_get("started_at")?,
                    ended_at: row.try_get("ended_at")?,
                    status: row.try_get("status")?,
                    reached_target: row.try_get("reached_target")?,
                    hop_count: row.try_get("hop_count")?,
                })
            })
            .collect()
    }

    /// Loads all hop rows for a trace ordered by hop number.
    pub async fn load_trace_hops(&self, trace_id: i64) -> Result<Vec<TraceHopRow>, DbError> {
        let rows = sqlx::query(
            "SELECT trace_id, hop, address, hostname, rtt1_ms, rtt2_ms, rtt3_ms, annotation, at \
             FROM trace_hops WHERE trace_id = ? ORDER BY hop",
        )
        .bind(trace_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(TraceHopRow {
                    trace_id: row.try_get("trace_id")?,
                    hop: row.try_get("hop")?,
                    address: row.try_get("address")?,
                    hostname: row.try_get("hostname")?,
                    rtt1_ms: row.try_get("rtt1_ms")?,
                    rtt2_ms: row.try_get("rtt2_ms")?,
                    rtt3_ms: row.try_get("rtt3_ms")?,
                    annotation: row.try_get("annotation")?,
                    at: row.try_get("at")?,
                })
            })
            .collect()
    }

    /// Deletes a trace; its hops are removed via ON DELETE CASCADE.
    pub async fn delete_trace(&self, id: i64) -> Result<(), DbError> {
        sqlx::query("DELETE FROM traces WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

/// Formats a `SystemTime` as an RFC 3339 UTC timestamp (`YYYY-MM-DDTHH:MM:SSZ`).
pub fn system_time_to_rfc3339(time: SystemTime) -> String {
    let secs: i64 = match time.duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs() as i64),
    };
    let days = secs.div_euclid(86_400);
    let day_secs = secs.rem_euclid(86_400);
    // Howard Hinnant's civil-from-days algorithm (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!(
        "{year:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        day_secs / 3_600,
        (day_secs % 3_600) / 60,
        day_secs % 60
    )
}

/// Current UTC time as an RFC 3339 string.
pub fn now_rfc3339() -> String {
    system_time_to_rfc3339(SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    /// Unique per-test tempdir, removed on drop (process-parallel safe).
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let dir = std::env::temp_dir().join(format!(
                "verkkokyyla-db-{name}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }

        /// DB path inside a nested subdir, exercising parent-dir creation.
        fn db_file(&self) -> PathBuf {
            self.0.join("nested").join("test.db")
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sample_session() -> NewSession {
        NewSession {
            target_input: "localhost".to_string(),
            resolved_ip: "127.0.0.1".to_string(),
            family: "v4".to_string(),
            engine: "surge".to_string(),
            interval_ms: 1000,
            timeout_ms: 1000,
            payload_size: 32,
            dont_fragment: false,
            started_at: now_rfc3339(),
        }
    }

    fn probe(seq: i64, rtt_ms: Option<f64>) -> ProbeRow {
        ProbeRow {
            seq,
            rtt_ms,
            loss: rtt_ms.is_none(),
            at: now_rfc3339(),
        }
    }

    fn sample_trace() -> NewTrace {
        NewTrace {
            target_input: "example.com".to_string(),
            resolved_ip: "203.0.113.10".to_string(),
            family: "v4".to_string(),
            engine: "traceroute".to_string(),
            max_hops: 30,
            started_at: now_rfc3339(),
        }
    }

    fn trace_hop(
        trace_id: i64,
        hop: i64,
        address: Option<&str>,
        hostname: Option<&str>,
    ) -> TraceHopRow {
        TraceHopRow {
            trace_id,
            hop,
            address: address.map(std::string::ToString::to_string),
            hostname: hostname.map(std::string::ToString::to_string),
            rtt1_ms: Some(12.5),
            rtt2_ms: None,
            rtt3_ms: Some(11.0),
            annotation: None,
            at: now_rfc3339(),
        }
    }

    async fn migration_count(db: &Database) -> i64 {
        let row = sqlx::query("SELECT COUNT(*) AS n FROM _sqlx_migrations")
            .fetch_one(&db.pool)
            .await
            .expect("count migrations");
        row.try_get("n").expect("read count")
    }

    #[tokio::test]
    async fn fresh_db_migrates_to_v3() {
        let dir = TestDir::new("fresh");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        assert_eq!(migration_count(&db).await, 3);
        let versions = sqlx::query("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&db.pool)
            .await
            .expect("read versions");
        let versions: Vec<i64> = versions
            .iter()
            .map(|row| row.try_get::<i64, _>("version").expect("version"))
            .collect();
        assert_eq!(versions, vec![1, 2, 3]);

        let tables =
            sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
                .fetch_all(&db.pool)
                .await
                .expect("read tables");
        let tables: Vec<String> = tables
            .iter()
            .map(|row| row.try_get::<String, _>("name").expect("table name"))
            .collect();
        assert!(tables.contains(&"traces".to_string()));
        assert!(tables.contains(&"trace_hops".to_string()));

        let indexes =
            sqlx::query("SELECT name FROM sqlite_master WHERE type = 'index' ORDER BY name")
                .fetch_all(&db.pool)
                .await
                .expect("read indexes");
        let indexes: Vec<String> = indexes
            .iter()
            .map(|row| row.try_get::<String, _>("name").expect("index name"))
            .collect();
        assert!(indexes.contains(&"idx_trace_hops_trace".to_string()));
    }

    #[tokio::test]
    async fn second_open_is_noop() {
        let dir = TestDir::new("reopen");
        let path = dir.db_file();
        let db = Database::connect(&path).await.expect("first connect");
        assert_eq!(migration_count(&db).await, 3);
        drop(db);
        let db = Database::connect(&path).await.expect("second connect");
        assert_eq!(migration_count(&db).await, 3);
    }

    #[tokio::test]
    async fn batch_insert_1000_probes_under_500ms() {
        let dir = TestDir::new("batch");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let session_id = db
            .create_session(&sample_session())
            .await
            .expect("create session");
        let probes: Vec<ProbeRow> = (0..1000)
            .map(|seq| probe(seq, Some(1.0 + f64::from(seq as u16 % 50))))
            .collect();
        let start = Instant::now();
        db.insert_probes_batch(session_id, &probes)
            .await
            .expect("batch insert");
        let elapsed = start.elapsed();
        eprintln!("batch insert of 1000 probes took {elapsed:?}");
        assert_eq!(db.load_probes(session_id).await.expect("load").len(), 1000);
        assert!(
            elapsed.as_millis() < 500,
            "batch insert took {elapsed:?}, expected < 500 ms"
        );
    }

    #[tokio::test]
    async fn summaries_report_counts_and_loss_percent() {
        let dir = TestDir::new("summary");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let id = db
            .create_session(&sample_session())
            .await
            .expect("create session");
        let probes: Vec<ProbeRow> = (0..10)
            .map(|seq| probe(seq, if seq % 5 == 0 { None } else { Some(12.5) }))
            .collect();
        db.insert_probes_batch(id, &probes).await.expect("insert");
        db.finish_session(id, &now_rfc3339()).await.expect("finish");
        let summaries = db.list_sessions().await.expect("list");
        assert_eq!(summaries.len(), 1);
        let s = &summaries[0];
        assert_eq!(s.probe_count, 10);
        assert_eq!(s.loss_count, 2);
        assert!((s.loss_percent - 20.0).abs() < 1e-9);
        assert!(s.ended_at.is_some());
        assert_eq!(s.target_input, "localhost");
    }

    #[tokio::test]
    async fn delete_session_cascades_probes() {
        let dir = TestDir::new("delete");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let id = db
            .create_session(&sample_session())
            .await
            .expect("create session");
        db.insert_probes_batch(id, &[probe(0, Some(1.0)), probe(1, None)])
            .await
            .expect("insert");
        db.delete_session(id).await.expect("delete");
        assert!(db.list_sessions().await.expect("list").is_empty());
        assert!(db.load_probes(id).await.expect("load").is_empty());
        let row = sqlx::query("SELECT COUNT(*) AS n FROM probes")
            .fetch_one(&db.pool)
            .await
            .expect("count probes");
        assert_eq!(row.try_get::<i64, _>("n").expect("n"), 0);
    }

    #[tokio::test]
    async fn list_traces_derives_hop_count_and_reached_target() {
        let dir = TestDir::new("trace-list");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let trace_id = db
            .create_trace(&sample_trace())
            .await
            .expect("create trace");
        let hops = [
            trace_hop(trace_id, 1, Some("192.0.2.1"), None),
            trace_hop(trace_id, 2, Some("203.0.113.10"), Some("target.example")),
        ];
        db.complete_trace_with_hops(trace_id, &now_rfc3339(), "completed", true, &hops)
            .await
            .expect("complete trace");

        let traces = db.list_traces().await.expect("list traces");
        assert_eq!(traces.len(), 1);
        let trace = &traces[0];
        assert_eq!(trace.hop_count, 2);
        assert!(trace.reached_target);
        assert_eq!(trace.status, "completed");
    }

    #[tokio::test]
    async fn delete_trace_cascades_hops() {
        let dir = TestDir::new("trace-delete");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let trace_id = db
            .create_trace(&sample_trace())
            .await
            .expect("create trace");
        let hops = [trace_hop(trace_id, 1, Some("192.0.2.1"), None)];
        db.complete_trace_with_hops(trace_id, &now_rfc3339(), "completed", false, &hops)
            .await
            .expect("complete trace");

        db.delete_trace(trace_id).await.expect("delete trace");

        assert!(db.list_traces().await.expect("list traces").is_empty());
        assert!(db
            .load_trace_hops(trace_id)
            .await
            .expect("load hops")
            .is_empty());
        let row = sqlx::query("SELECT COUNT(*) AS n FROM trace_hops WHERE trace_id = ?")
            .bind(trace_id)
            .fetch_one(&db.pool)
            .await
            .expect("count hops");
        assert_eq!(row.try_get::<i64, _>("n").expect("n"), 0);
    }

    #[tokio::test]
    async fn complete_trace_with_hops_rolls_back_on_bad_hop() {
        let dir = TestDir::new("trace-rollback");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let trace_id = db
            .create_trace(&sample_trace())
            .await
            .expect("create trace");
        let hops = [
            trace_hop(trace_id, 1, Some("192.0.2.1"), None),
            trace_hop(trace_id + 1, 2, Some("203.0.113.10"), None),
        ];

        let result = db
            .complete_trace_with_hops(trace_id, &now_rfc3339(), "completed", true, &hops)
            .await;
        assert!(result.is_err(), "bad hop should fail the transaction");

        let traces = db.list_traces().await.expect("list traces");
        assert_eq!(traces.len(), 1);
        let trace = &traces[0];
        assert_eq!(trace.status, "running");
        assert!(trace.ended_at.is_none());
        assert_eq!(trace.hop_count, 0);
        assert!(!trace.reached_target);
        assert!(db
            .load_trace_hops(trace_id)
            .await
            .expect("load hops")
            .is_empty());
    }

    #[tokio::test]
    async fn update_trace_hop_hostname_sets_matching_hop_only() {
        let dir = TestDir::new("trace-hostname");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let trace_id = db
            .create_trace(&sample_trace())
            .await
            .expect("create trace");
        let hops = [
            trace_hop(trace_id, 1, Some("192.0.2.1"), None),
            trace_hop(trace_id, 2, Some("198.51.100.1"), None),
        ];
        db.complete_trace_with_hops(trace_id, &now_rfc3339(), "completed", false, &hops)
            .await
            .expect("complete trace");

        db.update_trace_hop_hostname(trace_id, 2, Some("router.example"))
            .await
            .expect("update hostname");

        let loaded = db.load_trace_hops(trace_id).await.expect("load hops");
        assert_eq!(loaded[0].hostname.as_deref(), None);
        assert_eq!(loaded[1].hostname.as_deref(), Some("router.example"));
    }

    #[tokio::test]
    async fn tampered_migration_fails_with_typed_error() {
        let dir = TestDir::new("tamper");
        let path = dir.db_file();
        let db = Database::connect(&path).await.expect("first connect");
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 1")
            .bind(b"tampered".to_vec())
            .execute(&db.pool)
            .await
            .expect("corrupt checksum");
        drop(db);
        match Database::connect(&path).await {
            Ok(_) => panic!("tampered migration must fail"),
            Err(DbError::Migrate(_)) => {}
            Err(e) => panic!("expected DbError::Migrate, got: {e}"),
        }
    }

    #[test]
    fn rfc3339_formats_known_instants() {
        assert_eq!(system_time_to_rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            system_time_to_rfc3339(UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)),
            "2023-11-14T22:13:20Z"
        );
    }
}
