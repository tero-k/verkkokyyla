//! SQLite persistence layer (schema v1-v3: `sessions` + `probes` + `traces`).
//!
//! Owns the database file lifecycle and all SQL. Callers pass timestamps as
//! RFC 3339 strings (`now_rfc3339` / `system_time_to_rfc3339` are provided so
//! no chrono/time dependency is needed). This module is independent of the
//! stats/engine layers; the session layer (todo 7) maps domain types onto
//! [`NewSession`] / [`ProbeRow`].

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rand::RngCore;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Row;
use thiserror::Error;

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
#[derive(Debug, Error)]
pub enum DbError {
    /// Filesystem failure while preparing the database file location.
    #[error("database file error: {0}")]
    Io(std::io::Error),
    /// Failure from a SQL statement or pool operation.
    #[error("database query error: {0}")]
    Sqlx(sqlx::Error),
    /// Migration failure (includes checksum mismatch on tampered DBs).
    #[error("database migration error: {0}")]
    Migrate(sqlx::migrate::MigrateError),
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

/// Parameters for starting a persisted LAN scan.
#[derive(Debug, Clone)]
pub struct NewScan {
    pub interface_name: String,
    pub cidr: String,
    pub tcp_fallback: bool,
    /// RFC 3339 scan start timestamp.
    pub started_at: String,
}

/// Scan row joined with persisted host count.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanSummary {
    pub id: i64,
    pub interface_name: String,
    pub cidr: String,
    pub tcp_fallback: bool,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub host_count: i64,
}

/// Host row stored for a LAN scan.
#[derive(Debug, Clone, PartialEq)]
pub struct ScanHostRow {
    pub scan_id: i64,
    pub ip: String,
    pub mac: Option<String>,
    pub vendor: Option<String>,
    pub hostname: Option<String>,
    pub found_by: String,
    pub open_ports: String,
    /// RFC 3339 timestamp of discovery.
    pub at: String,
}

/// Parameters for starting a persisted DNS benchmark or diagnostics run.
#[derive(Debug, Clone)]
pub struct NewDnsRun {
    pub target_input: String,
    pub kind: String,
    pub config_json: String,
    /// RFC 3339 DNS run start timestamp.
    pub started_at: String,
}

/// DNS run row joined with persisted target count.
#[derive(Debug, Clone, PartialEq)]
pub struct DnsRunSummary {
    pub id: i64,
    pub target_input: String,
    pub kind: String,
    pub config_json: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub target_count: i64,
}

/// Target aggregate row stored for a DNS benchmark or diagnostics run.
#[derive(Debug, Clone, PartialEq)]
pub struct DnsRunTargetRow {
    pub run_id: i64,
    pub target: String,
    pub protocol: Option<String>,
    pub metrics_json: String,
}

/// Loaded DNS run and ordered target aggregate rows.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedDnsRun {
    pub run: DnsRunSummary,
    pub targets: Vec<DnsRunTargetRow>,
}

/// Parameters for starting a persisted download speed test session.
#[derive(Debug, Clone)]
pub struct NewDownloadSpeedSession {
    pub url: String,
    pub mode: String,
    pub http_settings_json: String,
    pub started_at: String,
}

/// Download speed session row with summary metrics.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadSpeedSessionSummary {
    pub id: i64,
    pub url: String,
    pub mode: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub average_mbps: f64,
    pub total_time_ms: i64,
}

/// Loaded download speed session including the full result JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedDownloadSpeedSession {
    pub session: DownloadSpeedSessionSummary,
    pub result_json: String,
}

/// Parameters for a Mikrotik connection profile. Passwords live in the OS keyring.
#[derive(Debug, Clone)]
pub struct NewMikrotikProfile {
    pub name: String,
    pub host: String,
    pub port: i64,
    pub use_tls: bool,
    pub allow_invalid_certs: bool,
    pub username: String,
    pub created_at: String,
}

/// Persisted Mikrotik connection profile. `secret_key` is the keyring account name.
#[derive(Debug, Clone, PartialEq)]
pub struct MikrotikProfile {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: i64,
    pub use_tls: bool,
    pub allow_invalid_certs: bool,
    pub username: String,
    pub secret_key: String,
    pub created_at: String,
}

/// Parameters for a Mikrotik monitoring session.
#[derive(Debug, Clone)]
pub struct NewMikrotikSession {
    pub profile_id: i64,
    pub started_at: String,
    pub status: String,
}

/// Version and firmware metadata attached after the Mikrotik version flow completes.
#[derive(Debug, Clone)]
pub struct MikrotikSessionVersionStatus {
    pub board_name: Option<String>,
    pub routeros_version: Option<String>,
    pub architecture_name: Option<String>,
    pub update_status_json: Option<String>,
    pub firmware_status_json: Option<String>,
}

/// Mikrotik monitoring session row joined with persisted snapshot count.
#[derive(Debug, Clone, PartialEq)]
pub struct MikrotikSessionSummary {
    pub id: i64,
    pub profile_id: i64,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub board_name: Option<String>,
    pub routeros_version: Option<String>,
    pub architecture_name: Option<String>,
    pub update_status_json: Option<String>,
    pub firmware_status_json: Option<String>,
    pub snapshot_count: i64,
}

/// Snapshot row for one Mikrotik monitoring tick.
#[derive(Debug, Clone, PartialEq)]
pub struct MikrotikSnapshotRow {
    pub id: i64,
    pub session_id: i64,
    pub at: String,
    pub cpu_load: Option<f64>,
    pub mem_used_bytes: Option<i64>,
    pub mem_total_bytes: Option<i64>,
    pub uptime: Option<String>,
    pub warning: Option<String>,
    pub sensors_json: Option<String>,
    pub interfaces_json: Option<String>,
    pub vlans_json: Option<String>,
    pub bridge_vlans_json: Option<String>,
}

/// Loaded Mikrotik session and snapshots ordered by timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedMikrotikSession {
    pub session: MikrotikSessionSummary,
    pub snapshots: Vec<MikrotikSnapshotRow>,
}

/// Loaded scan and ordered host rows.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedScan {
    pub scan: ScanSummary,
    pub hosts: Vec<ScanHostRow>,
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

    /// Inserts a scan row and returns its id.
    pub async fn create_scan(&self, scan: &NewScan) -> Result<i64, DbError> {
        let result = sqlx::query(
            "INSERT INTO scans (interface_name, cidr, tcp_fallback, started_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(&scan.interface_name)
        .bind(&scan.cidr)
        .bind(scan.tcp_fallback)
        .bind(&scan.started_at)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Inserts a DNS run row and returns its id.
    pub async fn create_dns_run(&self, run: &NewDnsRun) -> Result<i64, DbError> {
        let result = sqlx::query(
            "INSERT INTO dns_runs (target_input, kind, config_json, started_at) \
             VALUES (?, ?, ?, ?)",
        )
        .bind(&run.target_input)
        .bind(&run.kind)
        .bind(&run.config_json)
        .bind(&run.started_at)
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

    /// Finalizes a DNS run and writes target aggregate rows in one transaction.
    pub async fn finish_dns_run(
        &self,
        id: i64,
        ended_at: &str,
        status: &str,
        targets: &[DnsRunTargetRow],
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("UPDATE dns_runs SET ended_at = ?, status = ? WHERE id = ?")
            .bind(ended_at)
            .bind(status)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        for target in targets {
            sqlx::query(
                "INSERT INTO dns_run_targets (run_id, target, protocol, metrics_json) \
                 VALUES (?, ?, ?, ?)",
            )
            .bind(target.run_id)
            .bind(&target.target)
            .bind(&target.protocol)
            .bind(&target.metrics_json)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Inserts a batch of scan host rows in one transaction.
    pub async fn insert_scan_hosts_batch(
        &self,
        scan_id: i64,
        hosts: &[ScanHostRow],
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await?;
        for host in hosts {
            sqlx::query(
                "INSERT INTO scan_hosts (scan_id, ip, mac, vendor, hostname, found_by, open_ports, at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(scan_id)
            .bind(&host.ip)
            .bind(&host.mac)
            .bind(&host.vendor)
            .bind(&host.hostname)
            .bind(&host.found_by)
            .bind(&host.open_ports)
            .bind(&host.at)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Finalizes a scan and stores its terminal status.
    pub async fn finish_scan(&self, id: i64, ended_at: &str, status: &str) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE scans SET ended_at = ?, status = ?, \
             host_count = (SELECT COUNT(*) FROM scan_hosts WHERE scan_id = ?) WHERE id = ?",
        )
        .bind(ended_at)
        .bind(status)
        .bind(id)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lists all scans newest first.
    pub async fn list_scans(&self) -> Result<Vec<ScanSummary>, DbError> {
        let rows = sqlx::query(
            "SELECT id, interface_name, cidr, tcp_fallback, started_at, ended_at, status, \
                    COALESCE((SELECT COUNT(*) FROM scan_hosts h WHERE h.scan_id = scans.id), host_count) AS host_count \
             FROM scans ORDER BY id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| scan_summary_from_row(row).map_err(DbError::from))
            .collect()
    }

    /// Loads one scan summary with its hosts ordered by IP text.
    pub async fn load_scan(&self, scan_id: i64) -> Result<LoadedScan, DbError> {
        let scan = self
            .list_scans()
            .await?
            .into_iter()
            .find(|scan| scan.id == scan_id)
            .unwrap_or_else(|| ScanSummary {
                id: scan_id,
                interface_name: String::new(),
                cidr: String::new(),
                tcp_fallback: false,
                started_at: String::new(),
                ended_at: None,
                status: String::new(),
                host_count: 0,
            });
        let hosts = self.load_scan_hosts(scan_id).await?;
        Ok(LoadedScan { scan, hosts })
    }

    /// Loads all host rows of a scan ordered by IP text.
    pub async fn load_scan_hosts(&self, scan_id: i64) -> Result<Vec<ScanHostRow>, DbError> {
        let rows = sqlx::query(
            "SELECT scan_id, ip, mac, vendor, hostname, found_by, open_ports, at \
             FROM scan_hosts WHERE scan_id = ? ORDER BY ip",
        )
        .bind(scan_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(ScanHostRow {
                    scan_id: row.try_get("scan_id")?,
                    ip: row.try_get("ip")?,
                    mac: row.try_get("mac")?,
                    vendor: row.try_get("vendor")?,
                    hostname: row.try_get("hostname")?,
                    found_by: row.try_get("found_by")?,
                    open_ports: row.try_get("open_ports")?,
                    at: row.try_get("at")?,
                })
            })
            .collect()
    }

    /// Deletes a scan; its hosts are removed via ON DELETE CASCADE.
    pub async fn delete_scan(&self, id: i64) -> Result<(), DbError> {
        sqlx::query("DELETE FROM scans WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Lists all DNS runs newest first.
    pub async fn list_dns_runs(&self) -> Result<Vec<DnsRunSummary>, DbError> {
        let rows = sqlx::query(
            "SELECT r.id, r.target_input, r.kind, r.config_json, r.started_at, r.ended_at, r.status, \
                    COALESCE((SELECT COUNT(*) FROM dns_run_targets t WHERE t.run_id = r.id), 0) AS target_count \
             FROM dns_runs r ORDER BY r.id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| dns_run_summary_from_row(row).map_err(DbError::from))
            .collect()
    }

    /// Loads one DNS run summary with its target aggregate rows ordered by id.
    pub async fn load_dns_run(&self, run_id: i64) -> Result<LoadedDnsRun, DbError> {
        let run = self
            .list_dns_runs()
            .await?
            .into_iter()
            .find(|run| run.id == run_id)
            .unwrap_or_else(|| DnsRunSummary {
                id: run_id,
                target_input: String::new(),
                kind: String::new(),
                config_json: String::new(),
                started_at: String::new(),
                ended_at: None,
                status: String::new(),
                target_count: 0,
            });
        let targets = self.load_dns_run_targets(run_id).await?;
        Ok(LoadedDnsRun { run, targets })
    }

    /// Loads all target aggregate rows for a DNS run ordered by insertion id.
    pub async fn load_dns_run_targets(&self, run_id: i64) -> Result<Vec<DnsRunTargetRow>, DbError> {
        let rows = sqlx::query(
            "SELECT run_id, target, protocol, metrics_json \
             FROM dns_run_targets WHERE run_id = ? ORDER BY id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(DnsRunTargetRow {
                    run_id: row.try_get("run_id")?,
                    target: row.try_get("target")?,
                    protocol: row.try_get("protocol")?,
                    metrics_json: row.try_get("metrics_json")?,
                })
            })
            .collect()
    }

    /// Deletes a DNS run; its target rows are removed via ON DELETE CASCADE.
    pub async fn delete_dns_run(&self, id: i64) -> Result<(), DbError> {
        sqlx::query("DELETE FROM dns_runs WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Inserts a download speed session row and returns its id.
    pub async fn create_download_speed_session(
        &self,
        session: &NewDownloadSpeedSession,
    ) -> Result<i64, DbError> {
        let result = sqlx::query(
            "INSERT INTO download_speed_sessions \
             (url, mode, http_settings_json, started_at, status, result_json) \
             VALUES (?, ?, ?, ?, 'running', '{}')",
        )
        .bind(&session.url)
        .bind(&session.mode)
        .bind(&session.http_settings_json)
        .bind(&session.started_at)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Finalizes a download speed session with its result.
    pub async fn finish_download_speed_session(
        &self,
        id: i64,
        ended_at: &str,
        status: &str,
        average_mbps: f64,
        total_time_ms: i64,
        result_json: &str,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE download_speed_sessions \
             SET ended_at = ?, status = ?, average_mbps = ?, total_time_ms = ?, result_json = ? \
             WHERE id = ?",
        )
        .bind(ended_at)
        .bind(status)
        .bind(average_mbps)
        .bind(total_time_ms)
        .bind(result_json)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lists all download speed sessions newest first.
    pub async fn list_download_speed_sessions(
        &self,
    ) -> Result<Vec<DownloadSpeedSessionSummary>, DbError> {
        let rows = sqlx::query(
            "SELECT id, url, mode, started_at, ended_at, status, average_mbps, total_time_ms \
             FROM download_speed_sessions ORDER BY id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| {
                Ok(DownloadSpeedSessionSummary {
                    id: row.try_get("id")?,
                    url: row.try_get("url")?,
                    mode: row.try_get("mode")?,
                    started_at: row.try_get("started_at")?,
                    ended_at: row.try_get("ended_at")?,
                    status: row.try_get("status")?,
                    average_mbps: row.try_get("average_mbps")?,
                    total_time_ms: row.try_get("total_time_ms")?,
                })
            })
            .collect()
    }

    /// Loads one download speed session including its stored result JSON.
    pub async fn load_download_speed_session(
        &self,
        id: i64,
    ) -> Result<LoadedDownloadSpeedSession, DbError> {
        let session = self
            .list_download_speed_sessions()
            .await?
            .into_iter()
            .find(|session| session.id == id)
            .unwrap_or_else(|| DownloadSpeedSessionSummary {
                id,
                url: String::new(),
                mode: String::new(),
                started_at: String::new(),
                ended_at: None,
                status: String::new(),
                average_mbps: 0.0,
                total_time_ms: 0,
            });
        let result_json: String =
            sqlx::query_scalar("SELECT result_json FROM download_speed_sessions WHERE id = ?")
                .bind(id)
                .fetch_one(&self.pool)
                .await?;
        Ok(LoadedDownloadSpeedSession {
            session,
            result_json,
        })
    }

    /// Deletes a download speed session.
    pub async fn delete_download_speed_session(&self, id: i64) -> Result<(), DbError> {
        sqlx::query("DELETE FROM download_speed_sessions WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Inserts a Mikrotik profile and returns the persisted row.
    pub async fn create_mikrotik_profile(
        &self,
        profile: &NewMikrotikProfile,
    ) -> Result<MikrotikProfile, DbError> {
        let secret_key = new_mikrotik_secret_key();
        let result = sqlx::query(
            "INSERT INTO mikrotik_profiles \
             (name, host, port, use_tls, allow_invalid_certs, username, secret_key, created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&profile.name)
        .bind(&profile.host)
        .bind(profile.port)
        .bind(profile.use_tls)
        .bind(profile.allow_invalid_certs)
        .bind(&profile.username)
        .bind(&secret_key)
        .bind(&profile.created_at)
        .execute(&self.pool)
        .await?;
        Ok(MikrotikProfile {
            id: result.last_insert_rowid(),
            name: profile.name.clone(),
            host: profile.host.clone(),
            port: profile.port,
            use_tls: profile.use_tls,
            allow_invalid_certs: profile.allow_invalid_certs,
            username: profile.username.clone(),
            secret_key,
            created_at: profile.created_at.clone(),
        })
    }

    /// Updates editable Mikrotik profile fields; the secret key is immutable.
    pub async fn update_mikrotik_profile(
        &self,
        id: i64,
        profile: &NewMikrotikProfile,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE mikrotik_profiles \
             SET name = ?, host = ?, port = ?, use_tls = ?, allow_invalid_certs = ?, username = ? \
             WHERE id = ?",
        )
        .bind(&profile.name)
        .bind(&profile.host)
        .bind(profile.port)
        .bind(profile.use_tls)
        .bind(profile.allow_invalid_certs)
        .bind(&profile.username)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Lists all Mikrotik profiles newest first.
    pub async fn list_mikrotik_profiles(&self) -> Result<Vec<MikrotikProfile>, DbError> {
        let rows = sqlx::query(
            "SELECT id, name, host, port, use_tls, allow_invalid_certs, username, secret_key, created_at \
             FROM mikrotik_profiles ORDER BY id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| mikrotik_profile_from_row(row).map_err(DbError::from))
            .collect()
    }

    /// Loads one Mikrotik profile by id.
    pub async fn load_mikrotik_profile(&self, id: i64) -> Result<Option<MikrotikProfile>, DbError> {
        let row = sqlx::query(
            "SELECT id, name, host, port, use_tls, allow_invalid_certs, username, secret_key, created_at \
             FROM mikrotik_profiles WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.as_ref()
            .map(mikrotik_profile_from_row)
            .transpose()
            .map_err(DbError::from)
    }

    /// Deletes a Mikrotik profile; sessions and snapshots cascade.
    pub async fn delete_mikrotik_profile(&self, id: i64) -> Result<(), DbError> {
        sqlx::query("DELETE FROM mikrotik_profiles WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Inserts a Mikrotik monitoring session and returns its id.
    pub async fn create_mikrotik_session(
        &self,
        session: &NewMikrotikSession,
    ) -> Result<i64, DbError> {
        let result = sqlx::query(
            "INSERT INTO mikrotik_sessions (profile_id, started_at, status) VALUES (?, ?, ?)",
        )
        .bind(session.profile_id)
        .bind(&session.started_at)
        .bind(&session.status)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Finalizes a Mikrotik monitoring session.
    pub async fn complete_mikrotik_session(
        &self,
        id: i64,
        ended_at: &str,
        status: &str,
    ) -> Result<(), DbError> {
        sqlx::query("UPDATE mikrotik_sessions SET ended_at = ?, status = ? WHERE id = ?")
            .bind(ended_at)
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Stores Mikrotik version/update/firmware metadata on a session.
    pub async fn set_mikrotik_session_version_status(
        &self,
        id: i64,
        status: &MikrotikSessionVersionStatus,
    ) -> Result<(), DbError> {
        sqlx::query(
            "UPDATE mikrotik_sessions \
             SET board_name = ?, routeros_version = ?, architecture_name = ?, \
                 update_status_json = ?, firmware_status_json = ? \
             WHERE id = ?",
        )
        .bind(&status.board_name)
        .bind(&status.routeros_version)
        .bind(&status.architecture_name)
        .bind(&status.update_status_json)
        .bind(&status.firmware_status_json)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Inserts a Mikrotik snapshot and returns its id.
    pub async fn insert_mikrotik_snapshot(
        &self,
        snapshot: &MikrotikSnapshotRow,
    ) -> Result<i64, DbError> {
        let result = sqlx::query(
            "INSERT INTO mikrotik_snapshots \
             (session_id, at, cpu_load, mem_used_bytes, mem_total_bytes, uptime, warning, \
              sensors_json, interfaces_json, vlans_json, bridge_vlans_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(snapshot.session_id)
        .bind(&snapshot.at)
        .bind(snapshot.cpu_load)
        .bind(snapshot.mem_used_bytes)
        .bind(snapshot.mem_total_bytes)
        .bind(&snapshot.uptime)
        .bind(&snapshot.warning)
        .bind(&snapshot.sensors_json)
        .bind(&snapshot.interfaces_json)
        .bind(&snapshot.vlans_json)
        .bind(&snapshot.bridge_vlans_json)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    /// Lists all Mikrotik sessions newest first.
    pub async fn list_mikrotik_sessions(&self) -> Result<Vec<MikrotikSessionSummary>, DbError> {
        let rows = sqlx::query(
            "SELECT s.id, s.profile_id, s.started_at, s.ended_at, s.status, s.board_name, \
                    s.routeros_version, s.architecture_name, s.update_status_json, \
                    s.firmware_status_json, \
                    COALESCE((SELECT COUNT(*) FROM mikrotik_snapshots m WHERE m.session_id = s.id), 0) AS snapshot_count \
             FROM mikrotik_sessions s ORDER BY s.id DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| mikrotik_session_summary_from_row(row).map_err(DbError::from))
            .collect()
    }

    /// Loads one Mikrotik session with snapshots ordered by timestamp ascending.
    pub async fn load_mikrotik_session(&self, id: i64) -> Result<LoadedMikrotikSession, DbError> {
        let session = self
            .list_mikrotik_sessions()
            .await?
            .into_iter()
            .find(|session| session.id == id)
            .unwrap_or_else(|| MikrotikSessionSummary {
                id,
                profile_id: 0,
                started_at: String::new(),
                ended_at: None,
                status: String::new(),
                board_name: None,
                routeros_version: None,
                architecture_name: None,
                update_status_json: None,
                firmware_status_json: None,
                snapshot_count: 0,
            });
        let snapshots = self.load_mikrotik_snapshots(id).await?;
        Ok(LoadedMikrotikSession { session, snapshots })
    }

    /// Loads Mikrotik snapshots for a session ordered by timestamp ascending.
    pub async fn load_mikrotik_snapshots(
        &self,
        session_id: i64,
    ) -> Result<Vec<MikrotikSnapshotRow>, DbError> {
        let rows = sqlx::query(
            "SELECT id, session_id, at, cpu_load, mem_used_bytes, mem_total_bytes, uptime, warning, \
                    sensors_json, interfaces_json, vlans_json, bridge_vlans_json \
             FROM mikrotik_snapshots WHERE session_id = ? ORDER BY at ASC",
        )
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        rows.iter()
            .map(|row| mikrotik_snapshot_from_row(row).map_err(DbError::from))
            .collect()
    }

    /// Deletes a Mikrotik monitoring session; snapshots cascade.
    pub async fn delete_mikrotik_session(&self, id: i64) -> Result<(), DbError> {
        sqlx::query("DELETE FROM mikrotik_sessions WHERE id = ?")
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

fn scan_summary_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<ScanSummary, sqlx::Error> {
    Ok(ScanSummary {
        id: row.try_get("id")?,
        interface_name: row.try_get("interface_name")?,
        cidr: row.try_get("cidr")?,
        tcp_fallback: row.try_get("tcp_fallback")?,
        started_at: row.try_get("started_at")?,
        ended_at: row.try_get("ended_at")?,
        status: row.try_get("status")?,
        host_count: row.try_get("host_count")?,
    })
}

fn dns_run_summary_from_row(row: &sqlx::sqlite::SqliteRow) -> Result<DnsRunSummary, sqlx::Error> {
    Ok(DnsRunSummary {
        id: row.try_get("id")?,
        target_input: row.try_get("target_input")?,
        kind: row.try_get("kind")?,
        config_json: row.try_get("config_json")?,
        started_at: row.try_get("started_at")?,
        ended_at: row.try_get("ended_at")?,
        status: row.try_get("status")?,
        target_count: row.try_get("target_count")?,
    })
}

fn mikrotik_profile_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<MikrotikProfile, sqlx::Error> {
    Ok(MikrotikProfile {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        host: row.try_get("host")?,
        port: row.try_get("port")?,
        use_tls: row.try_get("use_tls")?,
        allow_invalid_certs: row.try_get("allow_invalid_certs")?,
        username: row.try_get("username")?,
        secret_key: row.try_get("secret_key")?,
        created_at: row.try_get("created_at")?,
    })
}

fn mikrotik_session_summary_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<MikrotikSessionSummary, sqlx::Error> {
    Ok(MikrotikSessionSummary {
        id: row.try_get("id")?,
        profile_id: row.try_get("profile_id")?,
        started_at: row.try_get("started_at")?,
        ended_at: row.try_get("ended_at")?,
        status: row.try_get("status")?,
        board_name: row.try_get("board_name")?,
        routeros_version: row.try_get("routeros_version")?,
        architecture_name: row.try_get("architecture_name")?,
        update_status_json: row.try_get("update_status_json")?,
        firmware_status_json: row.try_get("firmware_status_json")?,
        snapshot_count: row.try_get("snapshot_count")?,
    })
}

fn mikrotik_snapshot_from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<MikrotikSnapshotRow, sqlx::Error> {
    Ok(MikrotikSnapshotRow {
        id: row.try_get("id")?,
        session_id: row.try_get("session_id")?,
        at: row.try_get("at")?,
        cpu_load: row.try_get("cpu_load")?,
        mem_used_bytes: row.try_get("mem_used_bytes")?,
        mem_total_bytes: row.try_get("mem_total_bytes")?,
        uptime: row.try_get("uptime")?,
        warning: row.try_get("warning")?,
        sensors_json: row.try_get("sensors_json")?,
        interfaces_json: row.try_get("interfaces_json")?,
        vlans_json: row.try_get("vlans_json")?,
        bridge_vlans_json: row.try_get("bridge_vlans_json")?,
    })
}

fn new_mikrotik_secret_key() -> String {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let mut key = String::with_capacity(32);
    for byte in bytes {
        key.push_str(&format!("{byte:02x}"));
    }
    key
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

    fn sample_dns_run() -> NewDnsRun {
        NewDnsRun {
            target_input: "example.com benchmark".to_string(),
            kind: "benchmark".to_string(),
            config_json: r#"{"profile":"everyday"}"#.to_string(),
            started_at: now_rfc3339(),
        }
    }

    fn dns_target(run_id: i64, target: &str, protocol: Option<&str>) -> DnsRunTargetRow {
        DnsRunTargetRow {
            run_id,
            target: target.to_string(),
            protocol: protocol.map(std::string::ToString::to_string),
            metrics_json: r#"{"medianMs":12.5}"#.to_string(),
        }
    }

    fn sample_mikrotik_profile(name: &str) -> NewMikrotikProfile {
        NewMikrotikProfile {
            name: name.to_string(),
            host: "router.lan".to_string(),
            port: 8729,
            use_tls: true,
            allow_invalid_certs: false,
            username: "admin".to_string(),
            created_at: now_rfc3339(),
        }
    }

    fn sample_mikrotik_session(profile_id: i64) -> NewMikrotikSession {
        NewMikrotikSession {
            profile_id,
            started_at: now_rfc3339(),
            status: "running".to_string(),
        }
    }

    fn mikrotik_snapshot(session_id: i64, at: &str) -> MikrotikSnapshotRow {
        MikrotikSnapshotRow {
            id: 0,
            session_id,
            at: at.to_string(),
            cpu_load: Some(17.5),
            mem_used_bytes: Some(1_024),
            mem_total_bytes: Some(4_096),
            uptime: Some("1d2h".to_string()),
            warning: Some("fan sensor unavailable".to_string()),
            sensors_json: Some(r#"[{"name":"temp","value":42}]"#.to_string()),
            interfaces_json: Some(r#"[{"name":"ether1","running":true}]"#.to_string()),
            vlans_json: Some(r#"[{"name":"vlan10","id":10}]"#.to_string()),
            bridge_vlans_json: Some(r#"[{"bridge":"br0","tagged":["ether1"]}]"#.to_string()),
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
    async fn fresh_db_migrates_to_v10() {
        let dir = TestDir::new("fresh");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        assert_eq!(migration_count(&db).await, 10);
        let versions = sqlx::query("SELECT version FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&db.pool)
            .await
            .expect("read versions");
        let versions: Vec<i64> = versions
            .iter()
            .map(|row| row.try_get::<i64, _>("version").expect("version"))
            .collect();
        assert_eq!(versions, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);

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
        assert!(tables.contains(&"scans".to_string()));
        assert!(tables.contains(&"scan_hosts".to_string()));
        assert!(tables.contains(&"dns_runs".to_string()));
        assert!(tables.contains(&"dns_run_targets".to_string()));
        assert!(tables.contains(&"download_speed_sessions".to_string()));
        assert!(tables.contains(&"mikrotik_profiles".to_string()));
        assert!(tables.contains(&"mikrotik_sessions".to_string()));
        assert!(tables.contains(&"mikrotik_snapshots".to_string()));

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
        assert!(indexes.contains(&"idx_scan_hosts_scan".to_string()));
        assert!(indexes.contains(&"idx_dns_run_targets_run".to_string()));
        assert!(indexes.contains(&"idx_mikrotik_snapshots_session_at".to_string()));
    }

    #[tokio::test]
    async fn second_open_is_noop() {
        let dir = TestDir::new("reopen");
        let path = dir.db_file();
        let db = Database::connect(&path).await.expect("first connect");
        assert_eq!(migration_count(&db).await, 10);
        drop(db);
        let db = Database::connect(&path).await.expect("second connect");
        assert_eq!(migration_count(&db).await, 10);
    }

    #[tokio::test]
    async fn mikrotik_db_fresh_migrates() {
        let dir = TestDir::new("mikrotik-fresh");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        assert_eq!(migration_count(&db).await, 10);

        let tables = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table'")
            .fetch_all(&db.pool)
            .await
            .expect("read tables");
        let tables: Vec<String> = tables
            .iter()
            .map(|row| row.try_get::<String, _>("name").expect("table name"))
            .collect();
        assert!(tables.contains(&"mikrotik_profiles".to_string()));
        assert!(tables.contains(&"mikrotik_sessions".to_string()));
        assert!(tables.contains(&"mikrotik_snapshots".to_string()));

        let indexes = sqlx::query("SELECT name FROM sqlite_master WHERE type = 'index'")
            .fetch_all(&db.pool)
            .await
            .expect("read indexes");
        let indexes: Vec<String> = indexes
            .iter()
            .map(|row| row.try_get::<String, _>("name").expect("index name"))
            .collect();
        assert!(indexes.contains(&"idx_mikrotik_snapshots_session_at".to_string()));
    }

    #[tokio::test]
    async fn mikrotik_db_secret_key_populated_and_unique_on_create() {
        let dir = TestDir::new("mikrotik-secret");
        let db = Database::connect(&dir.db_file()).await.expect("connect");

        let first = db
            .create_mikrotik_profile(&sample_mikrotik_profile("edge"))
            .await
            .expect("create first profile");
        let second = db
            .create_mikrotik_profile(&sample_mikrotik_profile("core"))
            .await
            .expect("create second profile");

        assert_eq!(first.secret_key.len(), 32);
        assert!(first.secret_key.chars().all(|ch| ch.is_ascii_hexdigit()));
        assert_ne!(first.secret_key, second.secret_key);
    }

    #[tokio::test]
    async fn mikrotik_db_profile_crud() {
        let dir = TestDir::new("mikrotik-profile-crud");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let profile = db
            .create_mikrotik_profile(&sample_mikrotik_profile("edge"))
            .await
            .expect("create profile");
        assert_eq!(profile.name, "edge");
        assert_eq!(profile.host, "router.lan");

        let mut update = sample_mikrotik_profile("edge-renamed");
        update.host = "192.0.2.1".to_string();
        update.port = 8728;
        update.use_tls = false;
        update.allow_invalid_certs = true;
        update.username = "ops".to_string();
        db.update_mikrotik_profile(profile.id, &update)
            .await
            .expect("update profile");

        let loaded = db
            .load_mikrotik_profile(profile.id)
            .await
            .expect("load profile")
            .expect("profile exists");
        assert_eq!(loaded.name, "edge-renamed");
        assert_eq!(loaded.host, "192.0.2.1");
        assert_eq!(loaded.port, 8728);
        assert!(!loaded.use_tls);
        assert!(loaded.allow_invalid_certs);
        assert_eq!(loaded.username, "ops");
        assert_eq!(loaded.secret_key, profile.secret_key);
        assert_eq!(
            db.list_mikrotik_profiles().await.expect("list"),
            vec![loaded]
        );

        db.delete_mikrotik_profile(profile.id)
            .await
            .expect("delete profile");
        assert!(db
            .load_mikrotik_profile(profile.id)
            .await
            .expect("load after delete")
            .is_none());
    }

    #[tokio::test]
    async fn mikrotik_db_session_snapshot_roundtrip_includes_json_columns() {
        let dir = TestDir::new("mikrotik-roundtrip");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let profile = db
            .create_mikrotik_profile(&sample_mikrotik_profile("edge"))
            .await
            .expect("create profile");
        let session_id = db
            .create_mikrotik_session(&sample_mikrotik_session(profile.id))
            .await
            .expect("create session");
        let version_status = MikrotikSessionVersionStatus {
            board_name: Some("CCR2004".to_string()),
            routeros_version: Some("7.16.1".to_string()),
            architecture_name: Some("arm64".to_string()),
            update_status_json: Some(
                r#"{"channel":"stable","status":"System is already up to date"}"#.to_string(),
            ),
            firmware_status_json: Some(r#"{"current":"7.16.1","upgrade":"7.16.1"}"#.to_string()),
        };
        db.set_mikrotik_session_version_status(session_id, &version_status)
            .await
            .expect("set version status");
        let mut snapshot = mikrotik_snapshot(session_id, "2026-01-01T00:00:02Z");
        snapshot.id = db
            .insert_mikrotik_snapshot(&snapshot)
            .await
            .expect("insert snapshot");
        db.complete_mikrotik_session(session_id, "2026-01-01T00:00:03Z", "completed")
            .await
            .expect("complete session");

        let sessions = db.list_mikrotik_sessions().await.expect("list sessions");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].profile_id, profile.id);
        assert_eq!(sessions[0].status, "completed");
        assert_eq!(sessions[0].board_name.as_deref(), Some("CCR2004"));
        assert_eq!(sessions[0].routeros_version.as_deref(), Some("7.16.1"));
        assert_eq!(sessions[0].architecture_name.as_deref(), Some("arm64"));
        assert_eq!(
            sessions[0].update_status_json,
            version_status.update_status_json
        );
        assert_eq!(
            sessions[0].firmware_status_json,
            version_status.firmware_status_json
        );
        assert_eq!(sessions[0].snapshot_count, 1);

        let loaded = db
            .load_mikrotik_session(session_id)
            .await
            .expect("load session");
        assert_eq!(loaded.session, sessions[0]);
        assert_eq!(loaded.snapshots, vec![snapshot]);
    }

    #[tokio::test]
    async fn mikrotik_db_delete_profile_cascades_sessions_and_snapshots() {
        let dir = TestDir::new("mikrotik-cascade");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let profile = db
            .create_mikrotik_profile(&sample_mikrotik_profile("edge"))
            .await
            .expect("create profile");
        let session_id = db
            .create_mikrotik_session(&sample_mikrotik_session(profile.id))
            .await
            .expect("create session");
        db.insert_mikrotik_snapshot(&mikrotik_snapshot(session_id, "2026-01-01T00:00:01Z"))
            .await
            .expect("insert snapshot");

        db.delete_mikrotik_profile(profile.id)
            .await
            .expect("delete profile");

        let session_row = sqlx::query("SELECT COUNT(*) AS n FROM mikrotik_sessions")
            .fetch_one(&db.pool)
            .await
            .expect("count sessions");
        let snapshot_row = sqlx::query("SELECT COUNT(*) AS n FROM mikrotik_snapshots")
            .fetch_one(&db.pool)
            .await
            .expect("count snapshots");
        assert_eq!(session_row.try_get::<i64, _>("n").expect("sessions"), 0);
        assert_eq!(snapshot_row.try_get::<i64, _>("n").expect("snapshots"), 0);
    }

    #[tokio::test]
    async fn mikrotik_db_delete_session_cascades_snapshots() {
        let dir = TestDir::new("mikrotik-session-cascade");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let profile = db
            .create_mikrotik_profile(&sample_mikrotik_profile("edge"))
            .await
            .expect("create profile");
        let session_id = db
            .create_mikrotik_session(&sample_mikrotik_session(profile.id))
            .await
            .expect("create session");
        db.insert_mikrotik_snapshot(&mikrotik_snapshot(session_id, "2026-01-01T00:00:01Z"))
            .await
            .expect("insert snapshot");

        db.delete_mikrotik_session(session_id)
            .await
            .expect("delete session");

        let row = sqlx::query("SELECT COUNT(*) AS n FROM mikrotik_snapshots")
            .fetch_one(&db.pool)
            .await
            .expect("count snapshots");
        assert_eq!(row.try_get::<i64, _>("n").expect("snapshots"), 0);
    }

    #[tokio::test]
    async fn mikrotik_db_snapshots_at_asc() {
        let dir = TestDir::new("mikrotik-order");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let profile = db
            .create_mikrotik_profile(&sample_mikrotik_profile("edge"))
            .await
            .expect("create profile");
        let session_id = db
            .create_mikrotik_session(&sample_mikrotik_session(profile.id))
            .await
            .expect("create session");
        for at in [
            "2026-01-01T00:00:03Z",
            "2026-01-01T00:00:01Z",
            "2026-01-01T00:00:02Z",
        ] {
            db.insert_mikrotik_snapshot(&mikrotik_snapshot(session_id, at))
                .await
                .expect("insert snapshot");
        }

        let loaded = db
            .load_mikrotik_snapshots(session_id)
            .await
            .expect("load snapshots");
        let timestamps: Vec<&str> = loaded.iter().map(|snapshot| snapshot.at.as_str()).collect();
        assert_eq!(
            timestamps,
            vec![
                "2026-01-01T00:00:01Z",
                "2026-01-01T00:00:02Z",
                "2026-01-01T00:00:03Z"
            ]
        );
    }

    #[tokio::test]
    async fn dns_run_round_trip_and_delete_cascades_targets() {
        let dir = TestDir::new("dns-round-trip");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let run_id = db
            .create_dns_run(&sample_dns_run())
            .await
            .expect("create dns run");
        let targets = [
            dns_target(run_id, "cloudflare", Some("udp")),
            dns_target(run_id, "google", Some("tcp")),
            dns_target(run_id, "quad9", None),
        ];

        db.finish_dns_run(run_id, &now_rfc3339(), "completed", &targets)
            .await
            .expect("finish dns run");

        let runs = db.list_dns_runs().await.expect("list dns runs");
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.id, run_id);
        assert_eq!(run.target_input, "example.com benchmark");
        assert_eq!(run.kind, "benchmark");
        assert_eq!(run.config_json, r#"{"profile":"everyday"}"#);
        assert_eq!(run.status, "completed");
        assert_eq!(run.target_count, 3);
        assert!(run.ended_at.is_some());

        let loaded = db.load_dns_run(run_id).await.expect("load dns run");
        assert_eq!(loaded.run, *run);
        assert_eq!(loaded.targets, targets);

        db.delete_dns_run(run_id).await.expect("delete dns run");

        assert!(db.list_dns_runs().await.expect("list dns runs").is_empty());
        assert!(db
            .load_dns_run_targets(run_id)
            .await
            .expect("load dns targets")
            .is_empty());
        let row = sqlx::query("SELECT COUNT(*) AS n FROM dns_run_targets WHERE run_id = ?")
            .bind(run_id)
            .fetch_one(&db.pool)
            .await
            .expect("count dns targets");
        assert_eq!(row.try_get::<i64, _>("n").expect("n"), 0);
    }

    #[tokio::test]
    async fn dns_run_accepts_lookup_kind() {
        let dir = TestDir::new("dns-lookup-kind");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let run_id = db
            .create_dns_run(&NewDnsRun {
                target_input: "example.com via cloudflare".to_string(),
                kind: "lookup".to_string(),
                config_json: r#"{"name":"example.com"}"#.to_string(),
                started_at: now_rfc3339(),
            })
            .await
            .expect("create lookup dns run");
        db.finish_dns_run(
            run_id,
            &now_rfc3339(),
            "completed",
            &[dns_target(run_id, "A", Some("udp"))],
        )
        .await
        .expect("finish lookup dns run");

        let runs = db.list_dns_runs().await.expect("list dns runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].kind, "lookup");
        assert_eq!(runs[0].target_count, 1);
    }

    #[tokio::test]
    async fn finish_dns_run_rolls_back_on_bad_target_run_id() {
        let dir = TestDir::new("dns-rollback");
        let db = Database::connect(&dir.db_file()).await.expect("connect");
        let run_id = db
            .create_dns_run(&sample_dns_run())
            .await
            .expect("create dns run");
        let targets = [
            dns_target(run_id, "cloudflare", Some("udp")),
            dns_target(run_id + 1, "google", Some("tcp")),
        ];

        let result = db
            .finish_dns_run(run_id, &now_rfc3339(), "completed", &targets)
            .await;
        assert!(result.is_err(), "bad target should fail the transaction");

        let runs = db.list_dns_runs().await.expect("list dns runs");
        assert_eq!(runs.len(), 1);
        let run = &runs[0];
        assert_eq!(run.status, "running");
        assert!(run.ended_at.is_none());
        assert_eq!(run.target_count, 0);
        assert!(db
            .load_dns_run_targets(run_id)
            .await
            .expect("load dns targets")
            .is_empty());
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
