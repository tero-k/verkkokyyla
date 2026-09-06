use std::{
    collections::{HashMap, HashSet},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use futures_util::future::BoxFuture;
use serde::Serialize;
use tokio::{net::TcpStream, sync::Semaphore, time::timeout};

use crate::engine::{EngineError, PingEngine, ProbeResult, PING_PAYLOAD_BYTES};
use crate::scan::arp::{read_arp_table, ArpError};
use crate::scan::cidr::expand_ipv4_cidr;
use crate::scan::rdns::resolve_many;
use crate::scan::ScanError;
use crate::session::{default_engine_factory, EngineChoice, EngineFactory};

const PING_CONCURRENCY: usize = 128;
const PING_TIMEOUT: Duration = Duration::from_millis(500);
const TCP_TIMEOUT: Duration = Duration::from_millis(300);
const TCP_PORTS: [u16; 4] = [80, 443, 445, 22];

pub type PingProbe = Arc<dyn Fn(Ipv4Addr) -> BoxFuture<'static, bool> + Send + Sync>;
pub type TcpProbe = Arc<dyn Fn(Ipv4Addr) -> BoxFuture<'static, bool> + Send + Sync>;
pub type ArpReader =
    Arc<dyn Fn() -> BoxFuture<'static, Result<HashMap<Ipv4Addr, String>, ArpError>> + Send + Sync>;
pub type HostResolver = Arc<dyn Fn(Ipv4Addr) -> BoxFuture<'static, Option<String>> + Send + Sync>;

#[derive(Clone)]
pub struct DiscoveryServices {
    ping: PingProbe,
    tcp: TcpProbe,
    arp: ArpReader,
    resolver: HostResolver,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeOutcome {
    Ping,
    Tcp,
    Arp,
}

impl ProbeOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ping => "ping",
            Self::Tcp => "tcp",
            Self::Arp => "arp",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredHost {
    pub ip: Ipv4Addr,
    pub mac: Option<String>,
    pub hostname: Option<String>,
    pub found_by: ProbeOutcome,
    pub open_ports: Vec<crate::scan::ports::OpenPort>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryResult {
    pub total: usize,
    pub hosts: Vec<DiscoveredHost>,
}

impl DiscoveryServices {
    pub fn new(ping: PingProbe, tcp: TcpProbe, arp: ArpReader, resolver: HostResolver) -> Self {
        Self {
            ping,
            tcp,
            arp,
            resolver,
        }
    }

    pub fn production() -> Self {
        let factory = default_engine_factory();
        Self::new(
            Arc::new(move |ip| ping_once(ip, Arc::clone(&factory))),
            Arc::new(tcp_once),
            Arc::new(|| Box::pin(read_arp_table())),
            Arc::new(|ip| {
                Box::pin(async move {
                    resolve_many(&[IpAddr::V4(ip)])
                        .await
                        .remove(&IpAddr::V4(ip))
                })
            }),
        )
    }
}

pub async fn discover(cidr: &str, tcp_fallback: bool) -> Result<DiscoveryResult, ScanError> {
    discover_with(cidr, tcp_fallback, DiscoveryServices::production()).await
}

pub async fn discover_with(
    cidr: &str,
    tcp_fallback: bool,
    services: DiscoveryServices,
) -> Result<DiscoveryResult, ScanError> {
    let hosts = expand_ipv4_cidr(cidr)?;
    let total = hosts.len();
    let ping_hits = probe_hosts(&hosts, Arc::clone(&services.ping)).await;
    let tcp_hits = if tcp_fallback {
        let misses: Vec<Ipv4Addr> = hosts
            .into_iter()
            .filter(|ip| !ping_hits.contains(ip))
            .collect();
        probe_hosts(&misses, Arc::clone(&services.tcp)).await
    } else {
        HashSet::new()
    };
    let arp = (services.arp)().await?;
    let mut discovered = merge_hosts(&ping_hits, &tcp_hits, &arp);
    add_hostnames(&mut discovered, services.resolver).await;
    Ok(DiscoveryResult {
        total,
        hosts: discovered,
    })
}

async fn probe_hosts(hosts: &[Ipv4Addr], probe: PingProbe) -> HashSet<Ipv4Addr> {
    let semaphore = Arc::new(Semaphore::new(PING_CONCURRENCY));
    let mut set = tokio::task::JoinSet::new();
    for ip in hosts.iter().copied() {
        let semaphore = Arc::clone(&semaphore);
        let probe = Arc::clone(&probe);
        set.spawn(async move {
            let permit = semaphore.acquire_owned().await.ok()?;
            let alive = probe(ip).await;
            drop(permit);
            alive.then_some(ip)
        });
    }
    let mut hits = HashSet::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(Some(ip)) = joined {
            hits.insert(ip);
        }
    }
    hits
}

fn merge_hosts(
    ping_hits: &HashSet<Ipv4Addr>,
    tcp_hits: &HashSet<Ipv4Addr>,
    arp: &HashMap<Ipv4Addr, String>,
) -> Vec<DiscoveredHost> {
    let mut ips: Vec<Ipv4Addr> = ping_hits.union(tcp_hits).copied().collect();
    ips.extend(
        arp.keys()
            .filter(|ip| !ping_hits.contains(ip) && !tcp_hits.contains(ip))
            .copied(),
    );
    ips.sort_unstable();
    ips.into_iter()
        .map(|ip| {
            let found_by = if ping_hits.contains(&ip) {
                ProbeOutcome::Ping
            } else if tcp_hits.contains(&ip) {
                ProbeOutcome::Tcp
            } else {
                ProbeOutcome::Arp
            };
            DiscoveredHost {
                ip,
                mac: arp.get(&ip).cloned(),
                hostname: None,
                found_by,
                open_ports: Vec::new(),
            }
        })
        .collect()
}

async fn add_hostnames(hosts: &mut [DiscoveredHost], resolver: HostResolver) {
    let ips: Vec<Ipv4Addr> = hosts.iter().map(|host| host.ip).collect();
    let mut set = tokio::task::JoinSet::new();
    for ip in ips {
        let resolver = Arc::clone(&resolver);
        set.spawn(async move { (ip, resolver(ip).await) });
    }
    let mut names = HashMap::new();
    while let Some(joined) = set.join_next().await {
        if let Ok((ip, Some(name))) = joined {
            names.insert(ip, name);
        }
    }
    for host in hosts {
        host.hostname = names.remove(&host.ip);
    }
}

fn ping_once(ip: Ipv4Addr, factory: EngineFactory) -> BoxFuture<'static, bool> {
    Box::pin(async move {
        match select_engine(ip, &factory).await {
            Ok(mut engine) => matches!(
                timeout(PING_TIMEOUT, engine.probe(1)).await,
                Ok(ProbeResult::Rtt(_))
            ),
            Err(_) => false,
        }
    })
}

async fn select_engine(ip: Ipv4Addr, factory: &EngineFactory) -> Result<PingEngine, EngineError> {
    match factory(
        IpAddr::V4(ip),
        0,
        EngineChoice::Primary,
        PING_PAYLOAD_BYTES,
        false,
    )
    .await
    {
        Ok(engine) => Ok(engine),
        Err(EngineError::Socket(err)) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            factory(
                IpAddr::V4(ip),
                0,
                EngineChoice::Fallback,
                PING_PAYLOAD_BYTES,
                false,
            )
            .await
        }
        Err(err) => Err(err),
    }
}

fn tcp_once(ip: Ipv4Addr) -> BoxFuture<'static, bool> {
    Box::pin(async move {
        for port in TCP_PORTS {
            let addr = SocketAddr::new(IpAddr::V4(ip), port);
            if timeout(TCP_TIMEOUT, TcpStream::connect(addr))
                .await
                .is_ok_and(|result| result.is_ok())
            {
                return true;
            }
        }
        false
    })
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod discovery_tests;
