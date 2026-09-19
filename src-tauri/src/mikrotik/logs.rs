//! Live log streaming for MikroTik profiles.
//!
//! RouterOS REST has no streaming mode (official docs rule out continuous
//! commands like `monitor`), so the stream POLLs `/rest/log/print` on a
//! ~1.5s tick and dedupes records by their monotonic `.id`. New entries are
//! emitted over a Tauri Channel; there is deliberately NO DB session behind
//! a log stream — it is live-only and the UI caps its buffer.
//!
//! Failure handling mirrors the snapshot runtime (`runtime.rs`): a failed
//! poll emits `Warning` with its streak count, and after
//! [`MAX_LOG_FAILURES`] consecutive failures the stream emits a terminal
//! `Error` and stops, freeing its slot. Multiple streams run concurrently —
//! one per profile; a second start on the SAME profile is a typed
//! `AlreadyRunningForProfile`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;
use tokio::time::{interval, MissedTickBehavior};

use super::manager::MikrotikManager;
use super::parse::LogEntryDto;
use super::types::{
    MikrotikApi, MikrotikLogEvent, MikrotikLogEventSink, MikrotikLogStartDto,
    MikrotikLogStatusEvent, MikrotikLogStatusSink, MikrotikManagerError,
};

/// Fallback poll cadence when the caller does not pick one.
pub const LOG_POLL: Duration = Duration::from_secs(2);
/// Backlog emitted on stream start: the newest N entries already in the
/// router's memory buffer, so the view opens with context.
pub const LOG_BACKLOG: usize = 50;
/// Consecutive failed polls after which the stream gives up (terminal
/// `Error` + stop), mirroring `runtime::MAX_CORE_FAILURES`.
pub const MAX_LOG_FAILURES: u32 = 3;

/// Poll cadence for one stream, chosen per start (the UI offers whole
/// seconds). Tests shrink it so a poll cycle does not burn real seconds.
#[derive(Clone, Copy, Debug)]
pub struct LogPoll {
    pub interval: Duration,
}

impl Default for LogPoll {
    fn default() -> Self {
        Self { interval: LOG_POLL }
    }
}

/// Clamp for the UI-facing poll interval in seconds. Sub-second polling
/// drowns a busy router; past a minute the view is no longer "live".
pub const MIN_POLL_SECS: u64 = 1;
pub const MAX_POLL_SECS: u64 = 60;

/// Build the per-stream poll cadence from the UI's whole-second choice.
pub fn poll_from_secs(secs: u64) -> LogPoll {
    LogPoll {
        interval: Duration::from_secs(secs.clamp(MIN_POLL_SECS, MAX_POLL_SECS)),
    }
}

#[derive(Clone)]
pub struct LogStreamManager {
    manager: MikrotikManager,
    inner: Arc<Mutex<LogInner>>,
}

#[derive(Default)]
struct LogInner {
    /// Live streams keyed by `profile_id`: one stream per device.
    active: HashMap<i64, ActiveLogStream>,
}

struct ActiveLogStream {
    stop_tx: watch::Sender<bool>,
    join_handle: JoinHandle<Result<(), MikrotikManagerError>>,
}

/// The numeric part of a RouterOS record id (`*17` -> 23, `*1F` -> 31).
/// RouterOS renders internal ids in HEXADECIMAL (`*9`, `*A`, `*1F`, ...), so
/// decimal parsing silently fails from the tenth entry on — which would
/// freeze the stream exactly like a dead poll. Log ids are monotonic for
/// the life of the memory buffer, which is what the dedupe relies on.
/// Returns `None` for ids that do not match the shape — those entries can
/// never be deduped safely and are dropped.
fn log_id_number(id: &str) -> Option<u64> {
    id.strip_prefix('*')
        .and_then(|raw| u64::from_str_radix(raw, 16).ok())
}

/// Backlog selection: the newest [`LOG_BACKLOG`] entries, oldest first
/// (the router returns oldest first; take from the tail, then re-reverse).
fn backlog_of(entries: &[LogEntryDto]) -> Vec<LogEntryDto> {
    entries
        .iter()
        .rev()
        .take(LOG_BACKLOG)
        .rev()
        .cloned()
        .collect()
}

/// Keep only entries newer than `last_id` and advance it. Entries whose id
/// does not parse are dropped — emitting them again on every poll would
/// spam the UI.
fn fresh_entries(entries: Vec<LogEntryDto>, last_id: &mut Option<u64>) -> Vec<LogEntryDto> {
    let fresh: Vec<LogEntryDto> = entries
        .into_iter()
        .filter(|entry| match (log_id_number(&entry.id), *last_id) {
            (Some(new), Some(seen)) => new > seen,
            (Some(_), None) => true,
            (None, _) => false,
        })
        .collect();
    if let Some(max_id) = fresh
        .iter()
        .filter_map(|entry| log_id_number(&entry.id))
        .max()
    {
        *last_id = Some(max_id);
    }
    fresh
}

impl LogStreamManager {
    pub fn new(manager: MikrotikManager) -> Self {
        Self {
            manager,
            inner: Arc::new(Mutex::new(LogInner::default())),
        }
    }

    pub async fn start<E>(
        &self,
        profile_id: i64,
        poll: LogPoll,
        on_event: E,
        on_status: MikrotikLogStatusSink,
    ) -> Result<MikrotikLogStartDto, MikrotikManagerError>
    where
        E: Fn(MikrotikLogEvent) + Send + Sync + 'static,
    {
        {
            let inner = self.inner.lock().await;
            if inner.active.contains_key(&profile_id) {
                return Err(MikrotikManagerError::AlreadyRunningForProfile(profile_id));
            }
        }
        // Not under the lock: a slow connect must not block stop/clear for
        // other profiles' streams.
        let profile = self.manager.require_profile(profile_id).await?;
        let conn = self.manager.connection_for(&profile).await?;
        let api = match (self.manager.api_factory())(conn).await {
            Ok(api) => api,
            Err(err) => {
                on_status(MikrotikLogStatusEvent::Error {
                    message: format!("{err}"),
                });
                return Err(err.into());
            }
        };

        // Re-validate: another start may have claimed the profile while we
        // were connecting.
        let mut inner = self.inner.lock().await;
        if inner.active.contains_key(&profile_id) {
            return Err(MikrotikManagerError::AlreadyRunningForProfile(profile_id));
        }
        let (stop_tx, stop_rx) = watch::channel(false);
        let manager = self.clone();
        let on_event: MikrotikLogEventSink = Arc::new(on_event);
        let join_handle = tokio::spawn(async move {
            run_log_stream(RunLogContext {
                manager,
                api,
                poll,
                profile_id,
                on_event,
                on_status,
                stop_rx,
            })
            .await
        });
        inner.active.insert(
            profile_id,
            ActiveLogStream {
                stop_tx,
                join_handle,
            },
        );
        Ok(MikrotikLogStartDto { profile_id })
    }

    /// Cancel the stream for ONE profile and wait for its task. A task that
    /// already terminated with an error is swallowed here: the terminal
    /// `Error` status event is the report channel, and `stop` must stay
    /// idempotent from the UI's perspective.
    pub async fn stop(&self, profile_id: i64) -> Result<(), MikrotikManagerError> {
        let active = {
            self.inner
                .lock()
                .await
                .active
                .remove(&profile_id)
                .ok_or(MikrotikManagerError::NoActiveSession)?
        };
        let _ = active.stop_tx.send(true);
        let result = active.join_handle.await.map_err(|err| {
            MikrotikManagerError::Api(crate::mikrotik::error::MikrotikError::Connect(format!(
                "log stream task panicked: {err}"
            )))
        })?;
        let _ = result;
        Ok(())
    }

    pub(crate) async fn clear_active(&self, profile_id: i64) {
        self.inner.lock().await.active.remove(&profile_id);
    }
}

struct RunLogContext {
    manager: LogStreamManager,
    api: Arc<dyn MikrotikApi>,
    poll: LogPoll,
    profile_id: i64,
    on_event: MikrotikLogEventSink,
    on_status: MikrotikLogStatusSink,
    stop_rx: watch::Receiver<bool>,
}

async fn run_log_stream(ctx: RunLogContext) -> Result<(), MikrotikManagerError> {
    let RunLogContext {
        manager,
        api,
        poll,
        profile_id,
        on_event,
        on_status,
        mut stop_rx,
    } = ctx;

    let result = run_inner(api, poll, profile_id, &on_event, &on_status, &mut stop_rx).await;
    manager.clear_active(profile_id).await;
    result
}

async fn run_inner(
    api: Arc<dyn MikrotikApi>,
    poll: LogPoll,
    profile_id: i64,
    on_event: &MikrotikLogEventSink,
    on_status: &MikrotikLogStatusSink,
    stop_rx: &mut watch::Receiver<bool>,
) -> Result<(), MikrotikManagerError> {
    (on_status)(MikrotikLogStatusEvent::Started { profile_id });

    // Backlog: newest LOG_BACKLOG entries, oldest first.
    let mut last_id: Option<u64>;
    match api.get_log().await {
        Ok(entries) => {
            let backlog = backlog_of(&entries);
            last_id = backlog
                .iter()
                .filter_map(|entry| log_id_number(&entry.id))
                .max();
            if !backlog.is_empty() {
                (on_event)(MikrotikLogEvent::Entries { entries: backlog });
            }
        }
        Err(err) => {
            (on_status)(MikrotikLogStatusEvent::Error {
                message: format!("{err}"),
            });
            return Err(err.into());
        }
    }

    let mut failures = 0u32;
    let mut tick = interval(poll.interval);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                // Both a `true` send and a dropped sender mean "stop" —
                // either way the UI channel is done with us.
                (on_status)(MikrotikLogStatusEvent::Stopped);
                return Ok(());
            }
            _ = tick.tick() => {}
        }

        match api.get_log().await {
            Ok(entries) => {
                failures = 0;
                let fresh = fresh_entries(entries, &mut last_id);
                if !fresh.is_empty() {
                    (on_event)(MikrotikLogEvent::Entries { entries: fresh });
                }
            }
            Err(err) => {
                failures += 1;
                if failures >= MAX_LOG_FAILURES {
                    (on_status)(MikrotikLogStatusEvent::Error {
                        message: format!(
                            "log stream failed {MAX_LOG_FAILURES} polls in a row: {err}"
                        ),
                    });
                    return Err(err.into());
                }
                (on_status)(MikrotikLogStatusEvent::Warning {
                    message: format!("log poll failed ({failures}/{MAX_LOG_FAILURES}): {err}"),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    use crate::db::{now_rfc3339, Database, NewMikrotikProfile};
    use crate::mikrotik::client::MikrotikConnection;
    use crate::mikrotik::error::MikrotikError;
    use crate::mikrotik::parse::{
        BridgeVlanDto, EthernetMonitorDto, EthernetStatsDto, InterfaceDto, LogSeverity,
        ResourceDto, SensorDto, VlanDto,
    };
    use crate::mikrotik::secrets::{MemoryStore, SecretStore};
    use crate::mikrotik::types::MikrotikApiFactory;

    // ---- Pure helpers ------------------------------------------------------

    #[test]
    fn log_id_number_parses_routeros_hex_ids() {
        // RouterOS renders record ids in HEXADECIMAL: *9, *A, *1F, ...
        // Decimal parsing would silently fail from the tenth entry on and
        // freeze the stream — this is the "logs stop updating" bug.
        assert_eq!(log_id_number("*0"), Some(0));
        assert_eq!(log_id_number("*9"), Some(9));
        assert_eq!(log_id_number("*A"), Some(10));
        assert_eq!(log_id_number("*17"), Some(23));
        assert_eq!(log_id_number("*1F"), Some(31));
        assert_eq!(log_id_number("17"), None);
        assert_eq!(log_id_number("*G"), None);
        assert_eq!(log_id_number(""), None);
    }

    #[test]
    fn poll_from_secs_clamps_to_sane_bounds() {
        assert_eq!(poll_from_secs(0).interval, Duration::from_secs(1));
        assert_eq!(poll_from_secs(2).interval, Duration::from_secs(2));
        assert_eq!(poll_from_secs(30).interval, Duration::from_secs(30));
        assert_eq!(poll_from_secs(3600).interval, Duration::from_secs(60));
    }

    fn entry(id: &str, topics: &str) -> LogEntryDto {
        let topics: Vec<String> = topics
            .split(',')
            .map(|topic| topic.trim().to_owned())
            .collect();
        LogEntryDto {
            id: id.to_owned(),
            time: Some("12:52:24".to_owned()),
            severity: crate::mikrotik::parse::classify_log_severity(&topics),
            topics,
            message: format!("message {id}"),
        }
    }

    #[test]
    fn backlog_keeps_newest_entries_in_router_order() {
        let entries: Vec<LogEntryDto> = (1..=60)
            .map(|n| entry(&format!("*{n}"), "system,info"))
            .collect();
        let backlog = backlog_of(&entries);
        assert_eq!(backlog.len(), LOG_BACKLOG);
        assert_eq!(backlog.first().unwrap().id, "*11");
        assert_eq!(backlog.last().unwrap().id, "*60");
        // Order preserved oldest-first within the backlog.
        assert!(backlog.windows(2).all(|pair| pair[0].id < pair[1].id));
    }

    #[test]
    fn backlog_of_takes_everything_when_smaller_than_cap() {
        let entries = vec![entry("*1", "system,info"), entry("*2", "system,warning")];
        let backlog = backlog_of(&entries);
        assert_eq!(backlog.len(), 2);
        assert_eq!(backlog[1].severity, LogSeverity::Warning);
    }

    #[test]
    fn fresh_entries_filters_by_last_id_and_advances_it() {
        let mut last_id = Some(5u64);
        let entries = vec![
            entry("*4", "system,info"),  // older — skipped
            entry("*6", "system,info"),  // new
            entry("*7", "system,error"), // new
        ];
        let fresh = fresh_entries(entries, &mut last_id);
        assert_eq!(fresh.len(), 2);
        assert_eq!(fresh[0].id, "*6");
        assert_eq!(fresh[1].id, "*7");
        assert_eq!(fresh[1].severity, LogSeverity::Error);
        assert_eq!(last_id, Some(7));

        // Nothing new on the next poll.
        assert!(fresh_entries(Vec::new(), &mut last_id).is_empty());
        assert_eq!(last_id, Some(7));
    }

    #[test]
    fn fresh_entries_drops_unparseable_ids() {
        let mut last_id = Some(1u64);
        let entries = vec![entry("weird", "system,info"), entry("*2", "system,info")];
        let fresh = fresh_entries(entries, &mut last_id);
        assert_eq!(fresh.len(), 1);
        assert_eq!(fresh[0].id, "*2");
    }

    // ---- End-to-end stream tests -------------------------------------------

    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "verkkokyyla-logstream-{name}-{}-{stamp}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Scripted `get_log` responses; the LAST entry repeats indefinitely.
    type LogScript = Arc<Mutex<VecDeque<Result<Vec<LogEntryDto>, MikrotikError>>>>;

    struct LogOnlyApi {
        script: LogScript,
        calls: Arc<std::sync::atomic::AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl super::MikrotikApi for LogOnlyApi {
        async fn get_resource(&self) -> Result<ResourceDto, MikrotikError> {
            unreachable!("log stream only calls get_log")
        }
        async fn get_interfaces(&self) -> Result<Vec<InterfaceDto>, MikrotikError> {
            unreachable!("log stream only calls get_log")
        }
        async fn get_health(&self) -> Result<Vec<SensorDto>, MikrotikError> {
            unreachable!("log stream only calls get_log")
        }
        async fn get_interface_stats_detail(&self) -> Result<Vec<InterfaceDto>, MikrotikError> {
            unreachable!("log stream only calls get_log")
        }
        async fn get_ethernet_stats(&self) -> Result<Vec<EthernetStatsDto>, MikrotikError> {
            unreachable!("log stream only calls get_log")
        }
        async fn get_ethernet_monitor(
            &self,
            _name: &str,
        ) -> Result<Vec<EthernetMonitorDto>, MikrotikError> {
            unreachable!("log stream only calls get_log")
        }
        async fn get_vlans(&self) -> Result<Vec<VlanDto>, MikrotikError> {
            unreachable!("log stream only calls get_log")
        }
        async fn get_bridge_vlans(&self) -> Result<Vec<BridgeVlanDto>, MikrotikError> {
            unreachable!("log stream only calls get_log")
        }

        async fn get_log(&self) -> Result<Vec<LogEntryDto>, MikrotikError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut script = self.script.lock().await;
            if script.len() > 1 {
                script.pop_front().unwrap()
            } else {
                script.front().cloned().unwrap_or_else(|| Ok(Vec::new()))
            }
        }
    }

    struct Harness {
        manager: LogStreamManager,
        profile_id: i64,
        calls: Arc<std::sync::atomic::AtomicUsize>,
        events: tokio::sync::mpsc::UnboundedReceiver<MikrotikLogEvent>,
        statuses: tokio::sync::mpsc::UnboundedReceiver<MikrotikLogStatusEvent>,
        _dir: TempDir,
    }

    async fn harness(name: &str, script: Vec<Result<Vec<LogEntryDto>, MikrotikError>>) -> Harness {
        let dir = TempDir::new(name);
        let db = Database::connect(&dir.0.join("logs.db")).await.expect("db");
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
        let profile = db
            .create_mikrotik_profile(&NewMikrotikProfile {
                name: "lab".to_owned(),
                host: "192.0.2.10".to_owned(),
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

        let script: LogScript = Arc::new(Mutex::new(script.into()));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let api = Arc::new(LogOnlyApi {
            script: Arc::clone(&script),
            calls: Arc::clone(&calls),
        });
        let factory: MikrotikApiFactory = Arc::new(move |_conn: MikrotikConnection| {
            let api = Arc::clone(&api);
            Box::pin(async move { Ok(api as Arc<dyn super::MikrotikApi>) })
        });
        let manager = MikrotikManager::new(db, store, factory);

        let (event_tx, events) = tokio::sync::mpsc::unbounded_channel();
        let (status_tx, statuses) = tokio::sync::mpsc::unbounded_channel();
        let stream = LogStreamManager::new(manager);
        stream
            .start(
                profile.id,
                LogPoll {
                    interval: Duration::from_millis(20),
                },
                move |event| {
                    let _ = event_tx.send(event);
                },
                Arc::new(move |status| {
                    let _ = status_tx.send(status);
                }),
            )
            .await
            .expect("start");
        Harness {
            manager: stream,
            profile_id: profile.id,
            calls,
            events,
            statuses,
            _dir: dir,
        }
    }

    async fn next_event(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<MikrotikLogEvent>,
    ) -> MikrotikLogEvent {
        tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("event within 5s")
            .expect("event channel open")
    }

    async fn next_status(
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<MikrotikLogStatusEvent>,
    ) -> MikrotikLogStatusEvent {
        tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .expect("status within 5s")
            .expect("status channel open")
    }

    async fn wait_for_calls(calls: &Arc<std::sync::atomic::AtomicUsize>, at_least: usize) {
        for _ in 0..500 {
            if calls.load(std::sync::atomic::Ordering::SeqCst) >= at_least {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!("expected at least {at_least} get_log calls");
    }

    #[tokio::test]
    async fn stream_emits_backlog_then_fresh_entries_then_stopped() {
        let mut h = harness(
            "happy",
            vec![
                Ok(vec![entry("*1", "system,info"), entry("*2", "dhcp,error")]),
                Ok(vec![
                    entry("*1", "system,info"),
                    entry("*2", "dhcp,error"),
                    entry("*3", "system,warning"),
                ]),
            ],
        )
        .await;

        assert_eq!(
            next_status(&mut h.statuses).await,
            MikrotikLogStatusEvent::Started {
                profile_id: h.profile_id
            }
        );
        match next_event(&mut h.events).await {
            MikrotikLogEvent::Entries { entries } => {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].id, "*1");
                assert_eq!(entries[1].severity, LogSeverity::Error);
            }
        }

        match next_event(&mut h.events).await {
            MikrotikLogEvent::Entries { entries } => {
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0].id, "*3");
                assert_eq!(entries[0].severity, LogSeverity::Warning);
            }
        }

        h.manager.stop(h.profile_id).await.expect("stop");
        assert_eq!(
            next_status(&mut h.statuses).await,
            MikrotikLogStatusEvent::Stopped
        );
    }

    #[tokio::test]
    async fn hex_ids_past_nine_keep_streaming() {
        // Regression: RouterOS ids are hex (*9, *A, *1F...). Decimal parsing
        // silently dropped every entry past *9, so the stream showed the
        // backlog and never updated.
        let mut h = harness(
            "hex-ids",
            vec![
                Ok(vec![entry("*8", "system,info"), entry("*9", "system,info")]),
                Ok(vec![
                    entry("*8", "system,info"),
                    entry("*9", "system,info"),
                    entry("*A", "system,warning"),
                    entry("*1F", "system,error"),
                ]),
            ],
        )
        .await;

        match next_event(&mut h.events).await {
            MikrotikLogEvent::Entries { entries } => {
                assert_eq!(entries.len(), 2);
            }
        }
        match next_event(&mut h.events).await {
            MikrotikLogEvent::Entries { entries } => {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].id, "*A");
                assert_eq!(entries[0].severity, LogSeverity::Warning);
                assert_eq!(entries[1].id, "*1F");
                assert_eq!(entries[1].severity, LogSeverity::Error);
            }
        }

        h.manager.stop(h.profile_id).await.expect("stop");
    }

    #[tokio::test]
    async fn second_start_while_running_is_already_running() {
        let h = harness("busy", vec![Ok(Vec::new())]).await;
        let err = h
            .manager
            .start(
                h.profile_id,
                LogPoll {
                    interval: Duration::from_millis(20),
                },
                |_| {},
                Arc::new(|_| {}),
            )
            .await
            .expect_err("second start");
        assert!(
            matches!(err, MikrotikManagerError::AlreadyRunningForProfile(id) if id == h.profile_id)
        );
        h.manager.stop(h.profile_id).await.expect("stop");
    }

    #[tokio::test]
    async fn two_profiles_stream_concurrently_and_stop_independently() {
        let dir = TempDir::new("multi-stream");
        let db = Database::connect(&dir.0.join("multi.db"))
            .await
            .expect("db");
        let store: Arc<dyn SecretStore> = Arc::new(MemoryStore::new());
        let mut profile_ids = Vec::new();
        for name in ["edge-a", "edge-b"] {
            let profile = db
                .create_mikrotik_profile(&NewMikrotikProfile {
                    name: name.to_owned(),
                    host: format!("192.0.2.{}", profile_ids.len() + 10),
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
            profile_ids.push(profile.id);
        }

        let api = Arc::new(LogOnlyApi {
            script: Arc::new(Mutex::new(VecDeque::from(vec![Ok(vec![entry(
                "*1",
                "system,info",
            )])]))),
            calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        });
        let factory: MikrotikApiFactory = Arc::new(move |_conn: MikrotikConnection| {
            let api = Arc::clone(&api);
            Box::pin(async move { Ok(api as Arc<dyn super::MikrotikApi>) })
        });
        let stream = LogStreamManager::new(MikrotikManager::new(db, store, factory));

        let mut receivers = Vec::new();
        for &profile_id in &profile_ids {
            let (event_tx, events) = tokio::sync::mpsc::unbounded_channel();
            stream
                .start(
                    profile_id,
                    LogPoll {
                        interval: Duration::from_millis(20),
                    },
                    move |event| {
                        let _ = event_tx.send(event);
                    },
                    Arc::new(|_| {}),
                )
                .await
                .expect("start");
            receivers.push(events);
        }

        // Both streams deliver their own backlog.
        for events in receivers.iter_mut() {
            match next_event(events).await {
                MikrotikLogEvent::Entries { entries } => {
                    assert_eq!(entries.len(), 1);
                    assert_eq!(entries[0].id, "*1");
                }
            }
        }

        // Stopping one leaves the other live: a second stop of the same
        // profile is "no active session" while b still streams.
        stream.stop(profile_ids[0]).await.expect("stop a");
        let err = stream.stop(profile_ids[0]).await;
        assert!(
            matches!(err, Err(MikrotikManagerError::NoActiveSession)),
            "a is gone: {err:?}"
        );
        let restarted = stream
            .start(
                profile_ids[0],
                LogPoll {
                    interval: Duration::from_millis(20),
                },
                |_| {},
                Arc::new(|_| {}),
            )
            .await;
        assert!(restarted.is_ok(), "restart after stop: {restarted:?}");
        stream.stop(profile_ids[1]).await.expect("stop b");
        stream.stop(profile_ids[0]).await.expect("stop restarted a");
    }

    #[tokio::test]
    async fn failure_streak_emits_warnings_then_terminal_error_and_frees_slot() {
        let api_err = || {
            Err(MikrotikError::Api {
                status: 500,
                message: "boom".to_owned(),
            })
        };
        let mut h = harness(
            "failing",
            vec![Ok(Vec::new()), api_err(), api_err(), api_err()],
        )
        .await;

        assert_eq!(
            next_status(&mut h.statuses).await,
            MikrotikLogStatusEvent::Started {
                profile_id: h.profile_id
            }
        );
        assert_eq!(
            next_status(&mut h.statuses).await,
            MikrotikLogStatusEvent::Warning {
                message: "log poll failed (1/3): router API error 500: boom".to_owned(),
            }
        );
        assert_eq!(
            next_status(&mut h.statuses).await,
            MikrotikLogStatusEvent::Warning {
                message: "log poll failed (2/3): router API error 500: boom".to_owned(),
            }
        );
        match next_status(&mut h.statuses).await {
            MikrotikLogStatusEvent::Error { message } => {
                assert!(message.contains("3 polls in a row"), "{message}");
            }
            other => panic!("expected terminal error, got {other:?}"),
        }

        // Terminal exit frees the slot: a fresh start succeeds.
        wait_for_calls(&h.calls, 4).await;
        h.manager
            .start(
                h.profile_id,
                LogPoll {
                    interval: Duration::from_millis(20),
                },
                |_| {},
                Arc::new(|_| {}),
            )
            .await
            .expect("restart after terminal error");
        h.manager.stop(h.profile_id).await.expect("stop");
    }

    #[tokio::test]
    async fn backlog_failure_is_immediate_terminal_error() {
        let api_err = MikrotikError::Connect("refused".to_owned());
        let mut h = harness("backlog-fail", vec![Err(api_err)]).await;
        match next_status(&mut h.statuses).await {
            MikrotikLogStatusEvent::Started { .. } => {}
            other => panic!("expected started, got {other:?}"),
        }
        match next_status(&mut h.statuses).await {
            MikrotikLogStatusEvent::Error { message } => {
                assert!(message.contains("refused"), "{message}");
            }
            other => panic!("expected error, got {other:?}"),
        }
    }
}
