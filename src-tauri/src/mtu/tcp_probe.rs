//! TCP PLPMTUD probe engine (RFC 4821 style): connects to a TCP port with
//! PMTU discovery in "do" mode, then binary searches the largest request the
//! path carries. See `.omo/plans/mtu-discovery.md` milestone 3.
//!
//! Platform support: Linux only. Linux refuses oversized writes with
//! `EMSGSIZE` under `IP_PMTUDISC_DO` and exposes the kernel-cached path MTU
//! via `getsockopt(IP_MTU)`, which becomes the RFC 1191-style hint. On
//! Windows, `IP_DONTFRAGMENT` on a TCP socket does NOT make `send` atomic —
//! the stack silently segments writes at its own PMTU estimate, so oversized
//! probes appear to succeed and the measurement would be fiction. macOS has
//! no supported way to set DF on TCP at all. Both therefore report
//! `EngineError::Unavailable` from `connect`.

use std::io;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use super::types::ProbeOutcome;
use crate::engine::EngineError;

const LINUX_EMSGSIZE: i32 = 90;
const WINDOWS_WSAEMSGSIZE: i32 = 10040;
const REQUEST_PREFIX_START: &str = "HEAD / HTTP/1.1\r\nHost: ";
const REQUEST_PREFIX_END: &str = "\r\nX-Pad: ";
const REQUEST_SUFFIX: &str = "\r\n\r\n";

/// Connected TCP PLPMTUD prober.
#[derive(Debug)]
pub struct TcpMtuProber {
    target: IpAddr,
    port: u16,
    timeout: Duration,
    target_name: String,
    stream: TcpStream,
}

impl TcpMtuProber {
    /// Connect to `target:port`, configure the socket for DF probing, then hand
    /// it to Tokio. Socket creation is sync because the platform DF options are
    /// sync `setsockopt` calls and the manager constructs engines off the hot
    /// probe loop.
    pub fn connect(target: IpAddr, port: u16, timeout: Duration) -> Result<Self, EngineError> {
        let target_name = target.to_string();
        let stream = open_stream(target, port, timeout)?;
        Ok(Self {
            target,
            port,
            timeout,
            target_name,
            stream,
        })
    }

    /// Send one padded HEAD request. A successful write is confirmed by either
    /// response bytes or a clean peer EOF; if the persistent connection died,
    /// the same probe is retried once on a fresh DF-enabled connection.
    pub async fn probe(&mut self, payload_size: usize) -> ProbeOutcome {
        let first = self.probe_once(payload_size).await;
        if matches!(first, ProbeOutcome::Error(_)) {
            match self.reconnect() {
                Ok(()) => self.probe_once(payload_size).await,
                Err(err) => ProbeOutcome::Error(err.to_string()),
            }
        } else {
            first
        }
    }

    fn reconnect(&mut self) -> Result<(), EngineError> {
        self.stream = open_stream(self.target, self.port, self.timeout)?;
        Ok(())
    }

    #[cfg(test)]
    fn from_connected(target: IpAddr, port: u16, timeout: Duration, stream: TcpStream) -> Self {
        Self {
            target,
            port,
            timeout,
            target_name: target.to_string(),
            stream,
        }
    }

    async fn probe_once(&mut self, payload_size: usize) -> ProbeOutcome {
        let request = build_request(&self.target_name, payload_size);
        let started = Instant::now();

        match timeout(self.timeout, self.stream.write_all(&request)).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return self.classify_write_error(&err),
            Err(_elapsed) => return ProbeOutcome::Timeout,
        }

        let mut buf = [0u8; 1];
        match timeout(self.timeout, self.stream.read(&mut buf)).await {
            Ok(Ok(_bytes_read)) => ProbeOutcome::Ok {
                rtt: started.elapsed(),
            },
            Ok(Err(err)) => classify_io_error(&err),
            Err(_elapsed) => ProbeOutcome::Timeout,
        }
    }

    /// Map a failed write: EMSGSIZE under Linux `IP_PMTUDISC_DO` means the
    /// write exceeded the kernel-cached path MTU; the kernel's `IP_MTU` value
    /// is the next-hop estimate and rides along as the hint.
    #[cfg(target_os = "linux")]
    fn classify_write_error(&self, err: &io::Error) -> ProbeOutcome {
        match err.raw_os_error() {
            Some(LINUX_EMSGSIZE) => ProbeOutcome::TooBig {
                hint_mtu: self.current_path_mtu(),
            },
            _ => classify_io_error(err),
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn classify_write_error(&self, err: &io::Error) -> ProbeOutcome {
        classify_io_error(err)
    }

    /// Kernel-cached path MTU (`getsockopt(IP_MTU)`), when the stack has one.
    #[cfg(target_os = "linux")]
    fn current_path_mtu(&self) -> Option<u32> {
        use std::os::fd::AsRawFd;

        let mut mtu: libc::c_int = 0;
        let mut len = libc::socklen_t::try_from(std::mem::size_of_val(&mtu)).ok()?;
        // SAFETY: [Category 8 - FFI boundary] `self.stream` is a live TCP
        // socket, `&mut mtu` is an initialized `c_int` buffer of `len` bytes
        // for the duration of the call, and the pointer is not retained.
        let result = unsafe {
            libc::getsockopt(
                self.stream.as_raw_fd(),
                libc::IPPROTO_IP,
                libc::IP_MTU,
                std::ptr::addr_of_mut!(mtu).cast(),
                &mut len,
            )
        };
        if result == -1 || mtu <= 0 {
            None
        } else {
            u32::try_from(mtu).ok()
        }
    }
}

fn open_stream(target: IpAddr, port: u16, timeout: Duration) -> Result<TcpStream, EngineError> {
    let IpAddr::V4(_) = target else {
        return Err(EngineError::Unavailable(
            "TCP MTU probing is not supported for IPv6 yet".to_owned(),
        ));
    };
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))
        .map_err(EngineError::Socket)?;
    configure_df(&socket)?;
    let addr = SockAddr::from(std::net::SocketAddr::new(target, port));
    socket
        .connect_timeout(&addr, timeout)
        .map_err(EngineError::Socket)?;
    let stream: std::net::TcpStream = socket.into();
    stream.set_nonblocking(true).map_err(EngineError::Socket)?;
    TcpStream::from_std(stream).map_err(EngineError::Socket)
}

/// Build an exactly sized HTTP request. When `payload_size` is smaller than the
/// mandatory request skeleton, the skeleton size is the minimum legal probe.
fn build_request(target: &str, payload_size: usize) -> Vec<u8> {
    let prefix = format!("{REQUEST_PREFIX_START}{target}{REQUEST_PREFIX_END}");
    let minimum_size = prefix.len() + REQUEST_SUFFIX.len();
    let request_size = payload_size.max(minimum_size);
    let pad_len = request_size - minimum_size;

    let mut request = Vec::with_capacity(request_size);
    request.extend_from_slice(prefix.as_bytes());
    request.resize(prefix.len() + pad_len, b'a');
    request.extend_from_slice(REQUEST_SUFFIX.as_bytes());
    request
}

fn classify_io_error(err: &io::Error) -> ProbeOutcome {
    match err.raw_os_error() {
        Some(LINUX_EMSGSIZE | WINDOWS_WSAEMSGSIZE) => ProbeOutcome::TooBig { hint_mtu: None },
        Some(_) | None => match err.kind() {
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => ProbeOutcome::Timeout,
            _ => ProbeOutcome::Error(err.to_string()),
        },
    }
}

#[cfg(target_os = "linux")]
fn invalid_input(message: &'static str) -> EngineError {
    EngineError::Socket(io::Error::new(io::ErrorKind::InvalidInput, message))
}

#[cfg(windows)]
fn configure_df(_socket: &Socket) -> Result<(), EngineError> {
    // Windows TCP ignores DF semantics for write atomicity: the stack
    // segments any write at its own PMTU estimate, so an oversized probe
    // "succeeds" and the measurement would report the ceiling, not the path.
    // Refuse rather than fabricate a result.
    Err(EngineError::Unavailable(
        "TCP MTU probing is not supported on Windows: the TCP stack segments \
         writes itself, so oversized probes appear to succeed"
            .to_owned(),
    ))
}

#[cfg(target_os = "linux")]
fn configure_df(socket: &Socket) -> Result<(), EngineError> {
    use std::mem::size_of_val;
    use std::os::fd::AsRawFd;

    // "Do" mode (not "probe"): the kernel refuses writes larger than the
    // cached path MTU with EMSGSIZE and updates the cache from ICMP PTB
    // messages, which is exactly the feedback loop this engine needs.
    let mode = libc::IP_PMTUDISC_DO;
    let opt_len = libc::socklen_t::try_from(size_of_val(&mode))
        .map_err(|_| invalid_input("invalid socket option length"))?;
    // SAFETY: [Category 8 - FFI boundary] `socket.as_raw_fd()` is a live TCP
    // socket, `&mode` points to an initialized `c_int`, and the pointer is not
    // retained after `setsockopt` returns.
    let result = unsafe {
        libc::setsockopt(
            socket.as_raw_fd(),
            libc::IPPROTO_IP,
            libc::IP_MTU_DISCOVER,
            std::ptr::addr_of!(mode).cast(),
            opt_len,
        )
    };
    if result == -1 {
        Err(EngineError::Socket(io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
fn configure_df(_socket: &Socket) -> Result<(), EngineError> {
    Err(EngineError::Unavailable(
        "TCP MTU probing is not supported on this platform".to_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener as StdTcpListener;

    use super::*;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    /// Connect a prober to a local fixture without platform DF setup, so the
    /// platform-agnostic probe semantics can be tested on every OS.
    async fn connect_fixture(ip: IpAddr, port: u16, timeout: Duration) -> TcpMtuProber {
        let std_stream = std::net::TcpStream::connect((ip, port)).expect("fixture connect");
        std_stream.set_nonblocking(true).expect("nonblocking");
        let stream = TcpStream::from_std(std_stream).expect("tokio stream");
        TcpMtuProber::from_connected(ip, port, timeout, stream)
    }

    fn spawn_read_and_close(listener: TcpListener, min_read: usize) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept client");
            let mut buf = vec![0u8; 512];
            let read = stream.read(&mut buf).await.expect("read request");
            assert!(read >= min_read);
        })
    }

    // Given several requested payload sizes,
    // When a padded HTTP probe request is built,
    // Then the request length is exact and the framing is preserved.
    #[test]
    fn build_request_returns_exact_payload_size_when_size_exceeds_skeleton() {
        for payload_size in [64usize, 128, 512, 1472] {
            let request = build_request("127.0.0.1", payload_size);

            assert_eq!(request.len(), payload_size);
            assert!(request.starts_with(b"HEAD / HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Pad: "));
            assert!(request.ends_with(b"\r\n\r\n"));
        }
    }

    // Given a requested payload smaller than the required HTTP skeleton,
    // When a padded HTTP probe request is built,
    // Then the skeleton is emitted as the minimum legal request size.
    #[test]
    fn build_request_clamps_to_skeleton_when_payload_is_too_small() {
        let request = build_request("127.0.0.1", 1);
        let expected = b"HEAD / HTTP/1.1\r\nHost: 127.0.0.1\r\nX-Pad: \r\n\r\n";

        assert_eq!(request.len(), expected.len());
        assert_eq!(request, expected);
    }

    // Given synthetic OS errors from Linux and Windows oversized sends,
    // When the TCP error classifier maps them,
    // Then they become TooBig without an MTU hint.
    #[test]
    fn classify_io_error_returns_too_big_for_message_size_errors() {
        for code in [LINUX_EMSGSIZE, WINDOWS_WSAEMSGSIZE] {
            let err = io::Error::from_raw_os_error(code);

            assert_eq!(
                classify_io_error(&err),
                ProbeOutcome::TooBig { hint_mtu: None }
            );
        }
    }

    // Given timeout-shaped I/O errors,
    // When the TCP error classifier maps them,
    // Then they remain Timeout rather than evidence of a too-big packet.
    #[test]
    fn classify_io_error_returns_timeout_for_timeout_kinds() {
        let err = io::Error::from(io::ErrorKind::TimedOut);

        assert_eq!(classify_io_error(&err), ProbeOutcome::Timeout);
    }

    // Given synthetic connection-refused errors from common Unix platforms,
    // When the TCP error classifier maps them,
    // Then they stay transport errors.
    #[test]
    fn classify_io_error_returns_error_for_connection_refused_codes() {
        for code in [111, 61] {
            let err = io::Error::from_raw_os_error(code);

            assert!(matches!(classify_io_error(&err), ProbeOutcome::Error(_)));
        }
    }

    // Given a local server that reads one request and closes cleanly,
    // When a TCP MTU probe is sent,
    // Then EOF confirms the bytes were carried end-to-end.
    #[tokio::test]
    async fn probe_returns_ok_when_server_reads_request_and_closes() {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        let server = spawn_read_and_close(listener, 1);
        let mut prober = connect_fixture(addr.ip(), addr.port(), Duration::from_millis(250)).await;

        let outcome = prober.probe(96).await;

        assert!(matches!(outcome, ProbeOutcome::Ok { .. }));
        let _ = server.await;
    }

    // Given a local server that accepts but keeps the connection open,
    // When a TCP MTU probe waits for read confirmation,
    // Then the bounded read returns Timeout rather than TooBig.
    #[tokio::test]
    async fn probe_returns_timeout_when_server_keeps_connection_open() {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind listener");
        let addr = listener.local_addr().expect("listener address");
        let (release_tx, release_rx) = oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept client");
            let _ = release_rx.await;
        });
        let mut prober = connect_fixture(addr.ip(), addr.port(), Duration::from_millis(150)).await;

        let outcome = prober.probe(96).await;

        assert_eq!(outcome, ProbeOutcome::Timeout);
        let _ = release_tx.send(());
        let _ = server.await;
    }

    // Given a closed local port,
    // When the TCP prober constructor attempts to connect,
    // Then it returns an EngineError instead of panicking. On platforms
    // without TCP probe support the error is the honest Unavailable kind.
    #[test]
    fn connect_returns_error_for_closed_local_port() {
        let listener = StdTcpListener::bind(("127.0.0.1", 0)).expect("bind closed-port fixture");
        let addr = listener.local_addr().expect("fixture address");
        drop(listener);

        let result = TcpMtuProber::connect(addr.ip(), addr.port(), Duration::from_millis(100));

        assert!(result.is_err());
    }

    // Given Windows and macOS cannot measure the path through a TCP socket,
    // When the prober is constructed there,
    // Then connect refuses with Unavailable instead of fabricating results.
    #[cfg(all(windows, test))]
    #[test]
    fn connect_reports_unavailable_on_windows() {
        let err = TcpMtuProber::connect(
            IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
            443,
            Duration::from_millis(100),
        )
        .expect_err("windows must refuse TCP probing");

        assert!(matches!(err, EngineError::Unavailable(_)));
    }
}
