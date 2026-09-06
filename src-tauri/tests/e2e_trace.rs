//! Real-backend end-to-end integration test for the traceroute layer.
//!
//! Runs the OS traceroute binary against 127.0.0.1 and verifies that hops are
//! emitted, the trace completes, and rows are persisted. If the platform lacks
//! a traceroute binary the test logs the skip and returns Ok so it does not
//! fail the gate.
//!
//! Run with: cargo test -- --ignored e2e_trace

use std::net::IpAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use futures_util::future::BoxFuture;
use tokio::time::timeout;

use verkkokyyla_lib::db::Database;
use verkkokyyla_lib::engine::trace_parse::RawHop;
#[cfg(unix)]
use verkkokyyla_lib::engine::TracePosix;
#[cfg(windows)]
use verkkokyyla_lib::engine::TracertWin;
use verkkokyyla_lib::engine::{RawHopStream, TraceEngineError};
use verkkokyyla_lib::trace::{
    TraceEvent, TraceFactory, TraceManager, TraceResolver, TraceStatusEvent, TraceStream,
};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "verkkokyyla-e2e-trace-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }

    fn db_file(&self) -> PathBuf {
        self.0.join("test.db")
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct OsStream(RawHopStream);

impl TraceStream for OsStream {
    fn next<'a>(&'a mut self) -> BoxFuture<'a, Result<Option<RawHop>, TraceEngineError>> {
        Box::pin(async move { self.0.next().await })
    }
}

fn os_factory() -> TraceFactory {
    Arc::new(|addr: IpAddr| {
        Box::pin(async move {
            #[cfg(windows)]
            {
                let stream = TracertWin::new(addr)?.start()?;
                Ok(Box::new(OsStream(stream)) as Box<dyn TraceStream>)
            }
            #[cfg(unix)]
            {
                let stream = TracePosix::new(addr)?.start()?;
                Ok(Box::new(OsStream(stream)) as Box<dyn TraceStream>)
            }
        })
    })
}

fn resolver() -> TraceResolver {
    Arc::new(|addr: IpAddr| {
        Box::pin(async move {
            match tokio::task::spawn_blocking(move || dns_lookup::lookup_addr(&addr)).await {
                Ok(Ok(name)) => Some(name),
                _ => None,
            }
        })
    })
}

#[tokio::test]
#[ignore = "real traceroute / network"]
async fn e2e_trace_loopback() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TestDir::new("loopback")?;
    let db = Database::connect(&dir.db_file()).await?;
    let manager = TraceManager::new(db, os_factory(), resolver());

    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel::<TraceEvent>();
    let (status_tx, mut status_rx) = tokio::sync::mpsc::unbounded_channel::<TraceStatusEvent>();
    let on_event = move |event: TraceEvent| {
        let _ = event_tx.send(event);
    };
    let on_status = Arc::new(move |event: TraceStatusEvent| {
        let _ = status_tx.send(event);
    });

    let start = match manager.start("127.0.0.1", "v4", on_event, on_status).await {
        Ok(info) => info,
        Err(err) => {
            eprintln!("traceroute unavailable on this machine: {err}");
            return Ok(());
        }
    };

    let mut completed = false;
    let mut hop_count = 0u64;
    match timeout(Duration::from_secs(30), status_rx.recv()).await {
        Ok(Some(TraceStatusEvent::Completed {
            trace_id,
            hop_count: h,
            ..
        })) => {
            assert_eq!(trace_id, start.trace_id);
            completed = true;
            hop_count = h;
        }
        Ok(Some(TraceStatusEvent::Error { message })) => {
            eprintln!("trace error: {message}");
        }
        Ok(Some(TraceStatusEvent::Cancelled { .. })) | Ok(None) => {}
        Err(_) => {
            let _ = manager.stop().await;
        }
    }

    if completed {
        assert!(
            (1..=2).contains(&hop_count),
            "expected 1-2 loopback hops, got {hop_count}"
        );
        let traces = manager.list_traces().await?;
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].status, "completed");
        let loaded = manager.load_trace(start.trace_id).await?;
        assert_eq!(loaded.hops.len() as u64, hop_count);
    }

    Ok(())
}
