//! OPNsense live monitor: one snapshot-producing task per profile.
//!
//! The poll loop fetches system and VPN sections every [`SYSTEM_TICK`],
//! and rules every third tick. Intervening snapshots omit rules instead
//! of cloning and serializing the same potentially large table; the view
//! retains its last good data. A failed section is emitted as an error
//! with a stale timestamp; all device traffic is read-only HTTP GET via
//! [`OpnsenseApi`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;

use crate::db::now_rfc3339;
use crate::opnsense::client::{join_rule_counters, InterfaceInfoPage, OpnsenseApi, OpnsenseError};
use crate::opnsense::manager::OpnsenseManager;
use crate::opnsense::types::{
    OpnsenseActiveMonitorDto, OpnsenseManagerError, OpnsenseRulesSectionDto, OpnsenseSectionDto,
    OpnsenseSectionErrorDto, OpnsenseSnapshotDto, OpnsenseSnapshotSink, OpnsenseStartDto,
    OpnsenseStatusEvent, OpnsenseStatusSink, OpnsenseSystemSectionDto, OpnsenseVpnSectionDto,
    OpnsenseVpnServiceDto,
};

/// Global concurrency cap: at most eight live monitors.
const MAX_MONITORS: usize = 8;
/// Hard cap on the rule listing; beyond this the section is flagged
/// `truncated`.
const RULES_CAP: usize = 10_000;
/// Hard cap on the interface listing.
const INTERFACES_CAP: usize = 10_000;

/// System/VPN cadence.
const SYSTEM_TICK: Duration = Duration::from_secs(5);
/// Rules cadence: every Nth system tick.
const RULES_EVERY_TICKS: u32 = 3;

struct ActiveMonitor {
    stop_tx: Arc<watch::Sender<bool>>,
    join_handle: JoinHandle<()>,
}

/// One live monitor per profile (second start of the same profile → typed
/// [`OpnsenseManagerError::AlreadyMonitoring`]), each backed by a spawned
/// task with a per-session `watch<bool>` cancel channel. Mirrors the
/// MikroTik session lifecycle in `mikrotik/manager.rs`.
#[derive(Clone)]
pub struct OpnsenseMonitorManager {
    manager: OpnsenseManager,
    inner: Arc<Mutex<HashMap<i64, ActiveMonitor>>>,
    system_tick: Duration,
    rules_every_ticks: u32,
}

impl OpnsenseMonitorManager {
    /// The monitor manager shares the profile manager's activity registry
    /// (`manager.activity()`) so deletion is refused while a monitor runs.
    pub fn new(manager: OpnsenseManager) -> Self {
        Self {
            manager,
            inner: Arc::new(Mutex::new(HashMap::new())),
            system_tick: SYSTEM_TICK,
            rules_every_ticks: RULES_EVERY_TICKS,
        }
    }

    #[cfg(test)]
    fn with_cadence(
        manager: OpnsenseManager,
        system_tick: Duration,
        rules_every_ticks: u32,
    ) -> Self {
        let mut manager = Self::new(manager);
        manager.system_tick = system_tick;
        manager.rules_every_ticks = rules_every_ticks.max(1);
        manager
    }

    /// Start monitoring a profile. The first snapshot is fetched
    /// immediately; the task then polls until cancelled with [`stop`].
    pub async fn start(
        &self,
        profile_id: i64,
        on_snapshot: OpnsenseSnapshotSink,
        on_status: OpnsenseStatusSink,
    ) -> Result<OpnsenseStartDto, OpnsenseManagerError> {
        {
            let inner = self.inner.lock().await;
            if inner.contains_key(&profile_id) {
                return Err(OpnsenseManagerError::AlreadyMonitoring(profile_id));
            }
            if inner.len() >= MAX_MONITORS {
                return Err(OpnsenseManagerError::TooManySessions);
            }
        }
        // The lifecycle lock is NOT held across the awaits below: a slow
        // connect must not block stop/list for unrelated profiles.
        let profile = self.manager.require_profile(profile_id).await?;
        let conn = self.manager.connection_for(&profile).await?;
        let api = match (self.manager.api_factory())(conn).await {
            Ok(api) => api,
            Err(err) => {
                on_status(OpnsenseStatusEvent::Error {
                    profile_id,
                    message: err.to_string(),
                });
                return Err(err);
            }
        };

        // Re-validate under the lock: another start may have claimed the
        // profile (or the last slot) while we were connecting.
        let (stop_tx, stop_rx) = watch::channel(false);
        let task_stop_tx = Arc::new(stop_tx);
        let activity = self.manager.activity();
        let mut inner = self.inner.lock().await;
        if inner.contains_key(&profile_id) {
            return Err(OpnsenseManagerError::AlreadyMonitoring(profile_id));
        }
        if inner.len() >= MAX_MONITORS {
            return Err(OpnsenseManagerError::TooManySessions);
        }
        {
            let mut activity_guard = activity.lock().await;
            activity_guard.monitor_enter(profile_id);
            drop(activity_guard);
            let task_inner = Arc::clone(&self.inner);
            let task_stop = Arc::clone(&task_stop_tx);
            let system_tick = self.system_tick;
            let rules_every_ticks = self.rules_every_ticks;
            let join_handle = tokio::spawn(async move {
                run_monitor(
                    api,
                    profile_id,
                    on_snapshot,
                    on_status,
                    stop_rx,
                    system_tick,
                    rules_every_ticks,
                )
                .await;
                // Task exit always releases the slot and the activity flag
                // — but ONLY when this task still owns them: stop() removes
                // both eagerly, and a fresh monitor for the same profile
                // may already own a new slot by the time we get here.
                let mut inner = task_inner.lock().await;
                let own_slot = matches!(
                    inner.get(&profile_id),
                    Some(active) if Arc::ptr_eq(&active.stop_tx, &task_stop)
                );
                if own_slot {
                    inner.remove(&profile_id);
                }
                drop(inner);
                if own_slot {
                    activity.lock().await.monitor_exit(profile_id);
                }
            });
            inner.insert(
                profile_id,
                ActiveMonitor {
                    stop_tx: task_stop_tx,
                    join_handle,
                },
            );
        }
        drop(inner);
        Ok(OpnsenseStartDto { profile_id })
    }

    /// Cancel ONE monitor and wait for its task. The slot is removed before
    /// awaiting so restart is possible, but the activity mark remains until
    /// the old task has fully stopped. Reference-counted activity keeps a
    /// concurrently restarted monitor protected from profile deletion.
    pub async fn stop(&self, profile_id: i64) -> Result<(), OpnsenseManagerError> {
        let active = self
            .inner
            .lock()
            .await
            .remove(&profile_id)
            .ok_or(OpnsenseManagerError::NoActiveMonitor)?;
        let _ = active.stop_tx.send(true);
        let result = active.join_handle.await.map_err(|err| {
            OpnsenseManagerError::Api(OpnsenseError::Connect(format!(
                "opnsense monitor task panicked: {err}"
            )))
        });
        self.manager
            .activity()
            .lock()
            .await
            .monitor_exit(profile_id);
        result
    }

    /// The currently live monitors, for rehydrating the UI after a reload.
    pub async fn list_active(&self) -> Vec<OpnsenseActiveMonitorDto> {
        self.inner
            .lock()
            .await
            .keys()
            .map(|&profile_id| OpnsenseActiveMonitorDto { profile_id })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Poll loop
// ---------------------------------------------------------------------------

async fn run_monitor(
    api: Arc<dyn OpnsenseApi>,
    profile_id: i64,
    on_snapshot: OpnsenseSnapshotSink,
    on_status: OpnsenseStatusSink,
    mut stop_rx: watch::Receiver<bool>,
    system_tick: Duration,
    rules_every_ticks: u32,
) {
    let mut interval = tokio::time::interval(system_tick);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // The first interval tick fires immediately; consume it so the loop
    // waits a full period after every emitted snapshot.
    interval.tick().await;

    let mut tick: u32 = 0;
    loop {
        let (system, vpn, rules) = tokio::select! {
            _ = stop_rx.changed() => break,
            sections = async {
                let system = fetch_system_section(api.as_ref()).await;
                let vpn = fetch_vpn_section(api.as_ref()).await;
                let rules = if tick.is_multiple_of(rules_every_ticks) {
                    Some(fetch_rules_section(api.as_ref()).await)
                } else {
                    None
                };
                (system, vpn, rules)
            } => sections,
        };
        for (name, error) in [
            ("system", system.error.as_ref()),
            ("vpn", vpn.error.as_ref()),
            (
                "rules",
                rules.as_ref().and_then(|section| section.error.as_ref()),
            ),
        ] {
            if let Some(error) = error {
                on_status(OpnsenseStatusEvent::Warning {
                    profile_id,
                    message: format!("{name} section failed: {}", error.message),
                });
            }
        }
        on_snapshot(OpnsenseSnapshotDto {
            profile_id,
            at: now_rfc3339(),
            rules,
            system,
            vpn,
        });
        if tick == 0 {
            on_status(OpnsenseStatusEvent::Started { profile_id });
        }
        tick = tick.wrapping_add(1);
        tokio::select! {
            _ = stop_rx.changed() => break,
            _ = interval.tick() => {}
        }
    }
    on_status(OpnsenseStatusEvent::Stopped { profile_id });
}

// ---------------------------------------------------------------------------
// Section fetchers — each section fails independently
// ---------------------------------------------------------------------------

async fn fetch_rules_section(api: &dyn OpnsenseApi) -> OpnsenseSectionDto<OpnsenseRulesSectionDto> {
    match fetch_rules(api).await {
        Ok(section) => OpnsenseSectionDto::ok(section),
        Err(err) => OpnsenseSectionDto::error(&err),
    }
}

/// Fetch ALL rules in one GET (OPNsense ignores GET pagination params),
/// then join fresh per-UUID counters. A counters failure (e.g. 403 on
/// `ruleStats`) keeps the rule definitions with a `counters_error`
/// instead of failing the section.
async fn fetch_rules(api: &dyn OpnsenseApi) -> Result<OpnsenseRulesSectionDto, OpnsenseError> {
    let page = api.search_rules().await?;
    let mut rules = page.rules;
    let total = page.total;
    let mut truncated = false;
    if rules.len() >= RULES_CAP {
        truncated = total > RULES_CAP as i64 || rules.len() > RULES_CAP;
        rules.truncate(RULES_CAP);
    } else if (rules.len() as i64) < total {
        // rows < reported total — the server genuinely withheld rows.
        truncated = true;
    }

    let (counters_error, unmapped_running) = match api.rule_stats().await {
        Err(err) => (Some(OpnsenseSectionErrorDto::from_error(&err)), Vec::new()),
        Ok(stats) => {
            // Active PF labels only feed unmapped-rule detection; their
            // failure is not fatal.
            let pf_ids = api.list_rule_ids().await.unwrap_or_default();
            let unmapped = join_rule_counters(&mut rules, &stats, &pf_ids);
            (None, unmapped)
        }
    };
    Ok(OpnsenseRulesSectionDto {
        rules,
        total,
        truncated,
        counters_error,
        unmapped_running,
    })
}

async fn fetch_system_section(
    api: &dyn OpnsenseApi,
) -> OpnsenseSectionDto<OpnsenseSystemSectionDto> {
    match fetch_system(api).await {
        Ok(section) => OpnsenseSectionDto::ok(section),
        Err(err) => OpnsenseSectionDto::error(&err),
    }
}

/// Interfaces (single GET) plus best-effort PF state capacity and system
/// resources. Only the interfaces endpoint can fail the whole section; the
/// others are surfaced as per-field errors.
async fn fetch_system(api: &dyn OpnsenseApi) -> Result<OpnsenseSystemSectionDto, OpnsenseError> {
    let page: InterfaceInfoPage = api.interfaces_info().await?;
    let mut interfaces = page.interfaces;
    let total = page.total;
    let mut interfaces_truncated = false;
    if interfaces.len() >= INTERFACES_CAP {
        interfaces_truncated = total > INTERFACES_CAP as i64 || interfaces.len() > INTERFACES_CAP;
        interfaces.truncate(INTERFACES_CAP);
    } else if (interfaces.len() as i64) < total {
        interfaces_truncated = true;
    }

    // Traffic counters merge by EXACT interface name only; a statistics
    // failure leaves the interface list intact.
    if let Ok(stats) = api.interface_statistics().await {
        for iface in &mut interfaces {
            if let Some(stat) = stats.iter().find(|s| s.name == iface.name) {
                iface.rx_bytes = stat.rx_bytes.or(iface.rx_bytes);
                iface.tx_bytes = stat.tx_bytes.or(iface.tx_bytes);
                iface.rx_packets = stat.rx_packets.or(iface.rx_packets);
                iface.tx_packets = stat.tx_packets.or(iface.tx_packets);
            }
        }
    }

    let (pf_states, pf_states_error) = match api.pf_states().await {
        Ok(value) => (Some(value), None),
        Err(err) => (None, Some(OpnsenseSectionErrorDto::from_error(&err))),
    };

    // Resources are three independent sources: memory (system_resources),
    // uptime/load (system_time) and CPU % (a short-lived SSE sample). Each
    // is best-effort; the section errors only when ALL fail.
    let memory = api.system_resources().await;
    let time = api.system_time().await;
    let cpu = api.cpu_usage_sample().await;
    let (resources, resources_error) = match (&memory, &time, &cpu) {
        (Err(mem_err), Err(time_err), Err(cpu_err)) => (
            None,
            Some(OpnsenseSectionErrorDto::from_error(&OpnsenseError::Parse(
                format!("resources: {mem_err}; time: {time_err}; cpu: {cpu_err}"),
            ))),
        ),
        _ => {
            let mut dto = memory.unwrap_or_default();
            if let Ok(time) = time {
                dto.uptime = time.uptime;
                dto.loadavg = time.loadavg;
            }
            if let Ok(cpu) = cpu {
                dto.cpu_usage_percent = Some(cpu);
            }
            (Some(dto), None)
        }
    };
    Ok(OpnsenseSystemSectionDto {
        interfaces,
        interfaces_truncated,
        pf_states,
        pf_states_error,
        resources,
        resources_error,
    })
}

async fn fetch_vpn_section(api: &dyn OpnsenseApi) -> OpnsenseSectionDto<OpnsenseVpnSectionDto> {
    let wireguard = match api.wireguard_show().await {
        Ok(data) => OpnsenseVpnServiceDto::ok(data),
        Err(err) => OpnsenseVpnServiceDto::failed(&err),
    };
    let openvpn = match api.openvpn_sessions().await {
        Ok(data) => OpnsenseVpnServiceDto::ok(data),
        Err(err) => OpnsenseVpnServiceDto::failed(&err),
    };
    let ipsec = match api.ipsec_phase1().await {
        Ok(data) => OpnsenseVpnServiceDto::ok(data),
        Err(err) => OpnsenseVpnServiceDto::failed(&err),
    };
    let tailscale = match api.tailscale_status().await {
        Ok(mut data) => {
            // The service-running flag is a separate endpoint; its failure
            // leaves the field unknown rather than failing the section.
            if let Ok(running) = api.tailscale_service_status().await {
                data.service_running = running;
            }
            OpnsenseVpnServiceDto::ok(data)
        }
        Err(err) => OpnsenseVpnServiceDto::failed(&err),
    };
    OpnsenseSectionDto::ok(OpnsenseVpnSectionDto {
        wireguard,
        openvpn,
        ipsec,
        tailscale,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;
    use tokio::sync::Mutex as RequestMutex;

    use async_trait::async_trait;
    use tokio::sync::mpsc;
    use tokio::sync::mpsc::UnboundedReceiver;
    use tokio::sync::Notify;

    use crate::db::Database;
    use crate::opnsense::client::{
        FirmwareInfoPayload, OpnsenseConnection, PfRuleId, RuleSearchPage, RuleStat,
        SystemTimePayload,
    };
    use crate::opnsense::types::{
        CreateOpnsenseProfileRequest, OpnsenseInterfaceDto, OpnsensePfStatesDto,
        OpnsenseResourcesDto, OpnsenseRuleDto, OpnsenseStatusEvent, OpnsenseVpnSectionDto,
        OpnsenseWireguardInterfaceDto, OpnsenseWireguardStatusDto,
    };
    use crate::secrets::MemoryStore;

    // ---- Test harness (pattern: tests/mikrotik_version.rs) ---------------

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "verkkokyyla-opnsense-monitor-{name}-{}-{:016x}",
                std::process::id(),
                rand::random::<u64>()
            ));
            std::fs::create_dir(&dir).expect("unique temp dir");
            Self(dir)
        }

        fn db_file(&self) -> PathBuf {
            self.0.join("opnsense.db")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Scripted fake: static results per endpoint (cloned per call), with
    /// a call counter for refresh-cadence assertions.
    struct FakeApi {
        rules: Result<RuleSearchPage, OpnsenseError>,
        rule_requests: RequestMutex<usize>,
        rule_stats: Result<Vec<(String, RuleStat)>, OpnsenseError>,
        rule_ids: Result<Vec<PfRuleId>, OpnsenseError>,
        interfaces: Result<InterfaceInfoPage, OpnsenseError>,
        interface_entered: Option<Arc<Notify>>,
        interface_gate: Option<Arc<Notify>>,
        interface_stats: Result<Vec<OpnsenseInterfaceDto>, OpnsenseError>,
        pf_states: Result<OpnsensePfStatesDto, OpnsenseError>,
        resources: Result<OpnsenseResourcesDto, OpnsenseError>,
        system_time: Result<SystemTimePayload, OpnsenseError>,
        cpu_sample: Result<f64, OpnsenseError>,
        wireguard: Result<OpnsenseWireguardStatusDto, OpnsenseError>,
        openvpn: Result<Vec<super::super::types::OpnsenseOpenvpnSessionDto>, OpnsenseError>,
        ipsec: Result<Vec<super::super::types::OpnsenseIpsecSessionDto>, OpnsenseError>,
        tailscale: Result<super::super::types::OpnsenseTailscaleStatusDto, OpnsenseError>,
        tailscale_service: Result<Option<bool>, OpnsenseError>,
        firmware_info: Result<FirmwareInfoPayload, OpnsenseError>,
    }

    impl Default for FakeApi {
        fn default() -> Self {
            Self {
                rules: Ok(RuleSearchPage {
                    rules: Vec::new(),
                    total: 0,
                }),
                rule_requests: RequestMutex::new(0),
                rule_stats: Ok(Vec::new()),
                rule_ids: Ok(Vec::new()),
                interfaces: Ok(InterfaceInfoPage {
                    interfaces: Vec::new(),
                    total: 0,
                }),
                interface_entered: None,
                interface_gate: None,
                interface_stats: Ok(Vec::new()),
                pf_states: Ok(OpnsensePfStatesDto {
                    current: None,
                    max: None,
                }),
                resources: Ok(OpnsenseResourcesDto::default()),
                system_time: Ok(SystemTimePayload::default()),
                cpu_sample: Ok(0.0),
                wireguard: Ok(OpnsenseWireguardStatusDto {
                    interfaces: Vec::new(),
                }),
                openvpn: Ok(Vec::new()),
                ipsec: Ok(Vec::new()),
                tailscale: Ok(super::super::types::OpnsenseTailscaleStatusDto {
                    backend_state: None,
                    service_running: None,
                    hostname: None,
                    own_ips: Vec::new(),
                    peers: Vec::new(),
                }),
                tailscale_service: Ok(None),
                firmware_info: Ok(FirmwareInfoPayload::default()),
            }
        }
    }

    #[async_trait]
    impl OpnsenseApi for FakeApi {
        async fn get_system_status(&self) -> Result<(), OpnsenseError> {
            Ok(())
        }

        async fn search_rules(&self) -> Result<RuleSearchPage, OpnsenseError> {
            *self.rule_requests.lock().await += 1;
            self.rules.clone()
        }

        async fn rule_stats(&self) -> Result<Vec<(String, RuleStat)>, OpnsenseError> {
            self.rule_stats.clone()
        }

        async fn list_rule_ids(&self) -> Result<Vec<PfRuleId>, OpnsenseError> {
            self.rule_ids.clone()
        }

        async fn interfaces_info(&self) -> Result<InterfaceInfoPage, OpnsenseError> {
            if let Some(entered) = &self.interface_entered {
                entered.notify_one();
            }
            if let Some(gate) = &self.interface_gate {
                gate.notified().await;
            }
            self.interfaces.clone()
        }

        async fn interface_statistics(&self) -> Result<Vec<OpnsenseInterfaceDto>, OpnsenseError> {
            self.interface_stats.clone()
        }

        async fn pf_states(&self) -> Result<OpnsensePfStatesDto, OpnsenseError> {
            self.pf_states.clone()
        }

        async fn system_resources(&self) -> Result<OpnsenseResourcesDto, OpnsenseError> {
            self.resources.clone()
        }

        async fn system_time(&self) -> Result<SystemTimePayload, OpnsenseError> {
            self.system_time.clone()
        }

        async fn cpu_usage_sample(&self) -> Result<f64, OpnsenseError> {
            self.cpu_sample.clone()
        }

        async fn wireguard_show(&self) -> Result<OpnsenseWireguardStatusDto, OpnsenseError> {
            self.wireguard.clone()
        }

        async fn openvpn_sessions(
            &self,
        ) -> Result<Vec<super::super::types::OpnsenseOpenvpnSessionDto>, OpnsenseError> {
            self.openvpn.clone()
        }

        async fn ipsec_phase1(
            &self,
        ) -> Result<Vec<super::super::types::OpnsenseIpsecSessionDto>, OpnsenseError> {
            self.ipsec.clone()
        }

        async fn tailscale_status(
            &self,
        ) -> Result<super::super::types::OpnsenseTailscaleStatusDto, OpnsenseError> {
            self.tailscale.clone()
        }

        async fn tailscale_service_status(&self) -> Result<Option<bool>, OpnsenseError> {
            self.tailscale_service.clone()
        }

        async fn firmware_info(&self) -> Result<FirmwareInfoPayload, OpnsenseError> {
            self.firmware_info.clone()
        }

        async fn backup_revisions(
            &self,
        ) -> Result<Vec<super::super::types::OpnsenseRevisionDto>, OpnsenseError> {
            Ok(Vec::new())
        }

        async fn download_revision(&self, _id: &str) -> Result<Vec<u8>, OpnsenseError> {
            Ok(Vec::new())
        }

        async fn open_log_stream(
            &self,
        ) -> Result<crate::opnsense::client::SseByteStream, OpnsenseError> {
            Err(OpnsenseError::NotFound(
                "unused in monitor tests".to_owned(),
            ))
        }
    }

    fn factory_for(api: Arc<FakeApi>) -> crate::opnsense::types::OpnsenseApiFactory {
        Arc::new(move |_conn: OpnsenseConnection| {
            let api = Arc::clone(&api);
            Box::pin(async move { Ok(api as Arc<dyn OpnsenseApi>) })
        })
    }

    async fn manager_for(dir: &TempDir, api: Arc<FakeApi>) -> OpnsenseManager {
        let db = Database::connect(&dir.db_file()).await.expect("db");
        OpnsenseManager::new(db, Arc::new(MemoryStore::new()), factory_for(api))
    }

    async fn profile(manager: &OpnsenseManager, name: &str) -> i64 {
        let dto = manager
            .create_profile(&CreateOpnsenseProfileRequest {
                name: name.to_owned(),
                host: "fw.example.com".to_owned(),
                port: 443,
                allow_invalid_certs: false,
                ssh_port: 22,
            })
            .await
            .expect("profile");
        manager
            .set_credentials(dto.id, "key", "secret")
            .await
            .expect("credentials");
        dto.id
    }

    fn snapshot_sink() -> (OpnsenseSnapshotSink, UnboundedReceiver<OpnsenseSnapshotDto>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Arc::new(move |snapshot| {
                let _ = tx.send(snapshot);
            }),
            rx,
        )
    }

    fn status_sink() -> (OpnsenseStatusSink, UnboundedReceiver<OpnsenseStatusEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Arc::new(move |event| {
                let _ = tx.send(event);
            }),
            rx,
        )
    }

    fn rule(uuid: &str, description: &str) -> OpnsenseRuleDto {
        OpnsenseRuleDto {
            uuid: uuid.to_owned(),
            action: "pass".to_owned(),
            interface: "wan".to_owned(),
            description: description.to_owned(),
            source: "any".to_owned(),
            destination: "any".to_owned(),
            protocol: "any".to_owned(),
            enabled: true,
            packets: None,
            evaluations: None,
            bytes: None,
            states: None,
        }
    }

    fn stat(packets: i64) -> RuleStat {
        RuleStat {
            packets: Some(packets),
            evaluations: None,
            bytes: None,
            states: None,
        }
    }

    fn drain<T>(rx: &mut UnboundedReceiver<T>) -> Vec<T> {
        let mut out = Vec::new();
        while let Ok(item) = rx.try_recv() {
            out.push(item);
        }
        out
    }

    // ---- Rules section ---------------------------------------------------

    #[tokio::test]
    async fn rules_section_fetches_all_and_joins_counters_by_uuid() {
        let api = Arc::new(FakeApi {
            rules: Ok(RuleSearchPage {
                rules: vec![
                    rule("uuid-a", "allow lan"),
                    rule("uuid-b", "block bad"),
                    rule("uuid-c", "allow voip"),
                ],
                total: 3,
            }),
            rule_stats: Ok(vec![
                ("uuid-a".to_owned(), stat(12)),
                ("uuid-b".to_owned(), stat(0)),
            ]),
            ..Default::default()
        });

        let section = fetch_rules(api.as_ref()).await.expect("rules section");

        assert_eq!(*api.rule_requests.lock().await, 1, "single GET, no paging");
        assert_eq!(section.rules.len(), 3);
        assert!(!section.truncated);
        assert_eq!(section.total, 3);
        assert!(section.counters_error.is_none());
        // Hits join by UUID only: 12 and 0 are preserved, missing stays None.
        assert_eq!(section.rules[0].packets, Some(12));
        assert_eq!(section.rules[1].packets, Some(0));
        assert_eq!(section.rules[2].packets, None);
    }

    #[tokio::test]
    async fn rules_section_flags_truncation_when_rows_missing() {
        let api = Arc::new(FakeApi {
            rules: Ok(RuleSearchPage {
                rules: vec![rule("uuid-a", "allow lan"), rule("uuid-b", "block wan")],
                total: 3,
            }),
            ..Default::default()
        });
        let section = fetch_rules(api.as_ref()).await.expect("rules section");
        assert_eq!(section.rules.len(), 2);
        assert!(section.truncated, "withheld rows must be reported");
        assert_eq!(section.total, 3);
    }

    #[tokio::test]
    async fn interfaces_section_flags_truncation_when_rows_missing() {
        let iface = OpnsenseInterfaceDto {
            name: "igb0".to_owned(),
            description: None,
            identifier: None,
            enabled: true,
            link: Some("up".to_owned()),
            ipv4: Vec::new(),
            ipv6: Vec::new(),
            macaddr: None,
            mtu: None,
            media: None,
            lagg: None,
            rx_bytes: None,
            tx_bytes: None,
            rx_packets: None,
            tx_packets: None,
        };
        let api = Arc::new(FakeApi {
            interfaces: Ok(InterfaceInfoPage {
                interfaces: vec![iface],
                total: 2,
            }),
            ..Default::default()
        });
        let section = fetch_system(api.as_ref()).await.expect("system section");
        assert_eq!(section.interfaces.len(), 1);
        assert!(section.interfaces_truncated);
    }

    #[tokio::test]
    async fn rules_section_flags_truncation_at_ten_thousand() {
        let api = Arc::new(FakeApi {
            rules: Ok(RuleSearchPage {
                rules: (0..10_001)
                    .map(|n| rule(&format!("uuid-{n}"), "bulk"))
                    .collect(),
                total: 10_001,
            }),
            ..Default::default()
        });

        let section = fetch_rules(api.as_ref()).await.expect("rules section");

        assert_eq!(section.rules.len(), RULES_CAP);
        assert!(section.truncated, "cap cut must be flagged visible");
        assert_eq!(section.total, 10_001);
        assert_eq!(*api.rule_requests.lock().await, 1);
    }

    #[tokio::test]
    async fn rules_section_keeps_rules_when_counters_are_forbidden() {
        let api = Arc::new(FakeApi {
            rules: Ok(RuleSearchPage {
                rules: vec![rule("uuid-a", "allow lan")],
                total: 1,
            }),
            rule_stats: Err(OpnsenseError::Forbidden),
            ..Default::default()
        });

        let dto = fetch_rules_section(api.as_ref()).await;

        assert!(dto.ok);
        let section = dto.data.expect("rules data");
        assert_eq!(section.rules.len(), 1, "definitions must survive 403");
        let counters_error = section.counters_error.expect("counters error");
        assert_eq!(counters_error.kind, "forbidden");
        assert_eq!(section.rules[0].packets, None);
    }

    // ---- VPN section -------------------------------------------------------

    #[tokio::test]
    async fn vpn_section_marks_tailscale_unavailable_but_keeps_wireguard() {
        let api = Arc::new(FakeApi {
            wireguard: Ok(OpnsenseWireguardStatusDto {
                interfaces: vec![OpnsenseWireguardInterfaceDto {
                    name: "wg0".to_owned(),
                    peers: Vec::new(),
                }],
            }),
            tailscale: Err(OpnsenseError::NotFound(
                "tailscale plugin not installed".to_owned(),
            )),
            ..Default::default()
        });

        let dto = fetch_vpn_section(api.as_ref()).await;
        let OpnsenseSectionDto {
            data: Some(vpn), ..
        } = dto
        else {
            panic!("vpn section must stay ok");
        };
        let OpnsenseVpnSectionDto {
            wireguard,
            tailscale,
            ..
        } = vpn;
        assert_eq!(wireguard.state, "ok");
        assert_eq!(wireguard.data.unwrap().interfaces.len(), 1);
        assert_eq!(tailscale.state, "unavailable");
        assert!(tailscale.data.is_none());
    }

    // ---- Monitor lifecycle -------------------------------------------------

    #[tokio::test]
    async fn start_emits_snapshot_and_started_stop_cleans_up() {
        let dir = TempDir::new("lifecycle");
        let api = Arc::new(FakeApi::default());
        let manager = manager_for(&dir, api).await;
        let id = profile(&manager, "fw").await;
        let monitors = OpnsenseMonitorManager::new(manager);
        let (on_snapshot, mut snapshots) = snapshot_sink();
        let (on_status, mut statuses) = status_sink();
        // Pause only AFTER start: under paused time the runtime fast-forwards
        // sqlx's acquire timeout past the blocking connect when start and the
        // first tick race for the pool's single open connection (PoolTimedOut
        // flake on CI).
        monitors
            .start(id, on_snapshot, on_status)
            .await
            .expect("start");
        tokio::time::pause();

        let first = snapshots.recv().await.expect("first snapshot");
        assert_eq!(first.profile_id, id);
        assert!(!first.at.is_empty());
        assert!(
            first.rules.as_ref().is_some_and(|rules| rules.ok),
            "rules fetched on the first tick"
        );
        assert!(first.system.ok);
        assert!(first.vpn.ok);

        let events = drain(&mut statuses);
        assert!(
            events.contains(&OpnsenseStatusEvent::Started { profile_id: id }),
            "Started must fire after the first snapshot, got {events:?}"
        );

        assert_eq!(
            monitors.list_active().await,
            vec![OpnsenseActiveMonitorDto { profile_id: id }]
        );

        monitors.stop(id).await.expect("stop");
        let events = drain(&mut statuses);
        assert!(
            events.contains(&OpnsenseStatusEvent::Stopped { profile_id: id }),
            "Stopped must fire at exit, got {events:?}"
        );
        assert!(monitors.list_active().await.is_empty());
        assert!(matches!(
            monitors.stop(id).await,
            Err(OpnsenseManagerError::NoActiveMonitor)
        ));
    }
    #[tokio::test]
    async fn stop_cancels_an_in_flight_get_without_waiting_for_its_timeout() {
        let dir = TempDir::new("cancel-inflight");
        let entered = Arc::new(Notify::new());
        let gate = Arc::new(Notify::new());
        let api = Arc::new(FakeApi {
            interface_entered: Some(Arc::clone(&entered)),
            interface_gate: Some(gate),
            ..Default::default()
        });
        let manager = manager_for(&dir, api).await;
        let id = profile(&manager, "fw").await;
        let monitors = OpnsenseMonitorManager::new(manager.clone());
        let (on_snapshot, mut snapshots) = snapshot_sink();
        let (on_status, _statuses) = status_sink();
        monitors
            .start(id, on_snapshot, on_status)
            .await
            .expect("start");
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .expect("GET started");
        tokio::time::timeout(Duration::from_secs(1), monitors.stop(id))
            .await
            .expect("stop cancels in-flight GET")
            .expect("stop succeeded");
        assert!(
            snapshots.try_recv().is_err(),
            "incomplete tick was not emitted"
        );
        assert!(!manager.activity().lock().await.is_active(id));
    }

    #[tokio::test]
    async fn second_start_of_same_profile_is_rejected() {
        let dir = TempDir::new("already-monitoring");
        let api = Arc::new(FakeApi::default());
        let manager = manager_for(&dir, api).await;
        let id = profile(&manager, "fw").await;
        let monitors = OpnsenseMonitorManager::new(manager);

        let (on_snapshot, _snapshots) = snapshot_sink();
        let (on_status, _statuses) = status_sink();
        monitors
            .start(id, on_snapshot.clone(), on_status.clone())
            .await
            .expect("first start");

        let result = monitors.start(id, on_snapshot, on_status).await;
        assert!(matches!(
            result,
            Err(OpnsenseManagerError::AlreadyMonitoring(profile)) if profile == id
        ));

        monitors.stop(id).await.expect("stop");
    }

    #[tokio::test]
    async fn rules_section_refetches_only_every_third_tick() {
        let dir = TempDir::new("rules-cadence");
        let api = Arc::new(FakeApi::default());
        let manager = manager_for(&dir, api.clone()).await;
        let id = profile(&manager, "fw").await;
        let monitors = OpnsenseMonitorManager::with_cadence(
            manager,
            Duration::from_secs(5),
            RULES_EVERY_TICKS,
        );
        let (on_snapshot, mut snapshots) = snapshot_sink();
        let (on_status, _statuses) = status_sink();
        // Pause only AFTER start: under paused time the runtime fast-forwards
        // sqlx's acquire timeout past the blocking connect when start and the
        // first tick race for the pool's single open connection (PoolTimedOut
        // flake on CI).
        monitors
            .start(id, on_snapshot, on_status)
            .await
            .expect("start");
        tokio::time::pause();

        let mut seen = Vec::new();
        for _ in 0..4 {
            seen.push(snapshots.recv().await.expect("snapshot"));
        }
        // Ticks 0..3 fetch rules on ticks 0 and 3. Intervening snapshots
        // omit unchanged rules instead of re-serializing the table.
        assert_eq!(*api.rule_requests.lock().await, 2);
        assert!(seen[0].rules.as_ref().is_some_and(|rules| rules.ok));
        assert!(seen[1].rules.is_none());
        assert!(seen[2].rules.is_none());
        assert_eq!(seen[3].rules, seen[0].rules);

        monitors.stop(id).await.expect("stop");
    }
}
