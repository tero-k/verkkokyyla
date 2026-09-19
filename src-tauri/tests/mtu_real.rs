//! Real-adapter MTU discovery evidence: runs the full manager + search
//! controller against live network targets. Ignored by default; run with:
//! `cargo test --manifest-path src-tauri/Cargo.toml --test mtu_real -- --ignored`

mod mtu_real {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use futures_util::future::BoxFuture;
    use verkkokyyla_lib::db::Database;
    use verkkokyyla_lib::engine::EngineError;
    use verkkokyyla_lib::mtu::engine::MtuProbeEngine;
    #[cfg(unix)]
    use verkkokyyla_lib::mtu::engine::OsPingDfEngine;
    use verkkokyyla_lib::mtu::manager::{MtuFactory, MtuManager};
    use verkkokyyla_lib::mtu::{MtuMethod, MtuStatusEvent, ResultKindDto};

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "verkkokyyla-mtu-real-{name}-{}-{stamp}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn real_factory() -> MtuFactory {
        Arc::new(
            move |method: MtuMethod, address: std::net::IpAddr, port: u16| {
                Box::pin(async move {
                    let engine = match method {
                        #[cfg(windows)]
                        MtuMethod::Icmp => MtuProbeEngine::WinIcmpDf(address),
                        #[cfg(unix)]
                        MtuMethod::Icmp => MtuProbeEngine::OsPingDf(OsPingDfEngine::new(address)?),
                        MtuMethod::Tcp => MtuProbeEngine::TcpProbe(
                            verkkokyyla_lib::mtu::tcp_probe::TcpMtuProber::connect(
                                address,
                                port,
                                Duration::from_millis(2000),
                            )?,
                        ),
                    };
                    Ok(engine)
                }) as BoxFuture<'static, Result<MtuProbeEngine, EngineError>>
            },
        )
    }

    async fn run_and_report(method: &str, port: u16) -> ResultKindDto {
        let dir = TempDir::new(method);
        let db = Database::connect(&dir.0.join("mtu.db")).await.expect("db");
        let manager = MtuManager::new(db, real_factory());
        let (on_status, mut statuses) = {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            (
                Arc::new(move |event: MtuStatusEvent| {
                    let _ = tx.send(event);
                }) as Arc<dyn Fn(MtuStatusEvent) + Send + Sync>,
                rx,
            )
        };

        manager
            .start("8.8.8.8", method, 9000, port, move |_| {}, on_status)
            .await
            .expect("start");

        let completed = tokio::time::timeout(Duration::from_secs(120), statuses.recv())
            .await
            .expect("run timed out")
            .expect("status event");
        match completed {
            MtuStatusEvent::Completed { result, .. } => result,
            other => panic!("expected completed run, got {other:?}"),
        }
    }

    fn assert_internet_mtu(result: ResultKindDto) {
        let mtu = match result {
            ResultKindDto::Exact { mtu } => mtu,
            ResultKindDto::LowerBound { mtu, .. } => mtu,
            other => panic!("expected exact/lower-bound result, got {other:?}"),
        };
        assert!(
            (1400..=1500).contains(&mtu),
            "unexpected internet path MTU {mtu}"
        );
    }

    // Given the live internet path to a public resolver (1500-byte first hop),
    // When ICMP DF probing runs end-to-end through the manager,
    // Then the discovered MTU is reported and sits in the Ethernet range.
    #[tokio::test]
    #[ignore = "real adapter evidence; run with --ignored"]
    async fn icmp_mtu_discovery_to_public_resolver() {
        let result = run_and_report("icmp", 0).await;
        eprintln!("ICMP result: {result:?}");
        assert_internet_mtu(result);
    }

    // Given the live internet path to a public resolver on port 443,
    // When TCP PLPMTUD probing runs end-to-end through the manager on Linux,
    // Then the discovered estimate is reported and sits in the Ethernet range.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore = "real adapter evidence; run with --ignored"]
    async fn tcp_mtu_discovery_to_public_resolver() {
        let result = run_and_report("tcp", 443).await;
        eprintln!("TCP result: {result:?}");
        assert_internet_mtu(result);
    }

    // Given Windows TCP stacks segment writes regardless of DF,
    // When a TCP run is requested there,
    // Then the manager refuses with the honest Unavailable engine error.
    #[cfg(windows)]
    #[tokio::test]
    #[ignore = "real adapter evidence; run with --ignored"]
    async fn tcp_method_reports_unavailable_on_windows() {
        let dir = TempDir::new("tcp-unavailable");
        let db = Database::connect(&dir.0.join("mtu.db")).await.expect("db");
        let manager = MtuManager::new(db, real_factory());

        let err = manager
            .start("8.8.8.8", "tcp", 9000, 443, |_| {}, Arc::new(|_| {}))
            .await
            .expect_err("windows must refuse TCP probing");

        assert!(
            matches!(
                err,
                verkkokyyla_lib::mtu::MtuError::Engine(EngineError::Unavailable(_))
            ),
            "expected unavailable engine error, got {err:?}"
        );
    }
}
