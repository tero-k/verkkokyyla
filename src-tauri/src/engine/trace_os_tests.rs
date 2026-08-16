use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;
use std::time::Duration;

use super::trace_os::{absolute_fallbacks, TraceEngineError, TraceLimits, TraceOs};

fn target() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

#[cfg(windows)]
fn unique_temp_dir(name: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    std::env::temp_dir().join(format!("{name}-{}-{stamp}", std::process::id()))
}

#[cfg(windows)]
fn write_cmd_script(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("@echo off\r\n{body}\r\n")).expect("write cmd script");
    path
}

#[test]
fn resolves_binary_from_fallbacks_after_empty_search_paths() {
    let dir = std::env::temp_dir().join(format!("trace-os-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let resolved = TraceOs::resolve_program(
        "traceroute",
        &[dir],
        &absolute_fallbacks(&["/definitely/not/here"]),
    );

    assert!(matches!(resolved, Err(TraceEngineError::Unavailable(_))));
}

#[cfg(windows)]
#[tokio::test]
async fn trickling_fake_yields_first_hop_before_child_exits() {
    let dir = unique_temp_dir("trace-os-trickle");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let script = write_cmd_script(
        &dir,
        "trace.cmd",
        "echo  1    ^<1 ms    ^<1 ms    ^<1 ms  192.168.1.1\r\nping 127.0.0.1 -n 2 >nul",
    );
    let engine = crate::engine::TracertWin::with_program(
        PathBuf::from("cmd.exe"),
        vec!["/C".to_owned(), script.display().to_string()],
        target(),
    )
    .with_limits(TraceLimits::new(Duration::from_secs(5), Duration::from_secs(2)));
    let mut stream = engine.start().expect("stream");

    let hop = stream.next().await.expect("hop").expect("some hop");
    assert_eq!(hop.hop, 1);
    assert!(stream.child_running().expect("child running"));
}

#[cfg(windows)]
#[tokio::test]
async fn hung_fake_hits_watchdog_with_short_limits() {
    let engine = crate::engine::TracertWin::with_program(
        PathBuf::from("powershell.exe"),
        vec!["-NoProfile".to_owned(), "-Command".to_owned(), "Start-Sleep -Seconds 999".to_owned()],
        target(),
    )
    .with_limits(TraceLimits::new(Duration::from_millis(100), Duration::from_millis(250)));
    let mut stream = engine.start().expect("stream");

    let err = stream.next().await.expect_err("watchdog error");
    assert!(matches!(err, TraceEngineError::Spawn(_)));
}

#[cfg(windows)]
#[tokio::test]
async fn absolute_ceiling_ends_a_long_running_fake() {
    let dir = unique_temp_dir("trace-os-ceiling");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let script = write_cmd_script(
        &dir,
        "trace.cmd",
        "echo  1    ^<1 ms    ^<1 ms    ^<1 ms  192.168.1.1\r\nping 127.0.0.1 -n 3 >nul\r\necho  2    ^<1 ms    ^<1 ms    ^<1 ms  192.168.1.2",
    );
    let engine = crate::engine::TracertWin::with_program(
        PathBuf::from("cmd.exe"),
        vec!["/C".to_owned(), script.display().to_string()],
        target(),
    )
    .with_limits(TraceLimits::new(Duration::from_secs(5), Duration::from_millis(100)));
    let mut stream = engine.start().expect("stream");

    let hop = stream.next().await.expect("first hop").expect("some hop");
    assert_eq!(hop.hop, 1);
    let err = stream.next().await.expect_err("ceiling error");
    assert!(matches!(err, TraceEngineError::Spawn(_)));
}

#[cfg(windows)]
#[tokio::test]
async fn missing_binary_yields_unavailable() {
    let resolved = TraceOs::resolve_program(
        "traceroute",
        &[PathBuf::from(r"C:\definitely-missing-path")],
        &[PathBuf::from(r"C:\definitely-missing-binary\traceroute.exe")],
    );

    assert!(matches!(resolved, Err(TraceEngineError::Unavailable(_))));
}
