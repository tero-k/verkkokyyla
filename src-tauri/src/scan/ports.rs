use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::{watch, Semaphore};
use tokio::time::{sleep, timeout};

pub const DEFAULT_PORT_CONCURRENCY: usize = 32;
pub const DEFAULT_PORT_MIN_INTERVAL: Duration = Duration::from_millis(100);

pub const COMMON_PORTS: [(u16, &str); 25] = [
    (80, "http"),
    (23, "telnet"),
    (443, "https"),
    (21, "ftp"),
    (22, "ssh"),
    (25, "smtp"),
    (3389, "ms-wbt-server"),
    (110, "pop3"),
    (445, "microsoft-ds"),
    (139, "netbios-ssn"),
    (143, "imap"),
    (53, "domain"),
    (135, "msrpc"),
    (3306, "mysql"),
    (8080, "http-proxy"),
    (1723, "pptp"),
    (111, "sunrpc"),
    (995, "pop3s"),
    (993, "imaps"),
    (5900, "vnc"),
    (515, "printer"),
    (554, "rtsp"),
    (631, "ipp"),
    (1433, "ms-sql-s"),
    (8443, "https-alt"),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenPort {
    pub port: u16,
    pub service: String,
}

pub type PortConnector = Arc<dyn Fn(Ipv4Addr, u16) -> BoxFuture<'static, bool> + Send + Sync>;

pub const fn probable_service(port: u16) -> &'static str {
    let mut index = 0;
    while index < COMMON_PORTS.len() {
        let (candidate, service) = COMMON_PORTS[index];
        if candidate == port {
            return service;
        }
        index += 1;
    }
    "unknown"
}

pub fn production_port_connector(connect_timeout: Duration) -> PortConnector {
    Arc::new(move |host, port| {
        Box::pin(async move {
            let addr = SocketAddr::new(IpAddr::V4(host), port);
            timeout(connect_timeout, TcpStream::connect(addr))
                .await
                .is_ok_and(|result| result.is_ok())
        })
    })
}

pub async fn probe_ports_with_connector(
    host: Ipv4Addr,
    ports: &[(u16, &'static str)],
    connector: PortConnector,
    concurrency: usize,
    min_interval: Duration,
    cancel_rx: &mut watch::Receiver<bool>,
) -> Vec<OpenPort> {
    if ports.is_empty() || *cancel_rx.borrow() {
        return Vec::new();
    }
    let semaphore = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut set = tokio::task::JoinSet::new();
    let mut is_first_attempt = true;

    for &(port, service) in ports {
        if *cancel_rx.borrow() {
            break;
        }
        if is_first_attempt {
            is_first_attempt = false;
        } else {
            tokio::select! {
                biased;
                changed = cancel_rx.changed() => {
                    if changed.is_ok() && *cancel_rx.borrow() {
                        break;
                    }
                }
                () = sleep(min_interval) => {}
            }
        }

        let semaphore = Arc::clone(&semaphore);
        let connector = Arc::clone(&connector);
        set.spawn(async move {
            let permit = semaphore.acquire_owned().await.ok()?;
            let is_open = connector(host, port).await;
            drop(permit);
            is_open.then_some(OpenPort {
                port,
                service: service.to_owned(),
            })
        });
    }

    let mut open_ports = Vec::new();
    while let Some(joined) = set.join_next().await {
        if let Ok(Some(open_port)) = joined {
            open_ports.push(open_port);
        }
    }
    open_ports.sort_unstable_by_key(|open_port| open_port.port);
    open_ports
}

#[cfg(test)]
#[path = "ports_tests.rs"]
mod ports_tests;
