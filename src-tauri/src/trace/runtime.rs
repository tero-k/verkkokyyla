use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{watch, Mutex, Semaphore};
use tokio::time::timeout;

use super::manager::TraceManager;
use super::types::{
    StoppedTraceDto, TraceEvent, TraceResolver, TraceStatusEvent, TraceStatusSink, TraceStream,
};
use crate::db::{now_rfc3339, Database, TraceHopRow};

struct RunState {
    hops: Vec<TraceHopRow>,
    cache: HashMap<IpAddr, Option<String>>,
    pending: HashSet<IpAddr>,
    hops_by_address: HashMap<IpAddr, Vec<i64>>,
}

pub async fn run_trace(
    mut stream: Box<dyn TraceStream>,
    ctx: TraceRunContext,
) -> Result<StoppedTraceDto, super::types::TraceError> {
    let TraceRunContext {
        manager,
        db,
        trace_id,
        resolved_ip,
        resolver,
        on_event,
        on_status,
        mut stop_rx,
    } = ctx;
    let state = Arc::new(Mutex::new(RunState {
        hops: Vec::new(),
        cache: HashMap::new(),
        pending: HashSet::new(),
        hops_by_address: HashMap::new(),
    }));
    let semaphore = Arc::new(Semaphore::new(4));
    let mut reached_target = false;
    let mut cancelled = false;

    loop {
        tokio::select! {
            _ = stop_rx.changed() => { cancelled = true; break; }
            next = stream.next() => match next {
                Ok(Some(raw)) => {
                    let address = raw.address.as_ref().and_then(|value| value.parse::<IpAddr>().ok());
                    if address.is_some_and(|addr| addr == resolved_ip) { reached_target = true; }
                    let hop = raw.hop as i64;
                    let at = now_rfc3339();
                    let rtt1_ms = raw.rtts.first().copied().flatten();
                    let rtt2_ms = raw.rtts.get(1).copied().flatten();
                    let rtt3_ms = raw.rtts.get(2).copied().flatten();
                    let mut spawn_address = None;
                    let mut emit_hostname = None;
                    let row_hostname = if let Some(addr) = address {
                        let mut guard = state.lock().await;
                        guard.hops_by_address.entry(addr).or_default().push(hop);
                        match guard.cache.get(&addr).cloned() {
                            Some(hostname) => { emit_hostname = Some((addr, hostname.clone())); hostname }
                            None if guard.pending.insert(addr) => { spawn_address = Some(addr); None }
                            None => None,
                        }
                    } else { None };
                    {
                        let mut guard = state.lock().await;
                        guard.hops.push(TraceHopRow { trace_id, hop, address: raw.address.clone(), hostname: row_hostname.clone(), rtt1_ms, rtt2_ms, rtt3_ms, annotation: raw.annotation.clone(), at: at.clone() });
                    }
                    (on_event)(TraceEvent::Hop { hop: raw.hop, address: raw.address.clone(), rtt1_ms, rtt2_ms, rtt3_ms, annotation: raw.annotation, at });
                    if let Some((addr, hostname)) = emit_hostname {
                        let _ = db.update_trace_hop_hostname(trace_id, hop, hostname.as_deref()).await;
                        (on_event)(TraceEvent::Hostname { hop: raw.hop, address: addr.to_string(), hostname });
                    }
                    if let Some(addr) = spawn_address {
                        spawn_hostname_lookup(trace_id, addr, Arc::clone(&state), Arc::clone(&db), Arc::clone(&resolver), Arc::clone(&on_event), Arc::clone(&semaphore)).await;
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    on_status(TraceStatusEvent::Error { message: format!("{err:?}") });
                    let ended_at = now_rfc3339();
                    let hops = { state.lock().await.hops.clone() };
                    let dto = StoppedTraceDto { trace_id, hop_count: hops.len() as u64, ended_at: ended_at.clone() };
                    db.complete_trace_with_hops(trace_id, &ended_at, "error", reached_target, &hops).await?;
                    manager.clear_active(trace_id).await;
                    return Ok(dto);
                }
            }
        }
    }

    let ended_at = now_rfc3339();
    let hops = { state.lock().await.hops.clone() };
    let dto = StoppedTraceDto {
        trace_id,
        hop_count: hops.len() as u64,
        ended_at: ended_at.clone(),
    };
    let status = if cancelled { "cancelled" } else { "completed" };
    db.complete_trace_with_hops(trace_id, &ended_at, status, reached_target, &hops)
        .await?;
    on_status(match cancelled {
        true => TraceStatusEvent::Cancelled {
            trace_id,
            hop_count: dto.hop_count,
        },
        false => TraceStatusEvent::Completed {
            trace_id,
            hop_count: dto.hop_count,
            reached_target,
        },
    });
    manager.clear_active(trace_id).await;
    Ok(dto)
}

pub struct TraceRunContext {
    pub manager: TraceManager,
    pub db: Arc<Database>,
    pub trace_id: i64,
    pub resolved_ip: IpAddr,
    pub resolver: TraceResolver,
    pub on_event: Arc<dyn Fn(TraceEvent) + Send + Sync>,
    pub on_status: TraceStatusSink,
    pub stop_rx: watch::Receiver<bool>,
}

async fn spawn_hostname_lookup(
    trace_id: i64,
    address: IpAddr,
    state: Arc<Mutex<RunState>>,
    db: Arc<Database>,
    resolver: TraceResolver,
    on_event: Arc<dyn Fn(TraceEvent) + Send + Sync>,
    semaphore: Arc<Semaphore>,
) {
    let permit = match semaphore.clone().acquire_owned().await {
        Ok(permit) => permit,
        Err(_) => return,
    };
    tokio::spawn(async move {
        let _permit = permit;
        let mut lookup = (resolver)(address);
        let hostname = match timeout(Duration::from_secs(2), async { (&mut lookup).await }).await {
            Ok(name) => name,
            Err(_) => lookup.await,
        };
        emit_hostname_event(trace_id, address, hostname, state, db, on_event).await;
    });
}

async fn emit_hostname_event(
    trace_id: i64,
    address: IpAddr,
    hostname: Option<String>,
    state: Arc<Mutex<RunState>>,
    db: Arc<Database>,
    on_event: Arc<dyn Fn(TraceEvent) + Send + Sync>,
) {
    let hops = {
        let mut guard = state.lock().await;
        let hops = guard
            .hops_by_address
            .get(&address)
            .cloned()
            .unwrap_or_default();
        let mut updated = Vec::new();
        for hop in hops {
            if let Some(row) = guard.hops.iter_mut().find(|row| {
                row.hop == hop
                    && row
                        .address
                        .as_ref()
                        .and_then(|value| value.parse::<IpAddr>().ok())
                        == Some(address)
                    && row.hostname.is_none()
            }) {
                if hostname.is_some() {
                    row.hostname = hostname.clone();
                }
                updated.push(hop);
            }
        }
        updated
    };

    for hop in hops {
        let _ = db
            .update_trace_hop_hostname(trace_id, hop, hostname.as_deref())
            .await;
        on_event(TraceEvent::Hostname {
            hop: hop as u32,
            address: address.to_string(),
            hostname: hostname.clone(),
        });
    }
}
