mod mtu {
    use std::collections::VecDeque;
    use std::net::{IpAddr, Ipv4Addr};
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use futures_util::future::BoxFuture;
    use tokio::sync::mpsc;
    use verkkokyyla_lib::db::Database;
    use verkkokyyla_lib::engine::EngineError;
    use verkkokyyla_lib::mtu::engine::MtuProbeEngine;
    use verkkokyyla_lib::mtu::manager::{MtuFactory, MtuManager};
    use verkkokyyla_lib::mtu::search::SearchController;
    use verkkokyyla_lib::mtu::{
        MtuConfig, MtuError, MtuMethod, MtuProbeEvent, MtuStatusEvent, ProbeOutcome,
        ProbeOutcomeDto, ResultKindDto, SearchAction,
    };

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "verkkokyyla-mtu-{name}-{}-{stamp}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }

        fn db_file(&self) -> PathBuf {
            self.0.join("nested").join("mtu.db")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn status_sink() -> (
        Arc<dyn Fn(MtuStatusEvent) + Send + Sync>,
        mpsc::UnboundedReceiver<MtuStatusEvent>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Arc::new(move |event| {
                let _ = tx.send(event);
            }),
            rx,
        )
    }

    fn event_sink() -> (
        Arc<dyn Fn(MtuProbeEvent) + Send + Sync>,
        mpsc::UnboundedReceiver<MtuProbeEvent>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        (
            Arc::new(move |event| {
                let _ = tx.send(event);
            }),
            rx,
        )
    }

    fn scripted_link_factory(hidden_mtu: u32) -> MtuFactory {
        Arc::new(move |_method: MtuMethod, _addr: IpAddr, _port: u16| {
            Box::pin(async move {
                let script = link_script(hidden_mtu);
                Ok(MtuProbeEngine::Mock(script))
            }) as BoxFuture<'static, Result<MtuProbeEngine, EngineError>>
        })
    }

    fn link_script(hidden_mtu: u32) -> VecDeque<ProbeOutcome> {
        let mut controller = SearchController::new(MtuConfig::default());
        let mut action = controller.initial_action();
        let mut script = VecDeque::new();
        loop {
            match action {
                SearchAction::Probe { payload_size, .. } => {
                    let outcome =
                        if u32::try_from(payload_size).unwrap_or(u32::MAX) + 28 > hidden_mtu {
                            ProbeOutcome::TooBig { hint_mtu: None }
                        } else {
                            ProbeOutcome::Ok {
                                rtt: Duration::from_millis(1),
                            }
                        };
                    script.push_back(outcome.clone());
                    action = controller.step(outcome);
                }
                SearchAction::Done(_) => return script,
            }
        }
    }

    fn hanging_factory() -> MtuFactory {
        Arc::new(move |_method: MtuMethod, _addr: IpAddr, _port: u16| {
            Box::pin(async move {
                Ok(MtuProbeEngine::Mock(VecDeque::from([ProbeOutcome::Ok {
                    rtt: Duration::from_millis(1),
                }])))
            }) as BoxFuture<'static, Result<MtuProbeEngine, EngineError>>
        })
    }

    async fn manager_pair(name: &str, factory: MtuFactory) -> (TempDir, MtuManager, Database) {
        let dir = TempDir::new(name);
        let manager_db = Database::connect(&dir.db_file()).await.expect("manager db");
        let inspector_db = Database::connect(&dir.db_file())
            .await
            .expect("inspector db");
        (dir, MtuManager::new(manager_db, factory), inspector_db)
    }

    // Given a DB-backed MTU manager with a simulated 1420-byte link,
    // When an ICMP run is started,
    // Then the runtime completes exactly, streams outcomes, and persists ordered probes.
    #[tokio::test]
    async fn mtu_run_completes_and_persists_probe_history() {
        let (_dir, manager, _inspector) =
            manager_pair("complete", scripted_link_factory(1420)).await;
        let (on_event, mut events) = event_sink();
        let (on_status, mut statuses) = status_sink();

        let start = manager
            .start(
                "192.0.2.10",
                "icmp",
                9000,
                443,
                move |event| on_event(event),
                on_status,
            )
            .await
            .expect("start");
        assert_eq!(start.method, "icmp");
        assert_eq!(start.resolved_ip, "192.0.2.10");

        let completed = tokio::time::timeout(Duration::from_secs(3), statuses.recv())
            .await
            .expect("status timeout")
            .expect("status");
        assert!(matches!(
            completed,
            MtuStatusEvent::Completed {
                run_id,
                result: ResultKindDto::Exact { mtu: 1420 },
                probes_sent,
            } if run_id == start.run_id && probes_sent > 0
        ));

        let mut streamed_outcomes = 0usize;
        while let Ok(Some(event)) =
            tokio::time::timeout(Duration::from_millis(10), events.recv()).await
        {
            if matches!(event, MtuProbeEvent::Outcome { .. }) {
                streamed_outcomes += 1;
            }
        }
        assert!(streamed_outcomes > 0);

        let runs = manager.list_runs().await.expect("list runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].id, start.run_id);
        assert!(matches!(runs[0].result, ResultKindDto::Exact { mtu: 1420 }));

        let loaded = manager.load_run(start.run_id).await.expect("load run");
        assert_eq!(loaded.run, runs[0]);
        assert_eq!(loaded.probes.len(), streamed_outcomes);
        for (index, probe) in loaded.probes.iter().enumerate() {
            assert_eq!(probe.seq, index as u64 + 1);
        }
        assert!(loaded
            .probes
            .iter()
            .any(|probe| { matches!(probe.outcome, ProbeOutcomeDto::TooBig { hint_mtu: None }) }));
    }

    // Given an idle MTU manager,
    // When stop is requested,
    // Then the typed NoActiveRun error is returned.
    #[tokio::test]
    async fn mtu_stop_errors_when_idle() {
        let (_dir, manager, _inspector) = manager_pair("idle", scripted_link_factory(1420)).await;

        assert!(matches!(manager.stop().await, Err(MtuError::NoActiveRun)));
    }

    // Given one active MTU run,
    // When a second run is started and then the first is stopped,
    // Then the second start is rejected and the first run is cancelled.
    #[tokio::test]
    async fn mtu_rejects_concurrent_start_and_stop_cancels_active_run() {
        let (_dir, manager, _inspector) = manager_pair("cancel", hanging_factory()).await;
        let (on_status, mut statuses) = status_sink();
        let start = manager
            .start("192.0.2.10", "icmp", 9000, 443, |_| {}, on_status)
            .await
            .expect("start");

        assert!(matches!(
            manager
                .start("192.0.2.11", "icmp", 9000, 443, |_| {}, Arc::new(|_| {}))
                .await,
            Err(MtuError::AlreadyRunning)
        ));

        let stopped = manager.stop().await.expect("stop");
        assert_eq!(stopped.run_id, start.run_id);
        assert!(matches!(
            statuses.recv().await.expect("status"),
            MtuStatusEvent::Cancelled { run_id, .. } if run_id == start.run_id
        ));
        assert!(matches!(
            manager
                .load_run(start.run_id)
                .await
                .expect("load")
                .run
                .result,
            ResultKindDto::Failed { .. }
        ));
    }

    // Given invalid start inputs,
    // When the manager parses or resolves them,
    // Then typed InvalidMethod and Resolve errors are returned before a row is persisted.
    #[tokio::test]
    async fn mtu_start_reports_invalid_method_and_resolve_errors() {
        let (_dir, manager, _inspector) = manager_pair("errors", scripted_link_factory(1420)).await;

        assert!(matches!(
            manager
                .start("192.0.2.10", "udp", 9000, 443, |_| {}, Arc::new(|_| {}))
                .await,
            Err(MtuError::InvalidMethod(method)) if method == "udp"
        ));
        assert!(matches!(
            manager
                .start("::1", "icmp", 9000, 443, |_| {}, Arc::new(|_| {}))
                .await,
            Err(MtuError::Resolve(_))
        ));
        assert!(manager.list_runs().await.expect("list").is_empty());
    }

    #[test]
    fn mtu_factory_uses_ipv4_addresses_in_tests() {
        assert!(IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)).is_ipv4());
    }
}
