//! The MikroTik polling runtime: a 5s tick loop inside `tokio::select!` with
//! the manager's cancel watch. A tick's success is defined by
//! `/system/resource` + `/interface` ONLY (the CORE). Enrichers (health,
//! stats-detail, ethernet monitor, VLAN) never gate the tick: they run under
//! a per-tick time budget (< the 5s tick) with bounded concurrency
//! (semaphore of 4, mirroring `trace/runtime.rs`), and on failure the tick
//! still emits and persists with the LAST SUCCESSFUL enricher payload (null
//! only when that enricher never succeeded this session) plus a `warning`
//! note. CORE failure → warning status event, NO snapshot row, streak
//! increment; 3 consecutive CORE failures → terminal error status + stop.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{watch, Semaphore};
use tokio::time::{timeout, Instant, MissedTickBehavior};

use super::manager::MikrotikManager;
use super::types::{
    MikrotikApi, MikrotikEvent, MikrotikInterfaceDto, MikrotikResourcesDto, MikrotikSensorDto,
    MikrotikSnapshotPayload, MikrotikStatusEvent, MikrotikStatusSink, MikrotikStoppedDto,
    RunVersionProbe, VersionProbeArgs,
};
use crate::db::{now_rfc3339, Database, MikrotikSessionVersionStatus, MikrotikSnapshotRow};
use crate::mikrotik::error::MikrotikError;
use crate::mikrotik::parse::{
    BridgeVlanDto, EthernetMonitorDto, EthernetStatsDto, InterfaceDto, ResourceDto, SensorDto,
    SensorKind, VlanDto,
};

pub const TICK: Duration = Duration::from_secs(5);
pub const HEALTH_EVERY: u64 = 2;
pub const STATS_DETAIL_EVERY: u64 = 6;
pub const VLAN_EVERY: u64 = 12;
pub const MAX_CORE_FAILURES: u32 = 3;
/// Per-tick enricher time budget — strictly below the 5s tick so a hung
/// enricher can never delay the next core snapshot.
pub const ENRICHER_BUDGET: Duration = Duration::from_secs(3);

/// Last-known enricher payloads for this session. `None` = that enricher has
/// never succeeded this session.
#[derive(Default)]
struct EnricherState {
    sensors: Option<Vec<MikrotikSensorDto>>,
    sensors_supported: bool,
    interfaces: Option<Vec<MikrotikInterfaceDto>>,
    vlans: Option<Vec<VlanDto>>,
    bridge_vlans: Option<Vec<BridgeVlanDto>>,
}

impl EnricherState {
    fn new() -> Self {
        Self {
            sensors_supported: true,
            ..Self::default()
        }
    }
}

/// Per-interface byte counters from the previous tick for rate computation.
/// Rates are `8 * delta_bytes / elapsed_secs` from a monotonic Instant —
/// NEVER the assumed 5s interval.
#[derive(Default)]
struct RateState {
    last: HashMap<String, (u64, u64, Instant)>,
}
/// Compute per-interface rx/tx bit rates from ACTUAL elapsed time. A
/// wrap/reset (new < old) yields `None` for that counter this tick.
fn compute_rates(
    state: &mut RateState,
    name: &str,
    rx: Option<u64>,
    tx: Option<u64>,
    now: Instant,
) -> (Option<f64>, Option<f64>) {
    let (Some(rx), Some(tx)) = (rx, tx) else {
        state.last.remove(name);
        return (None, None);
    };
    let prev = state.last.insert(name.to_owned(), (rx, tx, now));
    let Some((prx, ptx, at)) = prev else {
        return (None, None);
    };
    let secs = at.elapsed().as_secs_f64();
    if secs <= 0.0 {
        return (None, None);
    }
    let rx_rate = (rx >= prx).then(|| 8.0 * (rx - prx) as f64 / secs);
    let tx_rate = (tx >= ptx).then(|| 8.0 * (tx - ptx) as f64 / secs);
    (rx_rate, tx_rate)
}

/// Drop interfaces that disappeared since the last tick.
fn prune_rates(state: &mut RateState, present: &std::collections::HashSet<String>) {
    state.last.retain(|name, _| present.contains(name));
}

fn sensor_kind(kind: &SensorKind) -> &'static str {
    match kind {
        SensorKind::Temperature => "temperature",
        SensorKind::Fan => "fan",
        SensorKind::Voltage => "voltage",
        SensorKind::Other => "other",
    }
}

fn to_sensor_dtos(sensors: Vec<SensorDto>) -> Vec<MikrotikSensorDto> {
    sensors
        .into_iter()
        .map(|sensor| MikrotikSensorDto {
            name: sensor.name,
            value: sensor.value,
            unit: sensor.unit,
            kind: sensor_kind(&sensor.kind).to_owned(),
        })
        .collect()
}

fn base_to_dto(
    iface: InterfaceDto,
    (rx_rate, tx_rate): (Option<f64>, Option<f64>),
) -> MikrotikInterfaceDto {
    MikrotikInterfaceDto {
        name: iface.name,
        iface_type: iface.iface_type,
        running: iface.running,
        disabled: iface.disabled,
        rx_byte: iface.rx_byte,
        tx_byte: iface.tx_byte,
        rx_packet: iface.rx_packet,
        tx_packet: iface.tx_packet,
        tx_queue_drop: iface.tx_queue_drop,
        link_downs: iface.link_downs,
        rx_error: iface.rx_error,
        tx_error: iface.tx_error,
        rx_drop: iface.rx_drop,
        rx_error_events: iface.rx_error_events,
        tx_error_events: iface.tx_error_events,
        rx_fcs_error: iface.rx_fcs_error,
        rx_align_error: iface.rx_align_error,
        tx_collision: iface.tx_collision,
        tx_drop: iface.tx_drop,
        rate: iface.rate,
        full_duplex: iface.full_duplex,
        rx_bits_per_second: rx_rate,
        tx_bits_per_second: tx_rate,
    }
}
/// Merge per-interface detail counters (stats-detail print) into the wire
/// DTOs by name.
fn merge_stats_detail(wire: &mut [MikrotikInterfaceDto], detail: Vec<InterfaceDto>) {
    for d in detail {
        let Some(target) = wire.iter_mut().find(|w| w.name == d.name) else {
            continue;
        };
        for (slot, value) in [
            (&mut target.rx_error, d.rx_error),
            (&mut target.tx_error, d.tx_error),
            (&mut target.rx_drop, d.rx_drop),
            (&mut target.link_downs, d.link_downs),
            (&mut target.rx_error_events, d.rx_error_events),
            (&mut target.tx_error_events, d.tx_error_events),
            (&mut target.rx_fcs_error, d.rx_fcs_error),
            (&mut target.rx_align_error, d.rx_align_error),
            (&mut target.tx_collision, d.tx_collision),
            (&mut target.tx_drop, d.tx_drop),
        ] {
            if value.is_some() {
                *slot = value;
            }
        }
    }
}

/// Merge DRIVER-level counters from the ethernet stats print BY NAME,
/// matching on `default-name` when ether ports were renamed.
fn merge_ethernet_stats(wire: &mut [MikrotikInterfaceDto], stats: Vec<EthernetStatsDto>) {
    for stat in stats {
        let idx = wire
            .iter()
            .position(|w| w.name == stat.name)
            .or_else(|| {
                stat.default_name
                    .as_deref()
                    .and_then(|d| wire.iter().position(|w| w.name == d))
            });
        let Some(i) = idx else { continue };
        let target = &mut wire[i];
        for (slot, value) in [
            (&mut target.rx_error_events, stat.rx_error_events),
            (&mut target.tx_error_events, stat.tx_error_events),
            (&mut target.rx_fcs_error, stat.rx_fcs_error),
            (&mut target.rx_align_error, stat.rx_align_error),
            (&mut target.tx_collision, stat.tx_collision),
            (&mut target.tx_drop, stat.tx_drop),
        ] {
            if value.is_some() {
                *slot = value;
            }
        }
    }
}

/// Merge negotiated rate/duplex from ethernet monitor into the wire DTOs.
fn merge_monitors(
    wire: &mut [MikrotikInterfaceDto],
    monitors: Vec<(String, Vec<EthernetMonitorDto>)>,
) {
    for (requested, results) in monitors {
        for monitor in results {
            let idx = wire
                .iter()
                .position(|w| w.name == monitor.name || w.name == requested);
            let Some(i) = idx else { continue };
            if monitor.rate.is_some() {
                wire[i].rate = monitor.rate;
            }
            if monitor.full_duplex.is_some() {
                wire[i].full_duplex = monitor.full_duplex;
            }
        }
    }
}
/// RouterOS marks unsupported endpoints with 404 or a 400/406
/// no-such-command body (both surfaced by todo 2/4 error helpers).
fn is_unsupported(err: &MikrotikError) -> bool {
    err.is_no_such_command() || matches!(err, MikrotikError::Api { status: 404, .. })
}

enum HealthOutcome {
    Succeeded(Vec<SensorDto>),
    Unsupported,
    Failed(String),
}

async fn run_health(api: &Arc<dyn MikrotikApi>) -> HealthOutcome {
    match timeout(ENRICHER_BUDGET, api.get_health()).await {
        Ok(Ok(sensors)) => HealthOutcome::Succeeded(sensors),
        Ok(Err(err)) if is_unsupported(&err) => HealthOutcome::Unsupported,
        Ok(Err(err)) => HealthOutcome::Failed(format!("health: {err}")),
        Err(_) => HealthOutcome::Failed("health: timed out".to_owned()),
    }
}

struct StatsEnrich {
    detail: Vec<InterfaceDto>,
    ether_stats: Vec<EthernetStatsDto>,
    monitors: Vec<(String, Vec<EthernetMonitorDto>)>,
}

/// Stats-detail + driver ethernet stats + per-interface monitors, bounded by
/// a semaphore of 4 and the per-tick time budget.
async fn run_stats_enrich(
    api: Arc<dyn MikrotikApi>,
    targets: Vec<String>,
) -> Result<StatsEnrich, String> {
    let (detail, ether_stats) = tokio::join!(
        timeout(ENRICHER_BUDGET, api.get_interface_stats_detail()),
        timeout(ENRICHER_BUDGET, api.get_ethernet_stats()),
    );
    let detail = detail
        .map_err(|_| "stats-detail: timed out".to_owned())?
        .map_err(|err| format!("stats-detail: {err}"))?;
    let ether_stats = ether_stats
        .map_err(|_| "ethernet-stats: timed out".to_owned())?
        .map_err(|err| format!("ethernet-stats: {err}"))?;

    let semaphore = Arc::new(Semaphore::new(4));
    let mut monitors = Vec::new();
    for target in targets {
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "ethernet-monitor: semaphore closed".to_owned())?;
        let api = Arc::clone(&api);
        let result = timeout(ENRICHER_BUDGET, api.get_ethernet_monitor(&target)).await;
        drop(permit);
        match result {
            Ok(Ok(values)) => monitors.push((target, values)),
            Ok(Err(err)) => return Err(format!("ethernet-monitor {target}: {err}")),
            Err(_) => return Err(format!("ethernet-monitor {target}: timed out")),
        }
    }
    Ok(StatsEnrich {
        detail,
        ether_stats,
        monitors,
    })
}

async fn run_vlans(
    api: &Arc<dyn MikrotikApi>,
) -> Result<(Vec<VlanDto>, Vec<BridgeVlanDto>), String> {
    let (vlans, bridge_vlans) = tokio::join!(
        timeout(ENRICHER_BUDGET, api.get_vlans()),
        timeout(ENRICHER_BUDGET, api.get_bridge_vlans()),
    );
    let vlans = vlans
        .map_err(|_| "vlans: timed out".to_owned())?
        .map_err(|err| format!("vlans: {err}"))?;
    let bridge_vlans = bridge_vlans
        .map_err(|_| "bridge-vlans: timed out".to_owned())?
        .map_err(|err| format!("bridge-vlans: {err}"))?;
    Ok((vlans, bridge_vlans))
}
fn core_failure_message(
    resource: &Result<ResourceDto, MikrotikError>,
    interfaces: &Result<Vec<InterfaceDto>, MikrotikError>,
) -> String {
    match (resource, interfaces) {
        (Err(a), Err(b)) => format!("resource: {a}; interfaces: {b}"),
        (Err(a), _) => format!("resource fetch failed: {a}"),
        (_, Err(b)) => format!("interface fetch failed: {b}"),
        _ => "core fetch failed".to_owned(),
    }
}

fn snapshot_row(payload: &MikrotikSnapshotPayload) -> MikrotikSnapshotRow {
    let resources = payload.resources.as_ref();
    MikrotikSnapshotRow {
        id: 0,
        session_id: payload.session_id,
        at: payload.at.clone(),
        cpu_load: resources.and_then(|r| r.cpu_load),
        mem_used_bytes: resources.and_then(|r| r.mem_used_bytes).map(|v| v as i64),
        mem_total_bytes: resources.and_then(|r| r.mem_total_bytes).map(|v| v as i64),
        uptime: resources.and_then(|r| r.uptime.clone()),
        warning: payload.warning.clone(),
        sensors_json: payload
            .sensors
            .as_ref()
            .and_then(|s| serde_json::to_string(s).ok()),
        interfaces_json: serde_json::to_string(&payload.interfaces).ok(),
        vlans_json: payload.vlans.as_ref().and_then(|v| serde_json::to_string(v).ok()),
        bridge_vlans_json: payload
            .bridge_vlans
            .as_ref()
            .and_then(|v| serde_json::to_string(v).ok()),
    }
}

pub struct MikrotikRunContext {
    pub manager: MikrotikManager,
    pub db: Arc<Database>,
    pub api: Arc<dyn MikrotikApi>,
    pub session_id: i64,
    pub profile_id: i64,
    pub on_event: Arc<dyn Fn(MikrotikEvent) + Send + Sync>,
    pub on_status: MikrotikStatusSink,
    pub stop_rx: watch::Receiver<bool>,
    /// Integration point owned by todo 6: the version/firmware probe. Invoked
    /// once per session start; MUST spawn its own cancellable task and never
    /// gate the 5s polling loop. Todo 5 does not implement it.
    pub run_version_probe: Option<RunVersionProbe>,
}
pub async fn run_mikrotik_session(
    ctx: MikrotikRunContext,
) -> Result<MikrotikStoppedDto, super::types::MikrotikManagerError> {
    let MikrotikRunContext {
        manager,
        db,
        api,
        session_id,
        profile_id,
        on_event,
        on_status,
        mut stop_rx,
        run_version_probe,
    } = ctx;

    (on_status)(MikrotikStatusEvent::Started {
        session_id,
        profile_id,
    });
    // Todo 6 wires the version/firmware probe here (separate spawned,
    // cancellable task — never gates the loop below).
    if let Some(probe) = &run_version_probe {
        probe(VersionProbeArgs {
            session_id,
            profile_id,
            db: Arc::clone(&db),
            manager: manager.clone(),
            api: Arc::clone(&api),
            on_status: Arc::clone(&on_status),
            stop_rx: stop_rx.clone(),
        });
    }

    let mut interval = tokio::time::interval(TICK);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut tick: u64 = 0;
    let mut core_failures = 0u32;
    let mut snapshot_count = 0u64;
    let mut resource_identity_persisted = false;
    let mut cancelled = false;
    let mut rates = RateState::default();
    let mut enrichers = EnricherState::new();

    loop {
        tokio::select! {
            _ = stop_rx.changed() => { cancelled = true; break; }
            _ = interval.tick() => {}
        }
        tick += 1;

        // CORE: resource + interfaces in parallel via tokio::join!. A tick's
        // success is defined by these two ONLY.
        let core = tokio::join!(api.get_resource(), api.get_interfaces());
        let (resource, interfaces) = match core {
            (Ok(resource), Ok(interfaces)) => (resource, interfaces),
            (resource, interfaces) => {
                core_failures += 1;
                let message = core_failure_message(&resource, &interfaces);
                (on_status)(MikrotikStatusEvent::Warning {
                    session_id,
                    source: "core".to_owned(),
                    message: message.clone(),
                });
                if core_failures >= MAX_CORE_FAILURES {
                    manager.clear_active(session_id).await;
                    (on_status)(MikrotikStatusEvent::Error {
                        session_id,
                        message,
                    });
                    break;
                }
                continue;
            }
        };
        core_failures = 0;
        let now = Instant::now();
        let at = now_rfc3339();

        // FIRST successful resource sample persists board/version/architecture
        // onto the session row so history renders them.
        if !resource_identity_persisted {
            resource_identity_persisted = true;
            let loaded = db.load_mikrotik_session(session_id).await?;
            db.set_mikrotik_session_version_status(
                session_id,
                &MikrotikSessionVersionStatus {
                    board_name: resource.board_name.clone(),
                    routeros_version: resource.version.clone(),
                    architecture_name: resource.architecture_name.clone(),
                    update_status_json: loaded.session.update_status_json,
                    firmware_status_json: loaded.session.firmware_status_json,
                },
            )
            .await?;
        }
        // Base wire DTOs with rates from ACTUAL elapsed time; disappeared
        // interfaces are dropped from the rate state.
        let mut wire: Vec<MikrotikInterfaceDto> = Vec::with_capacity(interfaces.len());
        let mut present = std::collections::HashSet::new();
        for iface in interfaces {
            present.insert(iface.name.clone());
            let name = iface.name.clone();
            let bit_rates = compute_rates(&mut rates, &name, iface.rx_byte, iface.tx_byte, now);
            wire.push(base_to_dto(iface, bit_rates));
        }
        prune_rates(&mut rates, &present);

        // Bounded monitor targets: running ether interfaces only (skip
        // non-ethernet and disabled interfaces).
        let monitor_targets: Vec<String> = wire
            .iter()
            .filter(|w| {
                w.iface_type.as_deref() == Some("ether")
                    && w.running == Some(true)
                    && w.disabled != Some(true)
            })
            .map(|w| w.name.clone())
            .collect();

        let want_health = tick % HEALTH_EVERY == 0;
        let want_stats = tick % STATS_DETAIL_EVERY == 0;
        let want_vlans = tick == 1 || tick % VLAN_EVERY == 0;

        let api_stats = Arc::clone(&api);
        let (health, stats, vlans) = tokio::join!(
            async {
                if want_health {
                    Some(run_health(&api).await)
                } else {
                    None
                }
            },
            async {
                if want_stats {
                    Some(run_stats_enrich(api_stats, monitor_targets).await)
                } else {
                    None
                }
            },
            async {
                if want_vlans {
                    Some(run_vlans(&api).await)
                } else {
                    None
                }
            },
        );

        // SPLIT FAILURE SEMANTICS: enricher failures never gate the tick —
        // last-known payload is retained (null only before the first success)
        // and the failure note lands in the snapshot's `warning` field. No
        // core-failure streak increment.
        let mut warnings: Vec<String> = Vec::new();
        if let Some(outcome) = health {
            match outcome {
                HealthOutcome::Succeeded(sensors) => {
                    enrichers.sensors_supported = true;
                    enrichers.sensors = Some(to_sensor_dtos(sensors));
                }
                HealthOutcome::Unsupported => {
                    enrichers.sensors_supported = false;
                    enrichers.sensors = Some(Vec::new());
                }
                HealthOutcome::Failed(note) => warnings.push(note),
            }
        }
        if let Some(Ok((vlans, bridge_vlans))) = vlans {
            enrichers.vlans = Some(vlans);
            enrichers.bridge_vlans = Some(bridge_vlans);
        } else if let Some(Err(note)) = vlans {
            warnings.push(note);
        }
        let vlans_out = if want_vlans { enrichers.vlans.clone() } else { None };
        let bridge_out = if want_vlans {
            enrichers.bridge_vlans.clone()
        } else {
            None
        };
        let stats_failed = matches!(stats, Some(Err(_)));
        if let Some(result) = stats {
            match result {
                Ok(enrich) => {
                    merge_stats_detail(&mut wire, enrich.detail);
                    merge_ethernet_stats(&mut wire, enrich.ether_stats);
                    merge_monitors(&mut wire, enrich.monitors);
                    enrichers.interfaces = Some(wire.clone());
                }
                Err(note) => warnings.push(note),
            }
        }
        let interfaces_out = match (want_stats, stats_failed, &enrichers.interfaces) {
            (true, true, Some(last_known)) => last_known.clone(),
            _ => wire,
        };

        let warning = match warnings.is_empty() {
            true => None,
            false => Some(warnings.join("; ")),
        };
        let payload = MikrotikSnapshotPayload {
            session_id,
            at,
            resources: Some(MikrotikResourcesDto {
                cpu_load: resource.cpu_load,
                mem_used_bytes: resource.mem_used_bytes,
                mem_total_bytes: resource.mem_total_bytes,
                uptime: resource.uptime,
                board_name: resource.board_name,
                routeros_version: resource.version,
                architecture_name: resource.architecture_name,
            }),
            sensors: enrichers.sensors.clone(),
            sensors_supported: enrichers.sensors_supported,
            interfaces: interfaces_out,
            vlans: vlans_out,
            bridge_vlans: bridge_out,
            warning,
        };
        db.insert_mikrotik_snapshot(&snapshot_row(&payload)).await?;
        snapshot_count += 1;
        (on_event)(MikrotikEvent::Snapshot(payload));
    }

    let ended_at = now_rfc3339();
    let status = if cancelled { "cancelled" } else { "error" };
    db.complete_mikrotik_session(session_id, &ended_at, status).await?;
    if cancelled {
        (on_status)(MikrotikStatusEvent::Cancelled {
            session_id,
            snapshot_count,
        });
    }
    manager.clear_active(session_id).await;
    Ok(MikrotikStoppedDto {
        session_id,
        snapshot_count,
        ended_at,
        status: status.to_owned(),
    })
}
