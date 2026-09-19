//! Windows IPHlpAPI fallback engine: unprivileged, locale-independent, and
//! no process spawn. IPv4 via IcmpCreateFile/IcmpSendEcho/IcmpParseReplies;
//! IPv6 via Icmp6CreateFile/Icmp6SendEcho2/Icmp6ParseReplies with
//! `sockaddr_in6.sin6_scope_id` carrying the parsed zone-id. Blocking FFI is
//! wrapped in `tokio::task::spawn_blocking`. Call patterns proven by the
//! todo-2 spike (src/bin/ping_spike.rs).

use std::io;
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use super::{ProbeResult, PING_TIMEOUT_MS};

/// IPHlpAPI status: no reply arrived before the timeout (ipexport.h).
const IP_REQ_TIMED_OUT: u32 = 11010;
/// IPHlpAPI status: DF packet exceeds the path MTU (ipexport.h).
const IP_PACKET_TOO_BIG: u32 = 11009;

/// Result of one synchronous IPHlpAPI echo.
enum EchoError {
    /// Request exceeded the path MTU with DF set.
    PacketTooBig,
    /// Request timed out (status / GetLastError == IP_REQ_TIMED_OUT).
    Timeout,
    /// Any other failure, described with the verbatim OS error.
    Failed(String),
}

/// Windows IPHlpAPI engine bound to one resolved target.
pub struct WinIcmpPinger {
    target: IpAddr,
    scope_id: u32,
    payload: Vec<u8>,
    dont_fragment: bool,
}

impl WinIcmpPinger {
    pub fn new(target: IpAddr, scope_id: u32, payload_size: usize, dont_fragment: bool) -> Self {
        Self {
            target,
            scope_id,
            payload: vec![0x61; payload_size.clamp(1, 65_507)],
            // DF is an IPv4-only concept; ignore it for v6 targets.
            dont_fragment: dont_fragment && target.is_ipv4(),
        }
    }

    /// Send one echo on a blocking thread and map the outcome.
    pub async fn probe(&mut self, _seq: u64) -> ProbeResult {
        let target = self.target;
        let scope_id = self.scope_id;
        let payload = self.payload.clone();
        let dont_fragment = self.dont_fragment;
        let timeout_ms = u32::try_from(PING_TIMEOUT_MS).unwrap_or(u32::MAX);
        match tokio::task::spawn_blocking(move || {
            echo(target, scope_id, &payload, timeout_ms, dont_fragment)
        })
        .await
        {
            Ok(Ok(rtt)) => ProbeResult::Rtt(rtt),
            Ok(Err(EchoError::PacketTooBig)) => {
                ProbeResult::Error(format!("echo reply status={IP_PACKET_TOO_BIG}"))
            }
            Ok(Err(EchoError::Timeout)) => ProbeResult::Timeout,
            Ok(Err(EchoError::Failed(msg))) => ProbeResult::Error(msg),
            Err(join_err) => ProbeResult::Error(format!("echo task join failed: {join_err}")),
        }
    }

    /// Send one IPv4 echo with the Don't Fragment flag set.
    pub async fn echo_v4_df(
        target: Ipv4Addr,
        payload: Vec<u8>,
        timeout_ms: u32,
    ) -> Result<Duration, io::Error> {
        match tokio::task::spawn_blocking(move || sys::echo_v4_df(target, &payload, timeout_ms))
            .await
        {
            Ok(Ok(rtt)) => Ok(rtt),
            Ok(Err(EchoError::PacketTooBig)) => Err(io::Error::from_raw_os_error(
                i32::try_from(IP_PACKET_TOO_BIG).unwrap_or(i32::MAX),
            )),
            Ok(Err(EchoError::Timeout)) => Err(io::Error::from_raw_os_error(
                i32::try_from(IP_REQ_TIMED_OUT).unwrap_or(i32::MAX),
            )),
            Ok(Err(EchoError::Failed(msg))) => Err(io::Error::other(msg)),
            Err(join_err) => Err(io::Error::other(format!(
                "echo task join failed: {join_err}"
            ))),
        }
    }
}

fn echo(
    target: IpAddr,
    scope_id: u32,
    payload: &[u8],
    timeout_ms: u32,
    dont_fragment: bool,
) -> Result<Duration, EchoError> {
    match target {
        IpAddr::V4(v4) if dont_fragment => sys::echo_v4_df(v4, payload, timeout_ms),
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

    use super::{EchoError, IP_PACKET_TOO_BIG, IP_REQ_TIMED_OUT};

    /// C layout from ipexport.h. The windows 0.61 bindings available to this
    /// crate do not expose `IP_OPTION_INFORMATION`, so the exact layout needed
    /// by `IcmpSendEcho` is declared locally.
    #[repr(C)]
    struct IpOptionInformation {
        ttl: u8,
        tos: u8,
        flags: u8,
        options_size: u8,
        options_data: *mut u8,
    }

    const IP_FLAG_DF: u8 = 0x2;

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

    fn last_error_df(context: &str) -> EchoError {
        // SAFETY: [Category 8 - FFI boundary] GetLastError has no arguments
        // and only reads the calling thread's Windows error slot.
        let code = unsafe { GetLastError() };
        match code.0 {
            IP_PACKET_TOO_BIG => EchoError::PacketTooBig,
            IP_REQ_TIMED_OUT => EchoError::Timeout,
            other => EchoError::Failed(format!(
                "{context}: GetLastError={} ({})",
                other,
                io::Error::from_raw_os_error(other as i32)
            )),
        }
    }

    fn map_reply_status(status: u32, context: &str, rtt_ms: u32) -> Result<Duration, EchoError> {
        if status == IP_SUCCESS {
            Ok(Duration::from_millis(u64::from(rtt_ms)))
        } else if status == IP_REQ_TIMED_OUT {
            Err(EchoError::Timeout)
        } else {
            Err(EchoError::Failed(format!(
                "{context} reply status={status}"
            )))
        }
    }

    fn map_reply_status_df(status: u32, context: &str, rtt_ms: u32) -> Result<Duration, EchoError> {
        match status {
            IP_SUCCESS => Ok(Duration::from_millis(u64::from(rtt_ms))),
            IP_PACKET_TOO_BIG => Err(EchoError::PacketTooBig),
            IP_REQ_TIMED_OUT => Err(EchoError::Timeout),
            other => Err(EchoError::Failed(format!("{context} reply status={other}"))),
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

    /// IPv4 echo via IcmpSendEcho with IP_FLAG_DF set in RequestOptions.
    pub fn echo_v4_df(
        target: Ipv4Addr,
        payload: &[u8],
        timeout_ms: u32,
    ) -> Result<Duration, EchoError> {
        let mut options = IpOptionInformation {
            ttl: 128,
            tos: 0,
            flags: IP_FLAG_DF,
            options_size: 0,
            options_data: std::ptr::null_mut(),
        };
        // SAFETY: [Category 8 - FFI boundary] all pointers reference live
        // values for the whole synchronous IcmpSendEcho call: `payload` is an
        // immutable input buffer, `options` has ipexport.h layout, and
        // `reply_buf` is sized for ICMP_ECHO_REPLY plus payload echo bytes.
        unsafe {
            let handle = create_handle("IcmpCreateFile", IcmpCreateFile())?;
            let mut reply_buf = vec![0u8; size_of::<ICMP_ECHO_REPLY>() + payload.len() + 8];
            let replies = IcmpSendEcho(
                handle.0,
                u32::from(target).to_be(),
                payload.as_ptr().cast(),
                payload.len() as u16,
                Some((&raw mut options).cast()),
                reply_buf.as_mut_ptr().cast(),
                reply_buf.len() as u32,
                timeout_ms,
            );
            let reply = &*(reply_buf.as_ptr() as *const ICMP_ECHO_REPLY);
            if replies == 0 {
                return match reply.Status {
                    IP_PACKET_TOO_BIG => Err(EchoError::PacketTooBig),
                    _ => Err(last_error_df("IcmpSendEcho returned 0 replies")),
                };
            }
            let _parsed = IcmpParseReplies(reply_buf.as_mut_ptr().cast(), reply_buf.len() as u32);
            map_reply_status_df(reply.Status, "IcmpSendEcho", reply.RoundTripTime)
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
        let mut pinger = WinIcmpPinger::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 32, false);
        match pinger.probe(1).await {
            ProbeResult::Rtt(rtt) => assert!(rtt <= Duration::from_millis(PING_TIMEOUT_MS)),
            other => panic!("expected Rtt from 127.0.0.1, got {other:?}"),
        }
    }

    // Given the WinIcmp engine pointed at IPv4 loopback with DF requested,
    // When probed as the unprivileged user,
    // Then a real reply comes back via the echo_v4_df path (IP_FLAG_DF set).
    #[tokio::test]
    async fn winicmp_v4_df_probe_uses_df_path_unprivileged() {
        let mut pinger = WinIcmpPinger::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0, 32, true);
        assert!(
            matches!(pinger.probe(1).await, ProbeResult::Rtt(_)),
            "expected DF loopback probe to succeed"
        );
    }

    // Given the WinIcmp DF adapter pointed at IPv4 loopback,
    // When a 32-byte payload is probed as the unprivileged user,
    // Then a real reply comes back with DF request options accepted.
    #[tokio::test]
    async fn winicmp_v4_df_loopback_32b_succeeds_unprivileged() {
        let payload = vec![0x61; 32];

        let result = WinIcmpPinger::echo_v4_df(Ipv4Addr::LOCALHOST, payload, 1000).await;

        assert!(
            result.is_ok(),
            "expected DF loopback success, got {result:?}"
        );
    }

    // Given loopback accepts jumbo echo payloads on this Windows machine,
    // When an 8972-byte payload is probed with DF,
    // Then a real reply comes back instead of IP_PACKET_TOO_BIG.
    #[tokio::test]
    async fn winicmp_v4_df_loopback_8972b_succeeds_unprivileged() {
        let payload = vec![0x61; 8972];

        let result = WinIcmpPinger::echo_v4_df(Ipv4Addr::LOCALHOST, payload, 1000).await;

        assert!(
            result.is_ok(),
            "expected jumbo DF loopback success, got {result:?}"
        );
    }

    // Given a non-loopback path with a typical 1500-byte first hop,
    // When an 8972-byte payload is probed with DF,
    // Then IPHlpAPI reports IP_PACKET_TOO_BIG (11009).
    #[tokio::test]
    #[ignore = "real adapter evidence; run with --ignored"]
    async fn winicmp_v4_df_internet_8972b_reports_packet_too_big() {
        let payload = vec![0x61; 8972];

        let result = WinIcmpPinger::echo_v4_df(Ipv4Addr::new(8, 8, 8, 8), payload, 1000).await;

        assert_eq!(result.err().and_then(|err| err.raw_os_error()), Some(11009));
    }

    // Given the WinIcmp engine pointed at IPv6 loopback,
    // When probed as the unprivileged user,
    // Then a real reply comes back (Icmp6SendEcho2 path, scope 0).
    #[tokio::test]
    async fn winicmp_v6_loopback_echo_succeeds_unprivileged() {
        let mut pinger = WinIcmpPinger::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 0, 32, false);
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
        let mut engine = PingEngine::WinIcmp(WinIcmpPinger::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            0,
            32,
            false,
        ));
        assert!(matches!(engine.probe(1).await, ProbeResult::Rtt(_)));
    }

    // Given TEST-NET-1 (guaranteed non-responding),
    // When probed,
    // Then the outcome is Timeout or Error (no Rtt), with no panic or hang.
    #[tokio::test]
    async fn winicmp_testnet_never_hangs() {
        let mut pinger = WinIcmpPinger::new(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)), 0, 32, false);
        let result = pinger.probe(1).await;
        assert!(
            matches!(result, ProbeResult::Timeout | ProbeResult::Error(_)),
            "expected Timeout/Error, got {result:?}"
        );
    }
}
