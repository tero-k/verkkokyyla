//! Windows IPHlpAPI fallback engine: unprivileged, locale-independent, and
//! no process spawn. IPv4 via IcmpCreateFile/IcmpSendEcho/IcmpParseReplies;
//! IPv6 via Icmp6CreateFile/Icmp6SendEcho2/Icmp6ParseReplies with
//! `sockaddr_in6.sin6_scope_id` carrying the parsed zone-id. Blocking FFI is
//! wrapped in `tokio::task::spawn_blocking`. Call patterns proven by the
//! todo-2 spike (src/bin/ping_spike.rs).

use std::net::IpAddr;
use std::time::Duration;

use super::{ProbeResult, PING_PAYLOAD_BYTES, PING_TIMEOUT_MS};

/// IPHlpAPI status: no reply arrived before the timeout (ipexport.h).
const IP_REQ_TIMED_OUT: u32 = 11010;

/// Result of one synchronous IPHlpAPI echo.
enum EchoError {
    /// Request timed out (status / GetLastError == IP_REQ_TIMED_OUT).
    Timeout,
    /// Any other failure, described with the verbatim OS error.
    Failed(String),
}

/// Windows IPHlpAPI engine bound to one resolved target.
pub struct WinIcmpPinger {
    target: IpAddr,
    scope_id: u32,
    payload: [u8; PING_PAYLOAD_BYTES],
}

impl WinIcmpPinger {
    pub fn new(target: IpAddr, scope_id: u32) -> Self {
        Self {
            target,
            scope_id,
            payload: [0x61; PING_PAYLOAD_BYTES],
        }
    }

    /// Send one echo on a blocking thread and map the outcome.
    pub async fn probe(&mut self, _seq: u64) -> ProbeResult {
        let target = self.target;
        let scope_id = self.scope_id;
        let payload = self.payload;
        let timeout_ms = u32::try_from(PING_TIMEOUT_MS).unwrap_or(u32::MAX);
        match tokio::task::spawn_blocking(move || echo(target, scope_id, &payload, timeout_ms))
            .await
        {
            Ok(Ok(rtt)) => ProbeResult::Rtt(rtt),
            Ok(Err(EchoError::Timeout)) => ProbeResult::Timeout,
            Ok(Err(EchoError::Failed(msg))) => ProbeResult::Error(msg),
            Err(join_err) => ProbeResult::Error(format!("echo task join failed: {join_err}")),
        }
    }
}

fn echo(
    target: IpAddr,
    scope_id: u32,
    payload: &[u8],
    timeout_ms: u32,
) -> Result<Duration, EchoError> {
    match target {
        IpAddr::V4(v4) => sys::echo_v4(v4, payload, timeout_ms),
        IpAddr::V6(v6) => sys::echo_v6(v6, scope_id, payload, timeout_ms),
    }
}

mod sys {
    use std::io;
    use std::mem::size_of;
    use std::net::{Ipv4Addr, Ipv6Addr};
    use std::time::Duration;

    use windows::Win32::Foundation::{GetLastError, HANDLE};
    use windows::Win32::NetworkManagement::IpHelper::{
        Icmp6CreateFile, Icmp6ParseReplies, Icmp6SendEcho2, IcmpCloseHandle, IcmpCreateFile,
        IcmpParseReplies, IcmpSendEcho, ICMPV6_ECHO_REPLY_LH, ICMP_ECHO_REPLY, IP_SUCCESS,
    };
    use windows::Win32::Networking::WinSock::{AF_INET6, IN6_ADDR, IN6_ADDR_0, SOCKADDR_IN6};

    use super::{EchoError, IP_REQ_TIMED_OUT};

    /// RAII wrapper so the IPHlpAPI handle is always closed.
    struct IcmpHandle(HANDLE);

    impl Drop for IcmpHandle {
        fn drop(&mut self) {
            // SAFETY: self.0 is a live handle returned by IcmpCreateFile.
            unsafe {
                let _ = IcmpCloseHandle(self.0);
            }
        }
    }

    fn last_error(context: &str) -> EchoError {
        // SAFETY: GetLastError only reads the calling thread's error slot.
        let code = unsafe { GetLastError() };
        if code.0 == IP_REQ_TIMED_OUT {
            return EchoError::Timeout;
        }
        EchoError::Failed(format!(
            "{context}: GetLastError={} ({})",
            code.0,
            io::Error::from_raw_os_error(code.0 as i32)
        ))
    }

    fn map_reply_status(status: u32, context: &str, rtt_ms: u32) -> Result<Duration, EchoError> {
        if status == IP_SUCCESS {
            Ok(Duration::from_millis(u64::from(rtt_ms)))
        } else if status == IP_REQ_TIMED_OUT {
            Err(EchoError::Timeout)
        } else {
            Err(EchoError::Failed(format!("{context} reply status={status}")))
        }
    }

    fn create_handle(
        context: &str,
        raw: windows::core::Result<HANDLE>,
    ) -> Result<IcmpHandle, EchoError> {
        match raw {
            Ok(h) => Ok(IcmpHandle(h)),
            Err(e) => Err(EchoError::Failed(format!("{context} failed: {e}"))),
        }
    }

    /// IPv4 echo via IcmpCreateFile/IcmpSendEcho/IcmpParseReplies.
    pub fn echo_v4(
        target: Ipv4Addr,
        payload: &[u8],
        timeout_ms: u32,
    ) -> Result<Duration, EchoError> {
        // SAFETY: all pointers reference live buffers that outlive the call;
        // the reply buffer is sized to hold ICMP_ECHO_REPLY + payload.
        unsafe {
            let handle = create_handle("IcmpCreateFile", IcmpCreateFile())?;
            let mut reply_buf = vec![0u8; size_of::<ICMP_ECHO_REPLY>() + payload.len() + 8];
            let replies = IcmpSendEcho(
                handle.0,
                u32::from(target).to_be(), // IPAddr expects network byte order
                payload.as_ptr().cast(),
                payload.len() as u16,
                None,
                reply_buf.as_mut_ptr().cast(),
                reply_buf.len() as u32,
                timeout_ms,
            );
            if replies == 0 {
                return Err(last_error("IcmpSendEcho returned 0 replies"));
            }
            let _parsed = IcmpParseReplies(reply_buf.as_mut_ptr().cast(), reply_buf.len() as u32);
            let reply = &*(reply_buf.as_ptr() as *const ICMP_ECHO_REPLY);
            map_reply_status(reply.Status, "IcmpSendEcho", reply.RoundTripTime)
        }
    }

    fn sockaddr_in6(addr: Ipv6Addr, scope_id: u32) -> SOCKADDR_IN6 {
        let mut sa = SOCKADDR_IN6 {
            sin6_family: AF_INET6,
            sin6_port: 0,
            sin6_flowinfo: 0,
            sin6_addr: IN6_ADDR {
                u: IN6_ADDR_0 {
                    Byte: addr.octets(),
                },
            },
            ..Default::default()
        };
        sa.Anonymous.sin6_scope_id = scope_id;
        sa
    }

    /// IPv6 echo via Icmp6CreateFile/Icmp6SendEcho2/Icmp6ParseReplies; the
    /// zone-id rides in `sin6_scope_id` of the destination sockaddr.
    pub fn echo_v6(
        target: Ipv6Addr,
        scope_id: u32,
        payload: &[u8],
        timeout_ms: u32,
    ) -> Result<Duration, EchoError> {
        // SAFETY: all pointers reference live values that outlive the call;
        // the reply buffer is sized to hold ICMPV6_ECHO_REPLY_LH + payload.
        // Event=None and ApcRoutine=None select the synchronous behavior.
        unsafe {
            let handle = create_handle("Icmp6CreateFile", Icmp6CreateFile())?;
            let source = sockaddr_in6(Ipv6Addr::UNSPECIFIED, 0);
            let dest = sockaddr_in6(target, scope_id);
            let mut reply_buf = vec![0u8; size_of::<ICMPV6_ECHO_REPLY_LH>() + payload.len() + 8];
            let replies = Icmp6SendEcho2(
                handle.0,
                None,
                None,
                None,
                &source,
                &dest,
                payload.as_ptr().cast(),
                payload.len() as u16,
                None,
                reply_buf.as_mut_ptr().cast(),
                reply_buf.len() as u32,
                timeout_ms,
            );
            if replies == 0 {
                return Err(last_error("Icmp6SendEcho2 returned 0 replies"));
            }
            let _parsed = Icmp6ParseReplies(reply_buf.as_mut_ptr().cast(), reply_buf.len() as u32);
            let reply = &*(reply_buf.as_ptr() as *const ICMPV6_ECHO_REPLY_LH);
            map_reply_status(reply.Status, "Icmp6SendEcho2", reply.RoundTripTime)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::PingEngine;
    use std::net::{Ipv4Addr, Ipv6Addr};

    // Given the WinIcmp engine pointed at IPv4 loopback,
    // When probed as the unprivileged user,
    // Then a real reply comes back (IcmpSendEcho path, no admin needed).
    #[tokio::test]
    async fn winicmp_v4_loopback_echo_succeeds_unprivileged() {
        let mut pinger = WinIcmpPinger::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0);
        match pinger.probe(1).await {
            ProbeResult::Rtt(rtt) => assert!(rtt <= Duration::from_millis(PING_TIMEOUT_MS)),
            other => panic!("expected Rtt from 127.0.0.1, got {other:?}"),
        }
    }

    // Given the WinIcmp engine pointed at IPv6 loopback,
    // When probed as the unprivileged user,
    // Then a real reply comes back (Icmp6SendEcho2 path, scope 0).
    #[tokio::test]
    async fn winicmp_v6_loopback_echo_succeeds_unprivileged() {
        let mut pinger = WinIcmpPinger::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 0);
        match pinger.probe(1).await {
            ProbeResult::Rtt(_) => {}
            other => panic!("expected Rtt from ::1, got {other:?}"),
        }
    }

    // Given the enum-dispatched engine holding the WinIcmp variant,
    // When probed,
    // Then dispatch reaches the IPHlpAPI implementation.
    #[tokio::test]
    async fn winicmp_enum_dispatch_reaches_impl() {
        let mut engine = PingEngine::WinIcmp(WinIcmpPinger::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0));
        assert!(matches!(engine.probe(1).await, ProbeResult::Rtt(_)));
    }

    // Given TEST-NET-1 (guaranteed non-responding),
    // When probed,
    // Then the outcome is Timeout or Error (no Rtt), with no panic or hang.
    #[tokio::test]
    async fn winicmp_testnet_never_hangs() {
        let mut pinger = WinIcmpPinger::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 0);
        let result = pinger.probe(1).await;
        assert!(
            matches!(result, ProbeResult::Timeout | ProbeResult::Error(_)),
            "expected Timeout/Error, got {result:?}"
        );
    }
}
