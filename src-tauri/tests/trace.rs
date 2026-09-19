mod trace {
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr};
    use std::path::{Path, PathBuf};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Duration;

    use futures_util::future::BoxFuture;
    use tokio::sync::{mpsc, oneshot, Mutex, Notify};

    use verkkokyyla_lib::db::Database;
    use verkkokyyla_lib::engine::trace_parse::RawHop;
    use verkkokyyla_lib::engine::TraceEngineError;
    use verkkokyyla_lib::trace::{
        TraceError, TraceEvent, TraceFactory, TraceManager, TraceResolver, TraceStatusEvent,
        TraceStream,
    };

    type OptionalHopRx = Option<mpsc::Receiver<Result<Option<RawHop>, TraceEngineError>>>;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "verkkokyyla-trace-{name}-{}-{stamp}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }

        fn db_file(&self) -> PathBuf {
            self.0.join("nested").join("trace.db")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct ScriptedTraceStream {
        rx: mpsc::Receiver<Result<Option<RawHop>, TraceEngineError>>,
    }

    impl TraceStream for ScriptedTraceStream {
        fn next<'a>(
            &'a mut self,
        ) -> futures_util::future::BoxFuture<'a, Result<Option<RawHop>, TraceEngineError>> {
            Box::pin(async move { self.rx.recv().await.unwrap_or(Ok(None)) })
        }
    }

    fn raw_hop(hop: u32, address: Option<&str>) -> RawHop {
        RawHop {
            hop,
            address: address.map(str::to_owned),
            rtts: vec![Some(1.0), Some(2.0), Some(3.0)],
            annotation: None,
        }
    }

    async fn test_db(path: &Path) -> Database {
        Database::connect(path).await.expect("db")
    }

    fn event_sink() -> (
        Arc<dyn Fn(TraceEvent) + Send + Sync>,
        mpsc::UnboundedReceiver<TraceEvent>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Arc::new(move |event| {
                let _ = tx.send(event);
            }),
            rx,
        )
    }

    fn status_sink() -> (
        Arc<dyn Fn(TraceStatusEvent) + Send + Sync>,
        mpsc::UnboundedReceiver<TraceStatusEvent>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Arc::new(move |event| {
                let _ = tx.send(event);
            }),
            rx,
        )
    }

    fn scripted_factory(rx: Arc<Mutex<OptionalHopRx>>) -> TraceFactory {
        Arc::new(move |_| {
            let rx = Arc::clone(&rx);
            Box::pin(async move {
                let mut guard = rx.lock().await;
                let rx = guard.take().expect("stream used once");
                Ok(Box::new(ScriptedTraceStream { rx }) as Box<dyn TraceStream>)
            })
        })
    }

    fn immediate_resolver(map: HashMap<IpAddr, Option<String>>) -> TraceResolver {
        Arc::new(move |addr| {
            let value = map.get(&addr).cloned().unwrap_or(None);
            Box::pin(async move { value }) as BoxFuture<'static, Option<String>>
        })
    }

    #[tokio::test]
    async fn trace_completes_and_persists_hostnames() {
        let dir = TempDir::new("complete");
        let manager_db = test_db(&dir.db_file()).await;
        let inspector_db = test_db(&dir.db_file()).await;
        let (tx, rx) = mpsc::channel(8);
        let factory = scripted_factory(Arc::new(Mutex::new(Some(rx))));
        let resolver = immediate_resolver(HashMap::from([
            (
                IpAddr::V4(Ipv4Addr::new(192, 0, 2, 1)),
                Some("edge.example".to_owned()),
            ),
            (IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1)), None),
            (
                IpAddr::V4(Ipv4Addr::new(203, 0, 113, 10)),
                Some("target.example".to_owned()),
            ),
        ]));
        let manager = TraceManager::new(manager_db, factory, resolver);
        let (on_event, mut events) = event_sink();
        let (on_status, mut statuses) = status_sink();
        let start = manager
            .start(
                "203.0.113.10",
                "auto",
                move |event| on_event(event),
                on_status,
            )
            .await
            .expect("start");
        assert_eq!(start.resolved_ip, "203.0.113.10");
        tx.send(Ok(Some(raw_hop(1, Some("192.0.2.1")))))
            .await
            .expect("hop 1");
        tx.send(Ok(Some(raw_hop(2, Some("198.51.100.1")))))
            .await
            .expect("hop 2");
        tx.send(Ok(Some(raw_hop(3, Some("203.0.113.10")))))
            .await
            .expect("hop 3");
        tx.send(Ok(None)).await.expect("end");

        let status = statuses.recv().await.expect("status");
        assert!(
            matches!(status, TraceStatusEvent::Completed { trace_id, hop_count: 3, reached_target: true } if trace_id == start.trace_id)
        );

        let mut hostnames = Vec::new();
        while hostnames.len() < 2 {
            if let Some(TraceEvent::Hostname {
                hostname: Some(name),
                ..
            }) = tokio::time::timeout(Duration::from_secs(30), events.recv())
                .await
                .expect("hostname event")
            {
                hostnames.push(name);
            }
        }
        assert!(hostnames.iter().any(|name| name == "edge.example"));
        assert!(hostnames.iter().any(|name| name == "target.example"));

        let traces = inspector_db.list_traces().await.expect("list traces");
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].status, "completed");
        assert_eq!(traces[0].hop_count, 3);
        let hops = inspector_db
            .load_trace_hops(start.trace_id)
            .await
            .expect("load hops");
        assert_eq!(hops.len(), 3);
    }

    #[tokio::test]
    async fn trace_rejects_second_start_and_stop_cancels_partial_hops() {
        let dir = TempDir::new("cancel");
        let manager_db = test_db(&dir.db_file()).await;
        let inspector_db = test_db(&dir.db_file()).await;
        let (tx, rx) = mpsc::channel(8);
        let factory = scripted_factory(Arc::new(Mutex::new(Some(rx))));
        let resolver = immediate_resolver(HashMap::new());
        let manager = TraceManager::new(manager_db, factory, resolver);
        let (on_event, mut events) = event_sink();
        let (on_status, mut statuses) = status_sink();
        let start = manager
            .start(
                "203.0.113.10",
                "auto",
                move |event| on_event(event),
                on_status,
            )
            .await
            .expect("start");
        tx.send(Ok(Some(raw_hop(1, Some("192.0.2.1")))))
            .await
            .expect("hop");
        assert!(matches!(
            manager
                .start("203.0.113.11", "auto", |_| {}, Arc::new(|_| {}))
                .await,
            Err(TraceError::AlreadyRunning)
        ));

        let _ = events.recv().await.expect("hop event");
        let stopped = manager.stop().await.expect("stop");
        assert_eq!(stopped.trace_id, start.trace_id);
        assert_eq!(stopped.hop_count, 1);
        assert!(
            matches!(statuses.recv().await.expect("status"), TraceStatusEvent::Cancelled { trace_id, hop_count: 1 } if trace_id == start.trace_id)
        );

        let hops = inspector_db
            .load_trace_hops(start.trace_id)
            .await
            .expect("load hops");
        assert_eq!(hops.len(), 1);
    }

    #[tokio::test]
    async fn trace_emits_late_hostname_updates_after_completion() {
        let dir = TempDir::new("late-hostname");
        let manager_db = test_db(&dir.db_file()).await;
        let inspector_db = test_db(&dir.db_file()).await;
        let (tx, rx) = mpsc::channel(8);
        let factory = scripted_factory(Arc::new(Mutex::new(Some(rx))));
        let (release_tx, release_rx) = oneshot::channel::<()>();
        let resolver = Arc::new(Mutex::new(Some(release_rx)));
        let resolver = Arc::new(move |addr| {
            let resolver = Arc::clone(&resolver);
            Box::pin(async move {
                let rx = resolver.lock().await.take().expect("resolver once");
                let _ = rx.await;
                Some(format!("host-{}", addr))
            }) as BoxFuture<'static, Option<String>>
        });
        let manager = TraceManager::new(manager_db, factory, resolver);
        let (on_event, mut events) = event_sink();
        let (on_status, mut statuses) = status_sink();
        let start = manager
            .start(
                "203.0.113.10",
                "auto",
                move |event| on_event(event),
                on_status,
            )
            .await
            .expect("start");
        tx.send(Ok(Some(raw_hop(1, Some("192.0.2.1")))))
            .await
            .expect("hop");
        tx.send(Ok(None)).await.expect("end");
        assert!(
            matches!(statuses.recv().await.expect("status"), TraceStatusEvent::Completed { trace_id, hop_count: 1, reached_target: false } if trace_id == start.trace_id)
        );

        let before = inspector_db
            .load_trace_hops(start.trace_id)
            .await
            .expect("load before");
        assert_eq!(before[0].hostname, None);
        release_tx.send(()).expect("release resolver");

        let hostname = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if let Some(TraceEvent::Hostname {
                    hostname: Some(name),
                    ..
                }) = events.recv().await
                {
                    break name;
                }
            }
        })
        .await
        .expect("hostname event");
        assert!(hostname.starts_with("host-"));

        let after = inspector_db
            .load_trace_hops(start.trace_id)
            .await
            .expect("load after");
        assert!(after[0].hostname.as_deref().unwrap().starts_with("host-"));
    }

    #[tokio::test]
    async fn trace_rejects_idle_stop() {
        let dir = TempDir::new("idle-stop");
        let manager = TraceManager::new(
            test_db(&dir.db_file()).await,
            Arc::new(|_| Box::pin(async { unreachable!() })),
            Arc::new(|_| Box::pin(async { None })),
        );
        assert!(matches!(
            manager.stop().await,
            Err(TraceError::NoActiveTrace)
        ));
    }

    #[tokio::test]
    async fn trace_reports_unavailable_engine_without_persisting_row() {
        let dir = TempDir::new("unavailable");
        let manager_db = test_db(&dir.db_file()).await;
        let inspector_db = test_db(&dir.db_file()).await;
        let factory = Arc::new(|_| {
            Box::pin(async { Err(TraceEngineError::Unavailable("missing".to_owned())) })
                as futures_util::future::BoxFuture<
                    'static,
                    Result<Box<dyn TraceStream>, TraceEngineError>,
                >
        });
        let manager =
            TraceManager::new(manager_db, factory, Arc::new(|_| Box::pin(async { None })));
        let (on_event, _) = event_sink();
        let (on_status, mut statuses) = status_sink();
        assert!(matches!(
            manager
                .start(
                    "203.0.113.10",
                    "auto",
                    move |event| on_event(event),
                    on_status
                )
                .await,
            Err(TraceError::Stream(TraceEngineError::Unavailable(_)))
        ));
        assert!(matches!(
            statuses.recv().await.expect("status"),
            TraceStatusEvent::Error { .. }
        ));
        assert!(inspector_db.list_traces().await.expect("list").is_empty());
    }

    #[tokio::test]
    async fn trace_caps_hostname_concurrency_at_four() {
        let dir = TempDir::new("concurrency");
        let manager_db = test_db(&dir.db_file()).await;
        let inspector_db = test_db(&dir.db_file()).await;
        let (tx, rx) = mpsc::channel(16);
        let factory = scripted_factory(Arc::new(Mutex::new(Some(rx))));
        let notify = Arc::new(Notify::new());
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let resolver = {
            let notify = Arc::clone(&notify);
            let active = Arc::clone(&active);
            let peak = Arc::clone(&peak);
            Arc::new(move |addr| {
                let notify = Arc::clone(&notify);
                let active = Arc::clone(&active);
                let peak = Arc::clone(&peak);
                Box::pin(async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(current, Ordering::SeqCst);
                    notify.notified().await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    Some(format!("host-{}", addr))
                }) as BoxFuture<'static, Option<String>>
            })
        };
        let manager = TraceManager::new(manager_db, factory, resolver);
        let (on_event, _) = event_sink();
        let (on_status, mut statuses) = status_sink();
        let _ = manager
            .start(
                "203.0.113.10",
                "auto",
                move |event| on_event(event),
                on_status,
            )
            .await
            .expect("start");
        for index in 1..=6u8 {
            let ip = format!("192.0.2.{index}");
            tx.send(Ok(Some(raw_hop(index as u32, Some(&ip)))))
                .await
                .expect("hop");
        }
        tx.send(Ok(None)).await.expect("end");
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(peak.load(Ordering::SeqCst) <= 4);
        notify.notify_waiters();
        assert!(matches!(
            statuses.recv().await.expect("status"),
            TraceStatusEvent::Completed { hop_count: 6, .. }
        ));
        let hops = inspector_db.load_trace_hops(1).await.expect("load hops");
        assert_eq!(hops.len(), 6);
    }
}
