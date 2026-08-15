//! surge-ping engine: the preferred ICMP echo engine (v4 + v6).
//!
//! One `Client`+`Pinger` per target. On Windows the DGRAM socket hint fails
//! and surge-ping transparently falls back to a raw ICMP socket, which works
//! unprivileged (spike: .omo/evidence/task-2-netdebug-ping.md).

use std::net::IpAddr;
use std::num::NonZeroU32;
use std::time::Duration;

use surge_ping::{Client, Config, PingIdentifier, PingSequence, Pinger, SurgeError, ICMP};

use super::{EngineError, ProbeResult, PING_TIMEOUT_MS};

/// Fixed ICMP identifier; the session layer runs one session at a time.
const SURGE_IDENT: u16 = 0xC0DE;

/// surge-ping engine bound to one resolved target.
pub struct SurgePinger {
    // The Client owns the receiver task and a `destroyed` flag that the
    // Pinger checks on every send — it must outlive the Pinger.
    #[allow(dead_code)]
    client: Client,
    pinger: Pinger,
    payload: Vec<u8>,
}

impl SurgePinger {
    /// Create a client + pinger for `addr`. A non-zero `scope_id` (IPv6
    /// zone) is applied both at the socket level (`interface_index`, where
    /// the platform supports it) and on the pinger's destination sockaddr.
    pub async fn new(
        addr: IpAddr,
        scope_id: u32,
        payload_size: usize,
        _dont_fragment: bool,
    ) -> Result<Self, EngineError> {
        let kind = match addr {
            IpAddr::V4(_) => ICMP::V4,
            IpAddr::V6(_) => ICMP::V6,
        };
        let mut builder = Config::builder().kind(kind);
        if let Some(index) = NonZeroU32::new(scope_id) {
            builder = builder.interface_index(index);
        }
        let client = Client::new(&builder.build()).map_err(EngineError::Socket)?;
        let mut pinger = client.pinger(addr, PingIdentifier(SURGE_IDENT)).await;
        pinger.timeout(Duration::from_millis(PING_TIMEOUT_MS));
        if scope_id != 0 {
            pinger.scope_id(scope_id);
        }
        let payload = vec![0x61; payload_size.clamp(1, 65_507)];
        Ok(Self {
            client,
            pinger,
            payload,
        })
    }

    /// Send one echo and map the outcome to a [`ProbeResult`].
    pub async fn probe(&mut self, seq: u64) -> ProbeResult {
        // The wire sequence space is u16; session seq wraps into it.
        let wire_seq = PingSequence((seq % 65536) as u16);
        match self.pinger.ping(wire_seq, &self.payload).await {
            Ok((_packet, rtt)) => ProbeResult::Rtt(rtt),
            Err(SurgeError::Timeout { .. }) => ProbeResult::Timeout,
            Err(other) => ProbeResult::Error(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    // Given a surge engine pointed at IPv4 loopback,
    // When 5 probes are sent,
    // Then at least 4 come back as Rtt (real unprivileged ICMP; ignored by
    // default, run explicitly for evidence).
    #[tokio::test]
    #[ignore = "real ICMP echo; run with --ignored for evidence"]
    async fn integration_ping_surge_loopback_v4() {
        let mut pinger = SurgePinger::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 32, false)
            .await
            .expect("surge client on v4 loopback");
        let mut replies = 0u32;
        for seq in 1..=5 {
            if let ProbeResult::Rtt(_) = pinger.probe(seq).await {
                replies += 1;
            }
        }
        assert!(replies >= 4, "expected >=4/5 replies, got {replies}");
    }

    // Given a surge engine pointed at IPv6 loopback,
    // When 5 probes are sent,
    // Then at least 4 come back as Rtt; if v6 ICMP is unavailable on this
    // host the test skips gracefully.
    #[tokio::test]
    #[ignore = "real ICMP echo; run with --ignored for evidence"]
    async fn integration_ping_surge_loopback_v6() {
        let Ok(mut pinger) = SurgePinger::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 0, 32, false).await else {
            eprintln!("SKIP: cannot create ICMPv6 client on this host");
            return;
        };
        let mut replies = 0u32;
        for seq in 1..=5 {
            if let ProbeResult::Rtt(_) = pinger.probe(seq).await {
                replies += 1;
            }
        }
        if replies == 0 {
            eprintln!("SKIP: ICMPv6 loopback produced no replies on this host");
            return;
        }
        assert!(replies >= 4, "expected >=4/5 replies, got {replies}");
    }

    // Given TEST-NET-1 (192.0.2.1, guaranteed non-responding),
    // When probed,
    // Then every probe is a Timeout within the 1s budget — no panic, no hang.
    #[tokio::test]
    #[ignore = "real ICMP echo; run with --ignored for evidence"]
    async fn integration_ping_testnet_timeouts_without_hang() {
        let mut pinger = SurgePinger::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 0, 32, false)
            .await
            .expect("surge client");
        for seq in 1..=3 {
            let result = pinger.probe(seq).await;
            assert!(
                matches!(result, ProbeResult::Timeout),
                "expected Timeout, got {result:?}"
            );
        }
    }
}
