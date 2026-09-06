use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch, Mutex};
use tokio::task::JoinHandle;

use crate::db::{now_rfc3339, Database, NewScan, ScanHostRow};
use crate::scan::discovery::{discover, DiscoveredHost};
use crate::scan::oui::lookup_vendor;
use crate::scan::ports::{
    probe_ports_with_connector, production_port_connector, PortConnector, COMMON_PORTS,
    DEFAULT_PORT_CONCURRENCY, DEFAULT_PORT_MIN_INTERVAL,
};
use crate::scan::{
    LoadedScanDto, ScanError, ScanEvent, ScanStatusEvent, ScanSummaryDto, StartScanDto,
    StoppedScanDto,
};

const SCAN_CHANNEL_CAPACITY: usize = 256;
const FLUSH_BATCH_SIZE: usize = 50;
const FLUSH_INTERVAL_MS: u64 = 500;
const PORT_CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

pub type ScanStatusSink = Arc<dyn Fn(ScanStatusEvent) + Send + Sync>;
type RunnerFuture = Pin<Box<dyn Future<Output = Result<(), ScanError>> + Send>>;
type RunnerFn = Arc<
    dyn Fn(
            String,
            bool,
            bool,
            watch::Receiver<bool>,
            mpsc::Sender<DiscoveredHost>,
            ScanStatusSink,
        ) -> RunnerFuture
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct ScanRunner {
    inner: RunnerFn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartScanRequest {
    pub interface_name: String,
    pub cidr: String,
    pub tcp_fallback: bool,
    pub ports_enabled: bool,
}

#[derive(Clone)]
pub struct ScanManager {
    db: Arc<Database>,
    runner: ScanRunner,
    inner: Arc<Mutex<ScanInner>>,
}

struct ScanInner {
    active: Option<ActiveScan>,
}

struct ActiveScan {
    scan_id: i64,
    stop_tx: watch::Sender<bool>,
    join_handle: JoinHandle<Result<StoppedScanDto, ScanError>>,
}

impl ScanRunner {
    pub fn production() -> Self {
        Self::with_port_connector(production_port_connector(PORT_CONNECT_TIMEOUT))
    }

    fn with_port_connector(connector: PortConnector) -> Self {
        Self {
            inner: Arc::new(
                move |cidr, tcp_fallback, ports_enabled, mut stop_rx, tx, on_status| {
                    let connector = Arc::clone(&connector);
                    Box::pin(async move {
                        let result = discover(&cidr, tcp_fallback).await?;
                        on_status(ScanStatusEvent::Progress {
                            done: 0,
                            total: u64::try_from(result.total).unwrap_or(u64::MAX),
                        });
                        let mut done = 0u64;
                        for mut host in result.hosts {
                            if *stop_rx.borrow() {
                                break;
                            }
                            if ports_enabled {
                                host.open_ports = probe_ports_with_connector(
                                    host.ip,
                                    &COMMON_PORTS,
                                    Arc::clone(&connector),
                                    DEFAULT_PORT_CONCURRENCY,
                                    DEFAULT_PORT_MIN_INTERVAL,
                                    &mut stop_rx,
                                )
                                .await;
                                if *stop_rx.borrow() {
                                    break;
                                }
                            }
                            if tx.send(host).await.is_err() {
                                break;
                            }
                            done = done.saturating_add(1);
                            on_status(ScanStatusEvent::Progress {
                                done,
                                total: u64::try_from(result.total).unwrap_or(u64::MAX),
                            });
                            tokio::select! {
                                biased;
                                _ = stop_rx.changed() => { if *stop_rx.borrow() { break; } }
                                () = tokio::task::yield_now() => {}
                            }
                        }
                        Ok(())
                    })
                },
            ),
        }
    }

    #[cfg(test)]
    pub fn fake_one_host(ip: &str, mac: &str) -> Self {
        let ip = ip.parse().expect("test ip");
        let mac = mac.to_owned();
        Self {
            inner: Arc::new(move |_cidr, _tcp, _ports_enabled, _stop, tx, _status| {
                let mac = mac.clone();
                Box::pin(async move {
                    tx.send(DiscoveredHost {
                        ip,
                        mac: Some(mac),
                        hostname: Some("router.local".to_owned()),
                        found_by: crate::scan::discovery::ProbeOutcome::Ping,
                        open_ports: Vec::new(),
                    })
                    .await
                    .map_err(|err| {
                        ScanError::Db(crate::db::DbError::Sqlx(sqlx::Error::Protocol(
                            err.to_string(),
                        )))
                    })
                })
            }),
        }
    }

    #[cfg(test)]
    pub fn fake_one_host_with_port_connector(
        ip: &str,
        mac: &str,
        connector: PortConnector,
    ) -> Self {
        let ip = ip.parse().expect("test ip");
        let mac = mac.to_owned();
        Self {
            inner: Arc::new(
                move |_cidr, _tcp, ports_enabled, mut stop_rx, tx, _status| {
                    let mac = mac.clone();
                    let connector = Arc::clone(&connector);
                    Box::pin(async move {
                        let open_ports = if ports_enabled {
                            probe_ports_with_connector(
                                ip,
                                &[(22, "ssh"), (80, "http")],
                                connector,
                                1,
                                Duration::ZERO,
                                &mut stop_rx,
                            )
                            .await
                        } else {
                            Vec::new()
                        };
                        if *stop_rx.borrow() {
                            return Ok(());
                        }
                        tx.send(DiscoveredHost {
                            ip,
                            mac: Some(mac),
                            hostname: Some("router.local".to_owned()),
                            found_by: crate::scan::discovery::ProbeOutcome::Ping,
                            open_ports,
                        })
                        .await
                        .map_err(|err| {
                            ScanError::Db(crate::db::DbError::Sqlx(sqlx::Error::Protocol(
                                err.to_string(),
                            )))
                        })
                    })
                },
            ),
        }
    }
}

impl ScanManager {
    pub fn new(db: Database) -> Self {
        Self::new_with_runner(db, ScanRunner::production())
    }

    pub fn new_with_runner(db: Database, runner: ScanRunner) -> Self {
        Self {
            db: Arc::new(db),
            runner,
            inner: Arc::new(Mutex::new(ScanInner { active: None })),
        }
    }

    pub async fn start<E>(
        &self,
        request: StartScanRequest,
        on_event: E,
        on_status: ScanStatusSink,
    ) -> Result<StartScanDto, ScanError>
    where
        E: Fn(ScanEvent) + Send + Sync + 'static,
    {
        crate::scan::cidr::expand_ipv4_cidr(&request.cidr)?;
        let mut inner = self.inner.lock().await;
        if inner.active.is_some() {
            return Err(ScanError::AlreadyRunning);
        }
        let scan_id = self
            .db
            .create_scan(&NewScan {
                interface_name: request.interface_name.clone(),
                cidr: request.cidr.clone(),
                tcp_fallback: request.tcp_fallback,
                started_at: now_rfc3339(),
            })
            .await?;
        on_status(ScanStatusEvent::Engine {
            engine: "ping+arp".to_owned(),
            tcp_fallback: request.tcp_fallback,
        });
        let (stop_tx, stop_rx) = watch::channel(false);
        let status_stop_rx = stop_rx.clone();
        let (tx, rx) = mpsc::channel(SCAN_CHANNEL_CAPACITY);
        let runner = self.runner.clone();
        let db = Arc::clone(&self.db);
        let manager = self.clone();
        let on_event = Arc::new(on_event);
        let status = Arc::clone(&on_status);
        let runner_cidr = request.cidr.clone();
        let runner_tcp_fallback = request.tcp_fallback;
        let runner_ports_enabled = request.ports_enabled;
        let join_handle = tokio::spawn(async move {
            let producer = tokio::spawn((runner.inner)(
                runner_cidr,
                runner_tcp_fallback,
                runner_ports_enabled,
                stop_rx,
                tx,
                Arc::clone(&status),
            ));
            let consumer = tokio::spawn(consume_hosts(
                rx,
                Arc::clone(&db),
                scan_id,
                on_event,
                Arc::clone(&status),
            ));
            if let Ok(Err(err)) = producer.await {
                status(ScanStatusEvent::Error {
                    message: err.to_string(),
                });
            }
            let cancelled = *status_stop_rx.borrow();
            if let Ok(host_count) = consumer.await {
                let ended_at = now_rfc3339();
                let terminal = if cancelled { "cancelled" } else { "completed" };
                db.finish_scan(scan_id, &ended_at, terminal).await?;
                if cancelled {
                    status(ScanStatusEvent::Stopped {
                        scan_id,
                        host_count,
                    });
                } else {
                    status(ScanStatusEvent::Completed {
                        scan_id,
                        host_count,
                    });
                }
                manager.clear_active(scan_id).await;
                return Ok(StoppedScanDto {
                    scan_id,
                    host_count,
                    ended_at,
                });
            }
            let ended_at = now_rfc3339();
            db.finish_scan(scan_id, &ended_at, "cancelled").await?;
            manager.clear_active(scan_id).await;
            Ok(StoppedScanDto {
                scan_id,
                host_count: 0,
                ended_at,
            })
        });
        inner.active = Some(ActiveScan {
            scan_id,
            stop_tx,
            join_handle,
        });
        Ok(StartScanDto {
            scan_id,
            interface_name: request.interface_name,
            cidr: request.cidr,
            tcp_fallback: request.tcp_fallback,
        })
    }

    pub async fn stop(&self) -> Result<StoppedScanDto, ScanError> {
        let active = self
            .inner
            .lock()
            .await
            .active
            .take()
            .ok_or(ScanError::NoActiveScan)?;
        let _ = active.stop_tx.send(true);
        active.join_handle.await.map_err(|err| {
            ScanError::Db(crate::db::DbError::Sqlx(sqlx::Error::Protocol(format!(
                "scan task panicked: {err}"
            ))))
        })?
    }

    pub async fn list_scans(&self) -> Result<Vec<ScanSummaryDto>, ScanError> {
        Ok(self
            .db
            .list_scans()
            .await?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    pub async fn load_scan(&self, id: i64) -> Result<LoadedScanDto, ScanError> {
        let scan = self
            .db
            .list_scans()
            .await?
            .into_iter()
            .find(|row| row.id == id)
            .ok_or(ScanError::ScanNotFound(id))?;
        let hosts = self
            .db
            .load_scan_hosts(id)
            .await?
            .into_iter()
            .map(Into::into)
            .collect();
        Ok(LoadedScanDto {
            scan: scan.into(),
            hosts,
        })
    }

    pub async fn delete_scan(&self, id: i64) -> Result<(), ScanError> {
        self.db.delete_scan(id).await?;
        Ok(())
    }

    pub(crate) async fn clear_active(&self, scan_id: i64) {
        let mut inner = self.inner.lock().await;
        if inner
            .active
            .as_ref()
            .is_some_and(|active| active.scan_id == scan_id)
        {
            inner.active.take();
        }
    }
}

async fn consume_hosts(
    mut rx: mpsc::Receiver<DiscoveredHost>,
    db: Arc<Database>,
    scan_id: i64,
    on_event: Arc<dyn Fn(ScanEvent) + Send + Sync>,
    on_status: ScanStatusSink,
) -> u64 {
    let mut batch = Vec::with_capacity(FLUSH_BATCH_SIZE);
    let mut count = 0u64;
    let mut ticker = tokio::time::interval(Duration::from_millis(FLUSH_INTERVAL_MS));
    loop {
        tokio::select! {
            maybe = rx.recv() => {
                let Some(host) = maybe else { flush_batch(&db, scan_id, &mut batch, &on_status).await; break; };
                let row = host_row(scan_id, host).await;
                let open_ports = serde_json::from_str(&row.open_ports).unwrap_or_default();
                on_event(ScanEvent::Host { ip: row.ip.clone(), mac: row.mac.clone(), vendor: row.vendor.clone(), hostname: row.hostname.clone(), found_by: row.found_by.clone(), open_ports, at: row.at.clone() });
                batch.push(row);
                count = count.saturating_add(1);
                if batch.len() >= FLUSH_BATCH_SIZE { flush_batch(&db, scan_id, &mut batch, &on_status).await; }
            }
            _ = ticker.tick() => { flush_batch(&db, scan_id, &mut batch, &on_status).await; }
        }
    }
    count
}

async fn host_row(scan_id: i64, host: DiscoveredHost) -> ScanHostRow {
    let vendor = match host.mac.as_deref() {
        Some(mac) => lookup_vendor(mac).await.ok().flatten(),
        None => None,
    };
    ScanHostRow {
        scan_id,
        ip: host.ip.to_string(),
        mac: host.mac,
        vendor,
        hostname: host.hostname,
        found_by: host.found_by.as_str().to_owned(),
        open_ports: serde_json::to_string(&host.open_ports).unwrap_or_else(|_| "[]".to_owned()),
        at: now_rfc3339(),
    }
}

async fn flush_batch(
    db: &Database,
    scan_id: i64,
    batch: &mut Vec<ScanHostRow>,
    on_status: &ScanStatusSink,
) {
    if batch.is_empty() {
        return;
    }
    if let Err(err) = db.insert_scan_hosts_batch(scan_id, batch).await {
        on_status(ScanStatusEvent::Error {
            message: format!("failed to persist scan host batch: {err}"),
        });
    }
    batch.clear();
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use crate::db::Database;
    use crate::scan::ports::{OpenPort, PortConnector};

    use super::super::{ScanEvent, ScanManager, ScanStatusEvent};
    use super::{ScanRunner, StartScanRequest};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "verkkokyyla-scan-manager-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }

        fn db_file(&self) -> PathBuf {
            self.0.join("scan.db")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn db(path: &Path) -> Database {
        Database::connect(path).await.expect("db")
    }

    fn port_connector(open_port: u16) -> PortConnector {
        Arc::new(move |_ip, port| Box::pin(async move { port == open_port }))
    }

    #[tokio::test]
    async fn manager_starts_persists_lists_loads_and_deletes_scan() {
        let dir = TempDir::new();
        let events = Arc::new(Mutex::new(Vec::new()));
        let statuses = Arc::new(Mutex::new(Vec::new()));
        let runner = ScanRunner::fake_one_host("192.168.1.1", "AA:BB:CC:DD:EE:FF");
        let manager = ScanManager::new_with_runner(db(&dir.db_file()).await, runner);

        let start = manager
            .start(
                StartScanRequest {
                    interface_name: "Ethernet".to_owned(),
                    cidr: "192.168.1.0/30".to_owned(),
                    tcp_fallback: true,
                    ports_enabled: false,
                },
                {
                    let events = Arc::clone(&events);
                    move |event| events.lock().expect("events").push(event)
                },
                Arc::new({
                    let statuses = Arc::clone(&statuses);
                    move |event| statuses.lock().expect("statuses").push(event)
                }),
            )
            .await
            .expect("start scan");
        let stopped = manager.stop().await.expect("stop scan");

        assert_eq!(stopped.scan_id, start.scan_id);
        assert!(matches!(
            events.lock().expect("events").first(),
            Some(ScanEvent::Host { .. })
        ));
        assert!(statuses
            .lock()
            .expect("statuses")
            .iter()
            .any(|event| matches!(event, ScanStatusEvent::Stopped { .. })));
        assert_eq!(manager.list_scans().await.expect("list").len(), 1);
        assert_eq!(
            manager
                .load_scan(start.scan_id)
                .await
                .expect("load")
                .hosts
                .len(),
            1
        );
        manager.delete_scan(start.scan_id).await.expect("delete");
        assert!(manager.list_scans().await.expect("list deleted").is_empty());
    }

    #[tokio::test]
    async fn manager_host_events_include_ports_only_when_enabled() {
        for ports_enabled in [false, true] {
            let dir = TempDir::new();
            let events = Arc::new(Mutex::new(Vec::new()));
            let runner = ScanRunner::fake_one_host_with_port_connector(
                "192.168.1.1",
                "AA:BB:CC:DD:EE:FF",
                port_connector(22),
            );
            let manager = ScanManager::new_with_runner(db(&dir.db_file()).await, runner);

            let start = manager
                .start(
                    StartScanRequest {
                        interface_name: "Ethernet".to_owned(),
                        cidr: "192.168.1.0/30".to_owned(),
                        tcp_fallback: true,
                        ports_enabled,
                    },
                    {
                        let events = Arc::clone(&events);
                        move |event| events.lock().expect("events").push(event)
                    },
                    Arc::new(|_event| {}),
                )
                .await
                .expect("start scan");
            tokio::time::timeout(Duration::from_secs(2), async {
                loop {
                    if !manager
                        .load_scan(start.scan_id)
                        .await
                        .expect("load")
                        .hosts
                        .is_empty()
                    {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("scan host persisted");

            let expected = if ports_enabled {
                vec![OpenPort {
                    port: 22,
                    service: "ssh".to_owned(),
                }]
            } else {
                Vec::new()
            };
            let loaded = manager.load_scan(start.scan_id).await.expect("load");
            assert_eq!(loaded.hosts[0].open_ports, expected);
            assert!(matches!(
                events.lock().expect("events").first(),
                Some(ScanEvent::Host { open_ports, .. }) if *open_ports == expected
            ));
        }
    }
}
