//! Todo 2 privilege spike: which ICMP echo method works for the CURRENT
//! unprivileged user on this machine?
//!
//! Methods probed:
//!   1. surge-ping ICMPv4 echo -> 127.0.0.1
//!   2. surge-ping ICMPv6 echo -> ::1
//!   3. IPHlpAPI IcmpSendEcho  (IPv4) -> 127.0.0.1   [Windows only]
//!   4. IPHlpAPI Icmp6SendEcho2 (IPv6) -> ::1        [Windows only]
//!
//! Each method prints PASS (with RTT) or FAIL with the exact OS error
//! code/message verbatim. Env toggle `PING_SPIKE_FORCE_RAW=1` forces the
//! surge-ping socket type hint to RAW (skips the DGRAM-first path) to
//! exercise the raw-socket failure path explicitly. Optional positional args
//! override the targets: `ping_spike [IPV4_TARGET] [IPV6_TARGET]` — used to
//! force a timeout/loss failure path (e.g. TEST-NET-1 `192.0.2.1`).
//!
//! SPIKE ONLY - never wired into lib.rs / the app.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use surge_ping::{Client, Config, PingIdentifier, PingSequence, ICMP};

const PAYLOAD: [u8; 32] = [0x61; 32];
const TIMEOUT: Duration = Duration::from_millis(1000);

fn print_result(method: &str, target: &str, result: Result<Duration, String>) {
    match result {
        Ok(rtt) => println!("{method} -> {target}: PASS rtt={:.3}ms", rtt.as_secs_f64() * 1000.0),
        Err(err) => println!("{method} -> {target}: FAIL {err}"),
    }
}

async fn surge_probe(addr: IpAddr) -> Result<Duration, String> {
    let kind = match addr {
        IpAddr::V4(_) => ICMP::V4,
        IpAddr::V6(_) => ICMP::V6,
    };
    let sock_type_hint = if std::env::var_os("PING_SPIKE_FORCE_RAW").is_some() {
        socket2::Type::RAW
    } else {
        socket2::Type::DGRAM
    };
    let config = Config::builder()
        .kind(kind)
        .sock_type_hint(sock_type_hint)
        .build();
    let client = Client::new(&config).map_err(|e| {
        format!(
            "Client::new (sock_type_hint={sock_type_hint:?}): {e} (raw_os_error={:?})",
            e.raw_os_error()
        )
    })?;
    let actual_type = client.get_socket().get_type();
    let mut pinger = client.pinger(addr, PingIdentifier(0xC0DE)).await;
    pinger.timeout(TIMEOUT);
    let (_packet, rtt) = pinger
        .ping(PingSequence(1), &PAYLOAD)
        .await
        .map_err(|e| format!("pinger.ping (socket={actual_type:?}): {e:?}"))?;
    println!("  (surge-ping negotiated socket type: {actual_type:?})");
    Ok(rtt)
}

#[cfg(windows)]
mod winicmp {
    use super::{Duration, Ipv4Addr, Ipv6Addr};
    use std::io;
    use std::mem::size_of;
    use windows::Win32::Foundation::{GetLastError, HANDLE};
    use windows::Win32::NetworkManagement::IpHelper::{
        Icmp6CreateFile, Icmp6ParseReplies, Icmp6SendEcho2, IcmpCloseHandle, IcmpCreateFile,
        IcmpParseReplies, IcmpSendEcho, ICMPV6_ECHO_REPLY_LH, ICMP_ECHO_REPLY, IP_SUCCESS,
    };
    use windows::Win32::Networking::WinSock::{IN6_ADDR, IN6_ADDR_0, SOCKADDR_IN6, AF_INET6};

    fn last_error(context: &str) -> String {
        // SAFETY: GetLastError only reads the calling thread's error slot.
        let code = unsafe { GetLastError() };
        format!(
            "{context}: GetLastError={} ({})",
            code.0,
            io::Error::from_raw_os_error(code.0 as i32)
        )
    }

    fn create_handle(context: &str, raw: windows::core::Result<HANDLE>) -> Result<IcmpHandle, String> {
        match raw {
            Ok(h) => Ok(IcmpHandle(h)),
            Err(e) => Err(format!("{context} failed: {e}; {}", last_error(context))),
        }
    }

    struct IcmpHandle(HANDLE);

    impl Drop for IcmpHandle {
        fn drop(&mut self) {
            unsafe { let _ = IcmpCloseHandle(self.0); }
        }
    }

    pub fn echo_v4(target: Ipv4Addr, payload: &[u8], timeout_ms: u32) -> Result<Duration, String> {
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
            let parsed = IcmpParseReplies(reply_buf.as_mut_ptr().cast(), reply_buf.len() as u32);
            let reply = &*(reply_buf.as_ptr() as *const ICMP_ECHO_REPLY);
            if reply.Status == IP_SUCCESS {
                Ok(Duration::from_millis(u64::from(reply.RoundTripTime)))
            } else {
                Err(format!(
                    "echo reply status={} (parsed replies={parsed})",
                    reply.Status
                ))
            }
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

    pub fn echo_v6(target: Ipv6Addr, payload: &[u8], timeout_ms: u32) -> Result<Duration, String> {
        // SAFETY: all pointers reference live values that outlive the call;
        // the reply buffer is sized to hold ICMPV6_ECHO_REPLY_LH + payload.
        // Event=None and ApcRoutine=None select the synchronous behavior.
        unsafe {
            let handle = create_handle("Icmp6CreateFile", Icmp6CreateFile())?;
            let source = sockaddr_in6(Ipv6Addr::UNSPECIFIED, 0);
            let dest = sockaddr_in6(target, 0);
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
            let parsed = Icmp6ParseReplies(reply_buf.as_mut_ptr().cast(), reply_buf.len() as u32);
            let reply = &*(reply_buf.as_ptr() as *const ICMPV6_ECHO_REPLY_LH);
            if reply.Status == IP_SUCCESS {
                Ok(Duration::from_millis(u64::from(reply.RoundTripTime)))
            } else {
                Err(format!(
                    "echo reply status={} (parsed replies={parsed})",
                    reply.Status
                ))
            }
        }
    }
}

#[tokio::main]
async fn main() {
    println!("ping_spike: unprivileged ICMP echo probe");
    println!("os={} arch={}", std::env::consts::OS, std::env::consts::ARCH);
    println!(
        "surge-ping=0.9 force_raw={}",
        std::env::var_os("PING_SPIKE_FORCE_RAW").is_some()
    );
    println!();

    let mut args = std::env::args().skip(1);
    let v4_target: Ipv4Addr = args
        .next()
        .as_deref()
        .unwrap_or("127.0.0.1")
        .parse()
        .expect("arg1 must be an IPv4 address");
    let v6_target: Ipv6Addr = args
        .next()
        .as_deref()
        .unwrap_or("::1")
        .parse()
        .expect("arg2 must be an IPv6 address");
    println!("targets: v4={v4_target} v6={v6_target}");
    println!();

    print_result(
        "[1/4] surge-ping ICMPv4",
        &v4_target.to_string(),
        surge_probe(IpAddr::V4(v4_target)).await,
    );
    print_result(
        "[2/4] surge-ping ICMPv6",
        &v6_target.to_string(),
        surge_probe(IpAddr::V6(v6_target)).await,
    );

    #[cfg(windows)]
    {
        print_result(
            "[3/4] IPHlpAPI IcmpSendEcho (IPv4)",
            &v4_target.to_string(),
            winicmp::echo_v4(v4_target, &PAYLOAD, TIMEOUT.as_millis() as u32),
        );
        print_result(
            "[4/4] IPHlpAPI Icmp6SendEcho2 (IPv6)",
            &v6_target.to_string(),
            winicmp::echo_v6(v6_target, &PAYLOAD, TIMEOUT.as_millis() as u32),
        );
    }
    #[cfg(not(windows))]
    {
        println!("[3/4] IPHlpAPI IcmpSendEcho (IPv4) -> 127.0.0.1: SKIP (not Windows)");
        println!("[4/4] IPHlpAPI Icmp6SendEcho2 (IPv6) -> ::1: SKIP (not Windows)");
    }
}
