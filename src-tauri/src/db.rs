//! SQLite persistence layer (schema v1: `sessions` + `probes`).
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
    pub started_at: String,
    pub ended_at: Option<String>,
    pub probe_count: i64,
    pub loss_count: i64,
    pub loss_percent: f64,
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
             (target_input, resolved_ip, family, engine, interval_ms, timeout_ms, started_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&session.target_input)
        .bind(&session.resolved_ip)
        .bind(&session.family)
        .bind(&session.engine)
        .bind(session.interval_ms)
        .bind(session.timeout_ms)
        .bind(&session.started_at)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
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
                    s.interval_ms, s.timeout_ms, s.started_at, s.ended_at, \
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

    async fn migration_count(db: &Database) -> i64 {
        let row = sqlx::query("SELECT COUNT(*) AS n FROM _sqlx_migrations")
            .fetch_one(&db.pool)
            .await
            .expect("count migrations");
        row.try_get("n").expect("read count")
    }

    #[tokio::test]
    async fn fresh_db_migrates_to_v1() {
        let dir = TestDir::new("fresh");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        assert_eq!(migration_count(&db).await, 1);
        let row = sqlx::query("SELECT version FROM _sqlx_migrations")
            .fetch_one(&db.pool)
            .await
            .expect("read version");
        assert_eq!(row.try_get::<i64, _>("version").expect("version"), 1);
    }

    #[tokio::test]
    async fn second_open_is_noop() {
        let dir = TestDir::new("reopen");
        let path = dir.db_file();
        let db = Database::connect(&path).await.expect("first connect");
        assert_eq!(migration_count(&db).await, 1);
        drop(db);
        let db = Database::connect(&path).await.expect("second connect");
        assert_eq!(migration_count(&db).await, 1);
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
        assert_eq!(
            db.load_probes(session_id).await.expect("load").len(),
            1000
        );
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
        db.finish_session(id, &now_rfc3339())
            .await
            .expect("finish");
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
