mod mikrotik_version {
    use std::collections::VecDeque;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use tokio::sync::{mpsc, Notify};
    use verkkokyyla_lib::db::{
        Database, MikrotikSessionVersionStatus, NewMikrotikProfile, NewMikrotikSession,
        now_rfc3339,
    };
    use verkkokyyla_lib::mikrotik::client::{MikrotikClient, MikrotikConnection};
    use verkkokyyla_lib::mikrotik::changelog::*;
    use verkkokyyla_lib::mikrotik::error::MikrotikError;
    use verkkokyyla_lib::mikrotik::manager::MikrotikManager;
    use verkkokyyla_lib::mikrotik::parse::{
        BridgeVlanDto, EthernetMonitorDto, EthernetStatsDto, InterfaceDto, ResourceDto,
        RouterboardDto, SensorDto, UpdateStatusDto, VlanDto,
    };
    use verkkokyyla_lib::mikrotik::secrets::MemoryStore;
    use verkkokyyla_lib::mikrotik::types::*;
    use verkkokyyla_lib::mikrotik::version::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "verkkokyyla-version-{name}-{}-{stamp}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }

        fn db_file(&self) -> PathBuf {
            self.0.join("mikrotik.db")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[derive(Clone)]
    enum Step<T> {
        Val(T),
        Err(MikrotikError),
        Hang,
        Wait(Arc<Notify>, T),
    }

    struct VersionApi {
        updates: Mutex<VecDeque<Step<UpdateStatusDto>>>,
        routerboard: Mutex<VecDeque<Step<RouterboardDto>>>,
    }

    impl VersionApi {
        fn new(updates: Vec<Step<UpdateStatusDto>>, routerboard: Step<RouterboardDto>) -> Arc<Self> {
            Arc::new(Self {
                updates: Mutex::new(updates.into()),
                routerboard: Mutex::new(VecDeque::from([routerboard])),
            })
        }

        fn next<T: Clone>(queue: &Mutex<VecDeque<Step<T>>>) -> Step<T> {
            let mut queue = queue.lock().expect("queue");
            if queue.len() > 1 {
                queue.pop_front().expect("front")
            } else {
                queue.front().cloned().expect("stub")
            }
        }

        async fn resolve<T>(step: Step<T>) -> Result<T, MikrotikError> {
            match step {
                Step::Val(value) => Ok(value),
                Step::Err(err) => Err(err),
                Step::Hang => std::future::pending::<Result<T, MikrotikError>>().await,
                Step::Wait(gate, value) => {
                    gate.notified().await;
                    Ok(value)
                }
            }
        }
    }

    #[async_trait::async_trait]
    impl MikrotikApi for VersionApi {
        async fn get_resource(&self) -> Result<ResourceDto, MikrotikError> {
            Ok(ResourceDto {
                cpu_load: Some(10.0),
                mem_total_bytes: Some(1000),
                mem_used_bytes: Some(500),
                uptime: Some("1h".to_owned()),
                board_name: Some("RB5009".to_owned()),
                version: Some("7.18.2".to_owned()),
                architecture_name: Some("arm64".to_owned()),
            })
        }

        async fn get_interfaces(&self) -> Result<Vec<InterfaceDto>, MikrotikError> {
            Ok(Vec::new())
        }

        async fn get_health(&self) -> Result<Vec<SensorDto>, MikrotikError> {
            Ok(Vec::new())
        }

        async fn get_interface_stats_detail(&self) -> Result<Vec<InterfaceDto>, MikrotikError> {
            Ok(Vec::new())
        }

        async fn get_ethernet_stats(&self) -> Result<Vec<EthernetStatsDto>, MikrotikError> {
            Ok(Vec::new())
        }

        async fn get_ethernet_monitor(&self, _name: &str) -> Result<Vec<EthernetMonitorDto>, MikrotikError> {
            Ok(Vec::new())
        }

        async fn get_vlans(&self) -> Result<Vec<VlanDto>, MikrotikError> {
            Ok(Vec::new())
        }

        async fn get_bridge_vlans(&self) -> Result<Vec<BridgeVlanDto>, MikrotikError> {
            Ok(Vec::new())
        }

        async fn check_for_updates(&self) -> Result<UpdateStatusDto, MikrotikError> {
            Self::resolve(Self::next(&self.updates)).await
        }

        async fn get_update_status(&self) -> Result<UpdateStatusDto, MikrotikError> {
            Self::resolve(Self::next(&self.updates)).await
        }

        async fn get_routerboard(&self) -> Result<RouterboardDto, MikrotikError> {
            Self::resolve(Self::next(&self.routerboard)).await
        }
    }

    fn status(status: &str, latest: Option<&str>) -> Step<UpdateStatusDto> {
        Step::Val(status_dto(status, latest))
    }

    fn status_dto(status: &str, latest: Option<&str>) -> UpdateStatusDto {
        UpdateStatusDto {
            installed_version: Some("7.18.2".to_owned()),
            latest_version: latest.map(str::to_owned),
            channel: Some("stable".to_owned()),
            status: Some(status.to_owned()),
        }
    }

    fn routerboard(value: bool) -> Step<RouterboardDto> {
        routerboard_firmware(value, Some("7.18.2"), Some("7.19"))
    }

    fn routerboard_firmware(
        value: bool,
        current_firmware: Option<&str>,
        upgrade_firmware: Option<&str>,
    ) -> Step<RouterboardDto> {
        Step::Val(RouterboardDto {
            routerboard: Some(value),
            model: Some("RB5009".to_owned()),
            serial_number: None,
            current_firmware: current_firmware.map(str::to_owned),
            upgrade_firmware: upgrade_firmware.map(str::to_owned),
        })
    }

    fn factory_for(api: Arc<VersionApi>) -> MikrotikApiFactory {
        Arc::new(move |_conn: MikrotikConnection| {
            let api = Arc::clone(&api);
            Box::pin(async move { Ok(api as Arc<dyn MikrotikApi>) })
        })
    }

    async fn manager_for(dir: &TempDir, api: Arc<VersionApi>) -> MikrotikManager {
        let db = Database::connect(&dir.db_file()).await.expect("db");
        MikrotikManager::new(db, Arc::new(MemoryStore::new()), factory_for(api))
    }

    async fn profile(manager: &MikrotikManager, name: &str) -> i64 {
        let dto = manager
            .create_profile(&CreateMikrotikProfileRequest {
                name: name.to_owned(),
                host: "127.0.0.1".to_owned(),
                port: 80,
                use_tls: false,
                allow_invalid_certs: true,
                username: "admin".to_owned(),
            })
            .await
            .expect("profile");
        manager
            .set_profile_password(dto.id, "pw")
            .await
            .expect("password");
        dto.id
    }

    async fn status_rx() -> (MikrotikStatusSink, mpsc::UnboundedReceiver<MikrotikStatusEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        (Arc::new(move |event| { let _ = tx.send(event); }), rx)
    }

    async fn drain_statuses(
        statuses: &mut mpsc::UnboundedReceiver<MikrotikStatusEvent>,
    ) -> Vec<MikrotikStatusEvent> {
        let mut out = Vec::new();
        while let Ok(status) = statuses.try_recv() {
            out.push(status);
        }
        out
    }

    #[tokio::test]
    async fn mikrotik_version_manual_check_without_active_session_returns_status_and_persists_nothing() {
        let dir = TempDir::new("manual-no-active");
        let api = VersionApi::new(
            vec![status("System is already up to date", None)],
            routerboard(false),
        );
        let manager = manager_for(&dir, api).await;
        let profile_id = profile(&manager, "edge").await;

        let result = manager.check_updates(profile_id).await.expect("check");

        assert_eq!(result.update_status.latest_version, None);
        assert_eq!(result.update_status.state, UpdateState::Unknown);
        assert_eq!(result.firmware_status.state, FirmwareState::NotApplicable);
        assert!(manager.list_sessions().await.expect("sessions").is_empty());
    }

    #[tokio::test]
    async fn mikrotik_version_manual_check_update_error_with_firmware_na_returns_unknown_without_persisting() {
        let dir = TempDir::new("manual-update-error-firmware-na");
        let api = VersionApi::new(
            vec![Step::Err(MikrotikError::Api {
                status: 500,
                message: "check failed".to_owned(),
            })],
            Step::Err(MikrotikError::Api {
                status: 400,
                message: "no such command or directory (remove)".to_owned(),
            }),
        );
        let manager = manager_for(&dir, api).await;
        let profile_id = profile(&manager, "edge").await;

        let result = manager.check_updates(profile_id).await.expect("check");

        assert_eq!(result.update_status.status, "unknown");
        assert_eq!(result.update_status.latest_version, None);
        assert_eq!(result.update_status.state, UpdateState::Unknown);
        assert_eq!(result.firmware_status.state, FirmwareState::NotApplicable);
        assert!(manager.list_sessions().await.expect("sessions").is_empty());
    }

    #[tokio::test]
    async fn mikrotik_version_check_polls_checking_and_finding_out_until_terminal() {
        let dir = TempDir::new("poll-terminal");
        let api = VersionApi::new(
            vec![
                status("checking for updates...", None),
                status("finding out latest version...", None),
                status("New version is available", Some("7.19")),
            ],
            routerboard(true),
        );
        let manager = manager_for(&dir, api).await;
        let profile_id = profile(&manager, "edge").await;

        let result = manager.check_updates(profile_id).await.expect("check");

        assert_eq!(result.update_status.status, "New version is available");
        assert_eq!(result.update_status.latest_version.as_deref(), Some("7.19"));
        assert_eq!(result.update_status.state, UpdateState::UpdateAvailable);
        assert_eq!(result.firmware_status.state, FirmwareState::Available);
    }

    #[tokio::test]
    async fn mikrotik_version_update_state_classifies_terminal_status_matrix() {
        let cases = vec![
            ("New version is available", Some("7.19"), UpdateState::UpdateAvailable, "update-available"),
            ("System is already up to date", Some("7.18.2"), UpdateState::UpToDate, "up-to-date"),
            ("ERROR: no route to host", Some("7.19"), UpdateState::Unknown, "unknown"),
            ("New version is available", None, UpdateState::Unknown, "unknown"),
        ];
        for (idx, (status_text, latest, expected, wire)) in cases.into_iter().enumerate() {
            let dir = TempDir::new(&format!("update-state-{idx}"));
            let api = VersionApi::new(vec![status(status_text, latest)], routerboard(false));
            let manager = manager_for(&dir, api).await;
            let profile_id = profile(&manager, "edge").await;

            let result = manager.check_updates(profile_id).await.expect("check");
            let value = serde_json::to_value(&result.update_status).expect("update status json");

            assert_eq!(result.update_status.state, expected);
            assert_eq!(value["state"], wire);
            assert_eq!(value["status"], status_text);
        }
    }

    #[tokio::test]
    async fn mikrotik_version_firmware_state_classifies_routerboard_firmware_matrix() {
        let cases = vec![
            (routerboard_firmware(true, Some("7.18.2"), Some("7.18.2")), FirmwareState::UpToDate, "up-to-date"),
            (routerboard_firmware(true, Some("7.18.2"), Some("7.19")), FirmwareState::Available, "available"),
            (routerboard_firmware(true, Some("7.18.2"), None), FirmwareState::Unknown, "unknown"),
            (routerboard_firmware(false, Some("7.18.2"), Some("7.19")), FirmwareState::NotApplicable, "not-applicable"),
        ];
        for (idx, (routerboard_step, expected, wire)) in cases.into_iter().enumerate() {
            let dir = TempDir::new(&format!("firmware-state-{idx}"));
            let api = VersionApi::new(
                vec![status("System is already up to date", Some("7.18.2"))],
                routerboard_step,
            );
            let manager = manager_for(&dir, api).await;
            let profile_id = profile(&manager, "edge").await;

            let result = manager.check_updates(profile_id).await.expect("check");
            let value = serde_json::to_value(&result.firmware_status).expect("firmware json");

            assert_eq!(result.firmware_status.state, expected);
            assert_eq!(value["state"], wire);
        }
    }

    #[tokio::test]
    async fn mikrotik_version_routerboard_not_applicable_variants_are_distinct_from_unknown() {
        let cases = vec![
            (routerboard(false), FirmwareState::NotApplicable),
            (Step::Err(MikrotikError::Api { status: 404, message: String::new() }), FirmwareState::NotApplicable),
            (Step::Err(MikrotikError::Api { status: 400, message: "No Such Command Or Directory (remove)".to_owned() }), FirmwareState::NotApplicable),
            (Step::Err(MikrotikError::Api { status: 500, message: "No Such Command".to_owned() }), FirmwareState::Unknown),
            (Step::Err(MikrotikError::Forbidden), FirmwareState::Unknown),
            (Step::Err(MikrotikError::Timeout("deadline".to_owned())), FirmwareState::Unknown),
            (Step::Err(MikrotikError::Parse("bad json".to_owned())), FirmwareState::Unknown),
        ];
        for (idx, (routerboard_step, expected)) in cases.into_iter().enumerate() {
            let dir = TempDir::new(&format!("routerboard-{idx}"));
            let api = VersionApi::new(
                vec![status("System is already up to date", None)],
                routerboard_step,
            );
            let manager = manager_for(&dir, api).await;
            let profile_id = profile(&manager, "edge").await;

            let result = manager.check_updates(profile_id).await.expect("check");

            assert_eq!(result.firmware_status.state, expected);
        }
    }

    #[tokio::test]
    async fn mikrotik_version_probe_persists_row_and_emits_version_firmware_event() {
        let dir = TempDir::new("probe-event");
        let api = VersionApi::new(
            vec![status("New version is available", Some("7.19"))],
            routerboard(true),
        );
        let manager = manager_for(&dir, api).await;
        let profile_id = profile(&manager, "edge").await;
        let (_event_tx, _event_rx) = mpsc::unbounded_channel::<MikrotikEvent>();
        let (on_status, mut statuses) = status_rx().await;

        let start = manager
            .start(profile_id, |_| {}, on_status)
            .await
            .expect("start");
        let event = statuses.recv().await.expect("started");
        assert!(matches!(event, MikrotikStatusEvent::Started { .. }));
        let delivered = statuses.recv().await.expect("version firmware");

        match delivered {
            MikrotikStatusEvent::VersionFirmware { session_id, update_status, firmware_status } => {
                assert_eq!(session_id, start.session_id);
                assert_eq!(update_status.latest_version.as_deref(), Some("7.19"));
                assert_eq!(firmware_status.state, FirmwareState::Available);
            }
            other => panic!("unexpected status {other:?}"),
        }
        let loaded = manager.load_session(start.session_id).await.expect("load");
        assert!(loaded.session.update_status_json.expect("update json").contains("7.19"));
        assert!(loaded.session.firmware_status_json.expect("firmware json").contains("available"));
        let _ = manager.stop().await;
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_version_stale_probe_does_not_write_restarted_profile() {
        tokio::spawn(async { loop { tokio::task::yield_now().await; } });
        let dir = TempDir::new("stale-probe");
        let gate = Arc::new(Notify::new());
        let api = VersionApi::new(
            vec![
                Step::Wait(Arc::clone(&gate), status_dto("New version is available", Some("7.19"))),
                Step::Hang,
            ],
            routerboard(true),
        );
        let manager = manager_for(&dir, api).await;
        let profile_id = profile(&manager, "edge").await;
        let (on_status_a, mut statuses_a) = status_rx().await;
        let start_a = manager.start(profile_id, |_| {}, on_status_a).await.expect("start a");
        assert!(matches!(statuses_a.recv().await, Some(MikrotikStatusEvent::Started { .. })));

        manager.stop().await.expect("stop a");
        let (on_status_b, mut statuses_b) = status_rx().await;
        let start_b = manager.start(profile_id, |_| {}, on_status_b).await.expect("start b");
        assert_ne!(start_a.session_id, start_b.session_id);

        gate.notify_waiters();
        tokio::task::yield_now().await;
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        let loaded_b = manager.load_session(start_b.session_id).await.expect("load b");
        let loaded_a = manager.load_session(start_a.session_id).await.expect("load a");

        assert_eq!(loaded_b.session.update_status_json, None);
        assert_eq!(loaded_a.session.update_status_json, None);
        assert!(matches!(statuses_b.recv().await, Some(MikrotikStatusEvent::Started { .. })));
        assert!(statuses_b.try_recv().is_err());
        let stale_statuses = drain_statuses(&mut statuses_a).await;
        assert!(stale_statuses
            .iter()
            .all(|event| !matches!(event, MikrotikStatusEvent::VersionFirmware { .. })));
        let _ = manager.stop().await;
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_version_manual_check_stale_same_profile_does_not_write_restarted_session() {
        tokio::spawn(async { loop { tokio::task::yield_now().await; } });
        let dir = TempDir::new("manual-stale-same-profile");
        let gate = Arc::new(Notify::new());
        let api = VersionApi::new(
            vec![
                status("System is already up to date", None),
                Step::Wait(Arc::clone(&gate), status_dto("New version is available", Some("7.21"))),
                Step::Hang,
            ],
            routerboard(true),
        );
        let manager = manager_for(&dir, api).await;
        let profile_id = profile(&manager, "edge").await;
        let (on_status_a, mut statuses_a) = status_rx().await;
        let start_a = manager.start(profile_id, |_| {}, on_status_a).await.expect("start a");
        assert!(matches!(statuses_a.recv().await, Some(MikrotikStatusEvent::Started { .. })));
        tokio::task::yield_now().await;
        let check = {
            let manager = manager.clone();
            tokio::spawn(async move { manager.check_updates(profile_id).await })
        };

        manager.stop().await.expect("stop a");
        let (on_status_b, mut statuses_b) = status_rx().await;
        let start_b = manager.start(profile_id, |_| {}, on_status_b).await.expect("start b");
        gate.notify_waiters();
        tokio::task::yield_now().await;
        let result = check.await.expect("join").expect("check");
        let loaded_b = manager.load_session(start_b.session_id).await.expect("load b");

        assert_eq!(result.update_status.latest_version.as_deref(), Some("7.21"));
        assert_eq!(loaded_b.session.update_status_json, None);
        assert!(matches!(statuses_b.recv().await, Some(MikrotikStatusEvent::Started { .. })));
        assert!(statuses_b.try_recv().is_err());
        assert_ne!(start_a.session_id, start_b.session_id);
        let _ = manager.stop().await;
    }

    #[tokio::test]
    async fn mikrotik_version_manual_check_cross_profile_does_not_persist_active_session() {
        let dir = TempDir::new("manual-cross-profile");
        let api = VersionApi::new(
            vec![Step::Hang, status("New version is available", Some("7.20"))],
            routerboard(true),
        );
        let manager = manager_for(&dir, api).await;
        let profile_a = profile(&manager, "edge-a").await;
        let profile_b = profile(&manager, "edge-b").await;
        let (on_status, mut statuses) = status_rx().await;
        let start_a = manager.start(profile_a, |_| {}, on_status).await.expect("start a");
        assert!(matches!(statuses.recv().await, Some(MikrotikStatusEvent::Started { .. })));

        let result_b = manager.check_updates(profile_b).await.expect("check b");
        let loaded_a = manager.load_session(start_a.session_id).await.expect("load a");

        assert_eq!(loaded_a.session.update_status_json, None);
        assert_eq!(result_b.update_status.latest_version.as_deref(), Some("7.20"));
        let _ = manager.stop().await;
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_version_slow_probe_in_flight_does_not_delay_core_snapshots() {
        tokio::spawn(async { loop { tokio::task::yield_now().await; } });
        let dir = TempDir::new("slow-probe-cadence");
        let api = VersionApi::new(vec![Step::Hang], routerboard(true));
        let manager = manager_for(&dir, api).await;
        let profile_id = profile(&manager, "edge").await;
        let (event_tx, mut events) = mpsc::unbounded_channel();
        let (on_status, mut statuses) = status_rx().await;
        let start = manager
            .start(profile_id, move |event| { let _ = event_tx.send(event); }, on_status)
            .await
            .expect("start");
        assert!(matches!(statuses.recv().await, Some(MikrotikStatusEvent::Started { .. })));

        let first = events.recv().await.expect("first snapshot");
        tokio::time::advance(std::time::Duration::from_secs(5)).await;
        tokio::task::yield_now().await;
        let second = events.recv().await.expect("second snapshot");

        assert!(matches!(first, MikrotikEvent::Snapshot(payload) if payload.session_id == start.session_id));
        assert!(matches!(second, MikrotikEvent::Snapshot(payload) if payload.session_id == start.session_id));
        assert!(statuses.try_recv().is_err());
        let _ = manager.stop().await;
    }

    #[tokio::test]
    async fn mikrotik_version_routerboard_no_such_command_matrix_is_not_applicable() {
        let cases = vec![
            MikrotikError::Api { status: 400, message: "no such command or directory (remove)".to_owned() },
            MikrotikError::Api { status: 406, message: "no such command or directory".to_owned() },
            MikrotikError::Api { status: 400, message: r#"{"message":"no such command or directory"}"#.to_owned() },
            MikrotikError::Api { status: 406, message: "No Such Command Or Directory".to_owned() },
        ];
        for (idx, err) in cases.into_iter().enumerate() {
            let dir = TempDir::new(&format!("no-such-matrix-{idx}"));
            let api = VersionApi::new(
                vec![status("System is already up to date", None)],
                Step::Err(err),
            );
            let manager = manager_for(&dir, api).await;
            let profile_id = profile(&manager, "edge").await;

            let result = manager.check_updates(profile_id).await.expect("check");

            assert_eq!(result.firmware_status.state, FirmwareState::NotApplicable);
        }
    }

    #[tokio::test]
    async fn mikrotik_version_historical_load_exposes_update_matrix_and_firmware_na() {
        let dir = TempDir::new("historical-load");
        let db = Database::connect(&dir.db_file()).await.expect("db");
        let cases = vec![
            ("unknown", None, "unknown"),
            ("System is already up to date", None, "up-to-date"),
            ("New version is available", Some("7.20"), "update-available"),
        ];
        let mut ids = Vec::new();
        for (idx, (status_text, latest, label)) in cases.into_iter().enumerate() {
            let profile = db
                .create_mikrotik_profile(&NewMikrotikProfile {
                    name: format!("historical-{idx}"),
                    host: "127.0.0.1".to_owned(),
                    port: 80,
                    use_tls: false,
                    allow_invalid_certs: true,
                    username: "admin".to_owned(),
                    created_at: now_rfc3339(),
                })
                .await
                .expect("profile");
            let id = db
                .create_mikrotik_session(&NewMikrotikSession {
                    profile_id: profile.id,
                    started_at: now_rfc3339(),
                    status: "cancelled".to_owned(),
                })
                .await
                .expect("session");
            let update = UpdateStatusResultDto {
                installed_version: Some("7.18.2".to_owned()),
                latest_version: latest.map(str::to_owned),
                channel: Some(label.to_owned()),
                status: status_text.to_owned(),
                state: match label {
                    "update-available" => UpdateState::UpdateAvailable,
                    "up-to-date" => UpdateState::UpToDate,
                    "unknown" => UpdateState::Unknown,
                    _ => unreachable!(),
                },
            };
            let firmware = FirmwareStatusDto {
                state: FirmwareState::NotApplicable,
                current_firmware: None,
                upgrade_firmware: None,
                model: None,
            };
            db.set_mikrotik_session_version_status(
                id,
                &MikrotikSessionVersionStatus {
                    board_name: Some("CHR".to_owned()),
                    routeros_version: Some("7.18.2".to_owned()),
                    architecture_name: Some("x86_64".to_owned()),
                    update_status_json: Some(serde_json::to_string(&update).expect("update json")),
                    firmware_status_json: Some(serde_json::to_string(&firmware).expect("firmware json")),
                },
            )
            .await
            .expect("version status");
            ids.push((id, status_text.to_owned(), latest.map(str::to_owned), label.to_owned()));
        }
        let manager = manager_for(
            &dir,
            VersionApi::new(vec![status("System is already up to date", None)], routerboard(false)),
        )
        .await;

        for (id, status_text, latest, label) in ids {
            let loaded = manager.load_session(id).await.expect("load");
            let update: serde_json::Value = serde_json::from_str(
                loaded.session.update_status_json.as_deref().expect("update json"),
            )
            .expect("update value");
            let firmware: serde_json::Value = serde_json::from_str(
                loaded.session.firmware_status_json.as_deref().expect("firmware json"),
            )
            .expect("firmware value");

            assert_eq!(update["status"], status_text);
            assert_eq!(update["latestVersion"].as_str().map(str::to_owned), latest);
            assert_eq!(update["channel"], label);
            assert_eq!(firmware["state"], "not-applicable");
        }
    }

    struct FakeChangelogFetcher {
        responses: Mutex<VecDeque<Result<String, ChangelogError>>>,
        calls: Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl ChangelogFetcher for FakeChangelogFetcher {
        async fn fetch_changelog(&self, version: &str) -> Result<String, ChangelogError> {
            self.calls.lock().expect("calls").push(version.to_owned());
            self.responses
                .lock()
                .expect("responses")
                .pop_front()
                .unwrap_or_else(|| Ok("cached fixture".to_owned()))
        }
    }

    #[tokio::test]
    async fn mikrotik_version_changelog_fetcher_returns_200_fixture() {
        let fetcher = Arc::new(FakeChangelogFetcher {
            responses: Mutex::new(VecDeque::from([Ok("What's new in 7.19".to_owned())])),
            calls: Mutex::new(Vec::new()),
        });
        let service = ChangelogService::new(fetcher);

        let dto = service.fetch("7.19").await.expect("changelog");

        assert_eq!(dto.version, "7.19");
        assert!(dto.changelog.contains("7.19"));
    }

    #[tokio::test]
    async fn mikrotik_version_changelog_fetcher_maps_404_and_timeout_as_typed_errors() {
        let fetcher = Arc::new(FakeChangelogFetcher {
            responses: Mutex::new(VecDeque::from([
                Err(ChangelogError::ChangelogNotFound("7.1".to_owned())),
                Err(ChangelogError::Timeout("deadline".to_owned())),
            ])),
            calls: Mutex::new(Vec::new()),
        });
        let service = ChangelogService::new(fetcher);

        let missing = service.fetch("7.1").await.expect_err("404");
        let timeout = service.fetch("7.2").await.expect_err("timeout");

        assert!(matches!(missing, ChangelogError::ChangelogNotFound(_)));
        assert!(matches!(timeout, ChangelogError::Timeout(_)));
    }

    #[tokio::test]
    async fn mikrotik_version_changelog_cached_fetcher_hits_same_version_once() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("GET"))
            .and(wiremock::matchers::path("/7.19/CHANGELOG"))
            .respond_with(wiremock::ResponseTemplate::new(200).set_body_string("cached fixture"))
            .expect(1)
            .mount(&server)
            .await;
        let fetcher = CachedChangelogFetcher::new_for_base_url(server.uri()).expect("fetcher");

        let first = fetcher.fetch_changelog("7.19").await.expect("first");
        let second = fetcher.fetch_changelog("7.19").await.expect("second");

        assert_eq!(first, "cached fixture");
        assert_eq!(second, "cached fixture");
    }

    #[tokio::test]
    async fn mikrotik_version_slow_check_post_over_default_timeout_succeeds() {
        let server = wiremock::MockServer::start().await;
        wiremock::Mock::given(wiremock::matchers::method("POST"))
            .and(wiremock::matchers::path("/rest/system/package/update/check-for-updates"))
            .respond_with(
                wiremock::ResponseTemplate::new(200)
                    .set_delay(std::time::Duration::from_secs(11))
                    .set_body_json(serde_json::json!({
                        "installed-version": "7.18.2",
                        "latest-version": "7.19",
                        "channel": "stable",
                        "status": "New version is available"
                    })),
            )
            .expect(1)
            .mount(&server)
            .await;
        let url = url::Url::parse(&server.uri()).expect("server uri");
        let client = MikrotikClient::new(&MikrotikConnection {
            host: url.host_str().expect("host").to_owned(),
            port: url.port().expect("port"),
            use_tls: false,
            allow_invalid_certs: true,
            username: "admin".to_owned(),
            password: "pw".to_owned(),
        })
        .expect("client");

        let dto = client.check_for_updates().await.expect("slow command");

        assert_eq!(dto.latest_version.as_deref(), Some("7.19"));
    }
}
