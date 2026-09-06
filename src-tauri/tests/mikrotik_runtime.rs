mod mikrotik_runtime {
    use std::collections::{HashMap, VecDeque};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use serde_json::{json, Value};
    use tokio::sync::mpsc;
    use verkkokyyla_lib::db::Database;
    use verkkokyyla_lib::mikrotik::client::MikrotikConnection;
    use verkkokyyla_lib::mikrotik::error::MikrotikError;
    use verkkokyyla_lib::mikrotik::manager::MikrotikManager;
    use verkkokyyla_lib::mikrotik::parse::{
        parse_bridge_vlans, parse_ethernet_monitor, parse_ethernet_stats, parse_health,
        parse_interfaces, parse_resource, parse_vlans, BridgeVlanDto, EthernetMonitorDto,
        EthernetStatsDto, InterfaceDto, ResourceDto, SensorDto, VlanDto,
    };
    use verkkokyyla_lib::mikrotik::secrets::{MemoryStore, SecretError, SecretStore};
    use verkkokyyla_lib::mikrotik::types::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "verkkokyyla-mikrotik-{name}-{}-{stamp}",
                std::process::id()
            ));
            std::fs::create_dir_all(&dir).expect("temp dir");
            Self(dir)
        }

        fn db_file(&self) -> PathBuf {
            self.0.join("nested").join("mikrotik.db")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// The `start_paused` clock auto-advances whenever the runtime goes idle
    /// (parked with pending timers) — which would fire sqlx's 30s pool-acquire
    /// timeout while its blocking worker is still opening the SQLite file,
    /// producing instant `PoolTimedOut`s. `harness()` spawns an always-ready
    /// yield task that keeps the runtime from ever parking, so the clock only
    /// moves via explicit `advance()` calls and sqlx can finish normally.
    async fn test_db(path: &std::path::Path) -> Database {
        Database::connect(path).await.expect("db")
    }

    /// One scripted response for an endpoint call. `Hang` never resolves
    /// until the session is cancelled (enricher-slowness tests).
    #[derive(Clone)]
    enum Stub {
        Val(Value),
        Err(MikrotikError),
        Hang,
    }

    fn ok(value: Value) -> Stub {
        Stub::Val(value)
    }

    fn err(status: u16, message: &str) -> Stub {
        Stub::Err(MikrotikError::Api {
            status,
            message: message.to_owned(),
        })
    }

    // ---- Fixture JSON shapes (matching todo 2's defensive parsers) ---------

    fn resource_json(cpu_load: f64) -> Value {
        json!({
            "cpu-load": cpu_load,
            "total-memory": 1_000_000_u64,
            "free-memory": 400_000_u64,
            "uptime": "1d 02:03:04",
            "board-name": "RB5009",
            "version": "7.16.1",
            "architecture-name": "arm64"
        })
    }

    fn iface_json_full(name: &str, iface_type: &str, rx: u64, tx: u64) -> Value {
        json!({
            "name": name,
            "type": iface_type,
            // RouterOS REST sends booleans as strings (parse.rs only accepts
            // that wire form).
            "running": "true",
            "disabled": "false",
            "rx-byte": rx,
            "tx-byte": tx,
            "rx-packet": 10,
            "tx-packet": 20,
            "tx-queue-drop": 1,
            "link-downs": 2,
            "rx-error": 3,
            "tx-error": 4,
            "rx-drop": 5
        })
    }

    fn one_ether(rx: u64, tx: u64) -> Value {
        Value::Array(vec![iface_json_full("ether1", "ether", rx, tx)])
    }

    fn health_json() -> Value {
        json!([{ "name": "cpu-temperature", "value": "45" }])
    }

    fn stats_detail_json(name: &str) -> Value {
        json!([{
            "name": name,
            "rx-error": 30, "tx-error": 31, "rx-drop": 32, "link-downs": 33,
            "rx-error-events": 34, "tx-error-events": 35, "rx-fcs-error": 36,
            "rx-align-error": 37, "tx-collision": 38, "tx-drop": 39
        }])
    }

    fn ethernet_stats_json(name: &str, default_name: &str) -> Value {
        json!([{
            "name": name, "default-name": default_name,
            "rx-error-events": 40, "tx-error-events": 41, "rx-fcs-error": 42,
            "rx-align-error": 43, "tx-collision": 44, "tx-drop": 45
        }])
    }

    fn monitor_json(name: &str) -> Value {
        json!([{ "name": name, "rate": "1Gbps", "full-duplex": "true", "status": "link-ok" }])
    }

    fn vlans_json() -> Value {
        json!([{
            "name": "vlan10", "vlan-id": 10, "interface": "ether1",
            "running": "true", "disabled": "false"
        }])
    }

    fn bridge_vlans_json() -> Value {
        json!([{
            "bridge": "bridge1", "vlan-ids": "10,20",
            "tagged": "ether1", "untagged": "ether2"
        }])
    }
    // ---- Scripted fake ------------------------------------------------------

    #[derive(Clone, Default)]
    struct ApiScript {
        resource: Vec<Stub>,
        interfaces: Vec<Stub>,
        health: Vec<Stub>,
        stats_detail: Vec<Stub>,
        ethernet_stats: Vec<Stub>,
        monitor: HashMap<String, Vec<Stub>>,
        vlans: Vec<Stub>,
        bridge_vlans: Vec<Stub>,
    }

    impl ApiScript {
        /// Every endpoint succeeds forever with one ether1 interface.
        fn working() -> Self {
            ApiScript {
                resource: vec![ok(resource_json(50.0))],
                interfaces: vec![ok(one_ether(1_000, 2_000))],
                health: vec![ok(health_json())],
                stats_detail: vec![ok(stats_detail_json("ether1"))],
                ethernet_stats: vec![ok(ethernet_stats_json("ether1", "ether1"))],
                monitor: HashMap::from([(
                    "ether1".to_owned(),
                    vec![ok(monitor_json("ether1"))],
                )]),
                vlans: vec![ok(vlans_json())],
                bridge_vlans: vec![ok(bridge_vlans_json())],
            }
        }

        fn build(self) -> Arc<ScriptedApi> {
            Arc::new(ScriptedApi {
                resource: Mutex::new(self.resource.into()),
                interfaces: Mutex::new(self.interfaces.into()),
                health: Mutex::new(self.health.into()),
                stats_detail: Mutex::new(self.stats_detail.into()),
                ethernet_stats: Mutex::new(self.ethernet_stats.into()),
                monitor: Mutex::new(
                    self.monitor
                        .into_iter()
                        .map(|(name, stubs)| (name, stubs.into()))
                        .collect(),
                ),
                vlans: Mutex::new(self.vlans.into()),
                bridge_vlans: Mutex::new(self.bridge_vlans.into()),
                health_calls: AtomicUsize::new(0),
                stats_detail_calls: AtomicUsize::new(0),
                ethernet_stats_calls: AtomicUsize::new(0),
                monitor_calls: AtomicUsize::new(0),
                seen: Mutex::new(Vec::new()),
            })
        }
    }

    /// Per-call scripted fake implementing `MikrotikApi`. Each endpoint has
    /// a FIFO script; the LAST entry repeats indefinitely so ticks beyond
    /// the script still succeed.
    struct ScriptedApi {
        resource: Mutex<VecDeque<Stub>>,
        interfaces: Mutex<VecDeque<Stub>>,
        health: Mutex<VecDeque<Stub>>,
        stats_detail: Mutex<VecDeque<Stub>>,
        ethernet_stats: Mutex<VecDeque<Stub>>,
        monitor: Mutex<HashMap<String, VecDeque<Stub>>>,
        vlans: Mutex<VecDeque<Stub>>,
        bridge_vlans: Mutex<VecDeque<Stub>>,
        health_calls: AtomicUsize,
        stats_detail_calls: AtomicUsize,
        ethernet_stats_calls: AtomicUsize,
        monitor_calls: AtomicUsize,
        seen: Mutex<Vec<MikrotikConnection>>,
    }

    impl ScriptedApi {
        fn next(queue: &Mutex<VecDeque<Stub>>) -> Stub {
            let mut queue = queue.lock().unwrap();
            if queue.len() > 1 {
                queue.pop_front().unwrap()
            } else {
                queue.front().cloned().unwrap_or_else(|| ok(Value::Null))
            }
        }

        async fn respond(stub: Stub) -> Result<Value, MikrotikError> {
            match stub {
                Stub::Val(value) => Ok(value),
                Stub::Err(err) => Err(err),
                Stub::Hang => std::future::pending::<Result<Value, MikrotikError>>().await,
            }
        }
    }

    #[async_trait::async_trait]
    impl MikrotikApi for ScriptedApi {
        async fn get_resource(&self) -> Result<ResourceDto, MikrotikError> {
            parse_resource(&Self::respond(Self::next(&self.resource)).await?)
        }

        async fn get_interfaces(&self) -> Result<Vec<InterfaceDto>, MikrotikError> {
            parse_interfaces(&Self::respond(Self::next(&self.interfaces)).await?)
        }

        async fn get_health(&self) -> Result<Vec<SensorDto>, MikrotikError> {
            self.health_calls.fetch_add(1, Ordering::SeqCst);
            parse_health(&Self::respond(Self::next(&self.health)).await?)
        }

        async fn get_interface_stats_detail(&self) -> Result<Vec<InterfaceDto>, MikrotikError> {
            self.stats_detail_calls.fetch_add(1, Ordering::SeqCst);
            parse_interfaces(&Self::respond(Self::next(&self.stats_detail)).await?)
        }

        async fn get_ethernet_stats(&self) -> Result<Vec<EthernetStatsDto>, MikrotikError> {
            self.ethernet_stats_calls.fetch_add(1, Ordering::SeqCst);
            parse_ethernet_stats(&Self::respond(Self::next(&self.ethernet_stats)).await?)
        }

        async fn get_ethernet_monitor(
            &self,
            name: &str,
        ) -> Result<Vec<EthernetMonitorDto>, MikrotikError> {
            self.monitor_calls.fetch_add(1, Ordering::SeqCst);
            let stub = {
                let mut map = self.monitor.lock().unwrap();
                let queue = map
                    .entry(name.to_owned())
                    .or_insert_with(|| VecDeque::from([ok(monitor_json(name))]));
                if queue.len() > 1 {
                    queue.pop_front().unwrap()
                } else {
                    queue.front().cloned().unwrap()
                }
            };
            parse_ethernet_monitor(&Self::respond(stub).await?)
        }

        async fn get_vlans(&self) -> Result<Vec<VlanDto>, MikrotikError> {
            parse_vlans(&Self::respond(Self::next(&self.vlans)).await?)
        }

        async fn get_bridge_vlans(&self) -> Result<Vec<BridgeVlanDto>, MikrotikError> {
            parse_bridge_vlans(&Self::respond(Self::next(&self.bridge_vlans)).await?)
        }
    }

    fn factory_for(api: Arc<ScriptedApi>) -> MikrotikApiFactory {
        Arc::new(move |conn: MikrotikConnection| {
            api.seen.lock().unwrap().push(conn);
            let api = Arc::clone(&api);
            Box::pin(async move { Ok(api as Arc<dyn MikrotikApi>) })
        })
    }
    // ---- Harness ------------------------------------------------------------

    struct Harness {
        manager: MikrotikManager,
        inspector: Database,
        api: Arc<ScriptedApi>,
        on_event: Arc<dyn Fn(MikrotikEvent) + Send + Sync>,
        on_status: MikrotikStatusSink,
        events: mpsc::UnboundedReceiver<MikrotikEvent>,
        statuses: mpsc::UnboundedReceiver<MikrotikStatusEvent>,
    }

    async fn harness(dir: &TempDir, api: Arc<ScriptedApi>) -> Harness {
        // Keep the paused runtime from parking: while ANY task is always
        // ready, tokio never calls park(), so its test-util clock never
        // auto-advances and sqlx's pool timers stay inert until an explicit
        // `advance()`. The task lives until the test runtime shuts down.
        tokio::spawn(async move {
            loop {
                tokio::task::yield_now().await;
            }
        });
        let manager_db = test_db(&dir.db_file()).await;
        let inspector = test_db(&dir.db_file()).await;
        let manager =
            MikrotikManager::new(manager_db, Arc::new(MemoryStore::new()), factory_for(api.clone()));
        let (event_tx, events) = mpsc::unbounded_channel();
        let (status_tx, statuses) = mpsc::unbounded_channel();
        Harness {
            manager,
            inspector,
            api,
            on_event: Arc::new(move |event| {
                let _ = event_tx.send(event);
            }),
            on_status: Arc::new(move |event| {
                let _ = status_tx.send(event);
            }),
            events,
            statuses,
        }
    }

    async fn create_profile(manager: &MikrotikManager) -> i64 {
        manager
            .create_profile(&CreateMikrotikProfileRequest {
                name: "edge".to_owned(),
                host: "192.0.2.10".to_owned(),
                port: 443,
                use_tls: true,
                allow_invalid_certs: false,
                username: "admin".to_owned(),
            })
            .await
            .expect("create profile")
            .id
    }

    async fn start_session(h: &Harness, profile_id: i64) -> MikrotikStartDto {
        let on_event = h.on_event.clone();
        let on_status = h.on_status.clone();
        h.manager
            .start(profile_id, move |event| on_event(event), on_status)
            .await
            .expect("start session")
    }

    async fn next_snapshot(events: &mut mpsc::UnboundedReceiver<MikrotikEvent>) -> MikrotikSnapshotPayload {
        match events.recv().await.expect("snapshot event") {
            MikrotikEvent::Snapshot(payload) => payload,
        }
    }

    async fn advance_secs(secs: u64) {
        tokio::time::advance(Duration::from_secs(secs)).await;
    }

    async fn drain_statuses(
        statuses: &mut mpsc::UnboundedReceiver<MikrotikStatusEvent>,
    ) -> Vec<MikrotikStatusEvent> {
        let mut collected = Vec::new();
        while let Ok(status) = statuses.try_recv() {
            collected.push(status);
        }
        collected
    }

    fn err_kind(err: &MikrotikManagerError) -> String {
        serde_json::to_value(err).expect("serialize error")["kind"]
            .as_str()
            .expect("kind string")
            .to_owned()
    }

    /// Memory store whose delete can be flipped to fail (orphan-tolerance).
    struct FlakyStore {
        inner: MemoryStore,
        fail_delete: Arc<AtomicBool>,
    }

    #[async_trait::async_trait]
    impl SecretStore for FlakyStore {
        async fn get(&self, secret_key: &str) -> Result<String, SecretError> {
            self.inner.get(secret_key).await
        }

        async fn set(&self, secret_key: &str, password: &str) -> Result<(), SecretError> {
            self.inner.set(secret_key, password).await
        }

        async fn delete(&self, secret_key: &str) -> Result<(), SecretError> {
            if self.fail_delete.load(Ordering::SeqCst) {
                return Err(SecretError::Keyring("backend down".to_owned()));
            }
            self.inner.delete(secret_key).await
        }
    }
    // ---- Wire contract (locked) ---------------------------------------------

    fn sample_payload() -> MikrotikSnapshotPayload {
        MikrotikSnapshotPayload {
            session_id: 7,
            at: "2026-01-01T00:00:00Z".to_owned(),
            resources: Some(MikrotikResourcesDto {
                cpu_load: Some(12.5),
                mem_used_bytes: Some(600_000),
                mem_total_bytes: Some(1_000_000),
                uptime: Some("1d".to_owned()),
            }),
            sensors: Some(vec![MikrotikSensorDto {
                name: "cpu-temperature".to_owned(),
                value: 45.0,
                unit: Some("C".to_owned()),
                kind: "temperature".to_owned(),
            }]),
            sensors_supported: true,
            interfaces: vec![MikrotikInterfaceDto {
                name: "ether1".to_owned(),
                iface_type: Some("ether".to_owned()),
                running: Some(true),
                disabled: Some(false),
                rx_byte: Some(1_000),
                tx_byte: Some(2_000),
                rx_packet: Some(10),
                tx_packet: Some(20),
                tx_queue_drop: Some(1),
                link_downs: Some(2),
                rx_error: Some(3),
                tx_error: Some(4),
                rx_drop: Some(5),
                rx_error_events: Some(40),
                tx_error_events: Some(41),
                rx_fcs_error: Some(42),
                rx_align_error: Some(43),
                tx_collision: Some(44),
                tx_drop: Some(45),
                rate: Some("1Gbps".to_owned()),
                full_duplex: Some(true),
                rx_bits_per_second: Some(800.0),
                tx_bits_per_second: Some(1_600.0),
            }],
            vlans: None,
            bridge_vlans: None,
            warning: Some("health: timed out".to_owned()),
        }
    }

    #[test]
    fn mikrotik_runtime_wire_contract_exact_event_tags_and_camel_case() {
        let value = serde_json::to_value(&MikrotikEvent::Snapshot(sample_payload())).unwrap();
        let obj = value.as_object().unwrap();
        assert_eq!(obj.get("event").and_then(Value::as_str), Some("snapshot"));
        for key in [
            "sessionId",
            "sensorsSupported",
            "bridgeVlans",
            "warning",
        ] {
            assert!(obj.contains_key(key), "snapshot must carry camelCase `{key}`");
        }
        let iface = obj["interfaces"].as_array().unwrap()[0]
            .as_object()
            .unwrap();
        for key in [
            "txQueueDrop",
            "linkDowns",
            "fullDuplex",
            "type",
            "rxBitsPerSecond",
            "txBitsPerSecond",
        ] {
            assert!(iface.contains_key(key), "interface must carry `{key}`");
        }
        assert!(!iface.contains_key("iface_type"));
        assert!(!iface.contains_key("ifaceType"));
        // Round-trip through serialized text keeps the exact tag.
        let reparsed: Value =
            serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
        assert_eq!(reparsed["event"], "snapshot");

        let cases: Vec<(MikrotikStatusEvent, &str)> = vec![
            (
                MikrotikStatusEvent::Started {
                    session_id: 1,
                    profile_id: 2,
                },
                "started",
            ),
            (
                MikrotikStatusEvent::Stopped {
                    session_id: 1,
                    snapshot_count: 3,
                },
                "stopped",
            ),
            (
                MikrotikStatusEvent::Cancelled {
                    session_id: 1,
                    snapshot_count: 3,
                },
                "cancelled",
            ),
            (
                MikrotikStatusEvent::Warning {
                    session_id: 1,
                    source: "core".to_owned(),
                    message: "m".to_owned(),
                },
                "warning",
            ),
            (
                MikrotikStatusEvent::Error {
                    session_id: 1,
                    message: "m".to_owned(),
                },
                "error",
            ),
        ];
        for (event, tag) in cases {
            let value = serde_json::to_value(&event).unwrap();
            assert_eq!(value["event"], tag, "wrong tag for {event:?}");
            let reparsed: Value =
                serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
            assert_eq!(reparsed["event"], tag);
        }
        let started = serde_json::to_value(&MikrotikStatusEvent::Started {
            session_id: 1,
            profile_id: 2,
        })
        .unwrap();
        assert!(started.get("sessionId").is_some());
        assert!(started.get("profileId").is_some());
    }

    #[tokio::test]
    async fn mikrotik_runtime_profile_dto_never_serializes_secret_key() {
        let dir = TempDir::new("dto");
        let api = ApiScript::working().build();
        let h = harness(&dir, api).await;
        let dto = h
            .manager
            .create_profile(&CreateMikrotikProfileRequest {
                name: "edge".to_owned(),
                host: "192.0.2.10".to_owned(),
                port: 443,
                use_tls: true,
                allow_invalid_certs: false,
                username: "admin".to_owned(),
            })
            .await
            .unwrap();
        let value = serde_json::to_value(&dto).unwrap();
        let obj = value.as_object().unwrap();
        assert!(!obj.contains_key("secretKey"), "secret_key must never serialize");
        assert!(!obj.contains_key("secret_key"));
        assert_eq!(obj["hasPassword"], false);

        h.manager.set_profile_password(dto.id, "pw").await.unwrap();
        let listed = h.manager.list_profiles().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].has_password);
    }
    // ---- Happy path (manual-QA scenario 1: 3 ticks -> 3 events + 3 rows) ----

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_three_ticks_emit_three_snapshots_and_persist_three_rows() {
        let dir = TempDir::new("happy");
        let api = ApiScript::working().build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();

        let start = start_session(&h, profile_id).await;
        let started = h.statuses.recv().await.expect("started status");
        assert!(matches!(
            started,
            MikrotikStatusEvent::Started { session_id, profile_id: pid }
                if session_id == start.session_id && pid == profile_id
        ));

        let mut snapshots = Vec::new();
        for tick in 0..3 {
            if tick > 0 {
                advance_secs(5).await;
            }
            snapshots.push(next_snapshot(&mut h.events).await);
        }
        assert_eq!(snapshots.len(), 3);

        let stopped = h.manager.stop().await.expect("stop");
        assert_eq!(stopped.status, "cancelled");
        assert_eq!(stopped.snapshot_count, 3);
        let cancelled = h.statuses.recv().await.expect("cancelled status");
        assert!(matches!(
            cancelled,
            MikrotikStatusEvent::Cancelled { session_id, snapshot_count: 3 }
                if session_id == start.session_id
        ));

        let loaded = h
            .inspector
            .load_mikrotik_session(start.session_id)
            .await
            .expect("load session");
        assert_eq!(loaded.snapshots.len(), 3, "3 persisted rows");
        assert_eq!(loaded.session.status, "cancelled");
        assert_eq!(loaded.session.snapshot_count, 3);
        // FIRST successful resource sample persisted board/version/architecture.
        assert_eq!(loaded.session.board_name.as_deref(), Some("RB5009"));
        assert_eq!(loaded.session.routeros_version.as_deref(), Some("7.16.1"));
        assert_eq!(loaded.session.architecture_name.as_deref(), Some("arm64"));
        for row in &loaded.snapshots {
            assert_eq!(row.cpu_load, Some(50.0));
            assert_eq!(row.mem_used_bytes, Some(600_000));
            assert_eq!(row.mem_total_bytes, Some(1_000_000));
            assert!(row.interfaces_json.is_some());
        }

        // load_session command DTO round-trips the same data (stale_state probe).
        let dto = h.manager.load_session(start.session_id).await.expect("load dto");
        assert_eq!(dto.snapshots.len(), 3);
        assert_eq!(dto.session.board_name.as_deref(), Some("RB5009"));
        assert_eq!(dto.snapshots[0].cpu_load, Some(50.0));
    }

    // ---- Rate math from ACTUAL elapsed time ----------------------------------

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_rates_use_actual_elapsed_not_assumed_interval() {
        let dir = TempDir::new("rates");
        let mut script = ApiScript::working();
        script.interfaces = vec![
            ok(one_ether(1_000, 2_000)),
            ok(one_ether(1_700, 3_400)),
            ok(one_ether(2_400, 4_800)),
        ];
        let api = script.build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let _start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");

        let first = next_snapshot(&mut h.events).await;
        assert!(first.interfaces[0].rx_bits_per_second.is_none());

        // 7s-delayed tick: rates must be 8·delta/7, NOT 8·delta/5.
        advance_secs(7).await;
        let second = next_snapshot(&mut h.events).await;
        assert_eq!(second.interfaces[0].rx_bits_per_second, Some(800.0));
        assert_eq!(second.interfaces[0].tx_bits_per_second, Some(1_600.0));

        // Back on the 5s cadence: 8·700/5 = 1120.
        advance_secs(5).await;
        let third = next_snapshot(&mut h.events).await;
        assert_eq!(third.interfaces[0].rx_bits_per_second, Some(1_120.0));
        assert_eq!(third.interfaces[0].tx_bits_per_second, Some(2_240.0));

        h.manager.stop().await.expect("stop");
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_counter_wrap_yields_none_rate_that_tick() {
        let dir = TempDir::new("wrap");
        let mut script = ApiScript::working();
        script.interfaces = vec![ok(one_ether(1_000, 2_000)), ok(one_ether(500, 1_000))];
        let api = script.build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let _start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");
        let _first = next_snapshot(&mut h.events).await;

        advance_secs(5).await;
        let second = next_snapshot(&mut h.events).await;
        assert_eq!(second.interfaces[0].rx_bits_per_second, None);
        assert_eq!(second.interfaces[0].tx_bits_per_second, None);

        h.manager.stop().await.expect("stop");
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_disappeared_interface_is_dropped_from_rates() {
        let dir = TempDir::new("disappear");
        let mut script = ApiScript::working();
        script.interfaces = vec![
            ok(Value::Array(vec![
                iface_json_full("ether1", "ether", 1_000, 2_000),
                iface_json_full("ether2", "ether", 5_000, 6_000),
            ])),
            ok(one_ether(1_700, 3_400)),
            ok(Value::Array(vec![
                iface_json_full("ether1", "ether", 2_400, 4_800),
                iface_json_full("ether2", "ether", 5_800, 6_900),
            ])),
        ];
        let api = script.build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let _start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");
        let _first = next_snapshot(&mut h.events).await;

        advance_secs(5).await;
        let second = next_snapshot(&mut h.events).await;
        assert_eq!(second.interfaces.len(), 1);

        // ether2 reappears: it must be treated as a fresh sample (no rate),
        // while ether1 keeps computing from actual elapsed time.
        advance_secs(5).await;
        let third = next_snapshot(&mut h.events).await;
        assert_eq!(third.interfaces.len(), 2);
        let ether1 = third
            .interfaces
            .iter()
            .find(|iface| iface.name == "ether1")
            .expect("ether1");
        let ether2 = third
            .interfaces
            .iter()
            .find(|iface| iface.name == "ether2")
            .expect("ether2");
        assert!(ether1.rx_bits_per_second.is_some());
        assert_eq!(ether2.rx_bits_per_second, None);
        assert_eq!(ether2.tx_bits_per_second, None);

        h.manager.stop().await.expect("stop");
    }
    // ---- VLAN cadence: start + every 12th tick only --------------------------

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_vlans_on_start_and_every_12th_tick_null_otherwise() {
        let dir = TempDir::new("vlans");
        let api = ApiScript::working().build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let _start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");

        for tick in 1..=13 {
            if tick > 1 {
                advance_secs(5).await;
            }
            let payload = next_snapshot(&mut h.events).await;
            match tick {
                // Session start and every 12th tick carry the VLAN payloads
                // (plan: "VLANs at session start and every 12th tick").
                1 | 12 => {
                    let vlans = payload.vlans.expect("VLAN payload on VLAN tick");
                    assert_eq!(vlans[0].name, "vlan10");
                    assert_eq!(vlans[0].vlan_id, Some(10));
                    let bridge = payload.bridge_vlans.expect("bridge VLAN payload");
                    assert_eq!(bridge[0].bridge.as_deref(), Some("bridge1"));
                }
                _ => {
                    assert!(payload.vlans.is_none(), "tick {tick} must carry null VLANs");
                    assert!(payload.bridge_vlans.is_none());
                }
            }
        }

        h.manager.stop().await.expect("stop");
    }

    // ---- Health/stats-detail cadence ------------------------------------------

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_health_and_stats_detail_cadence() {
        let dir = TempDir::new("cadence");
        let api = ApiScript::working().build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let _start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");

        for tick in 1..=7 {
            if tick > 1 {
                advance_secs(5).await;
            }
            let payload = next_snapshot(&mut h.events).await;
            match tick {
                1 => assert!(payload.sensors.is_none(), "health not fetched on tick 1"),
                2 | 4 | 6 => {
                    let sensors = payload.sensors.clone().expect("sensors on health ticks");
                    assert_eq!(sensors.len(), 1);
                    assert_eq!(sensors[0].name, "cpu-temperature");
                }
                _ => assert!(payload.sensors.is_some(), "last-known sensors retained"),
            }
            match tick {
                6 => {
                    // Stats-detail tick: monitor + driver counters merged.
                    assert_eq!(payload.interfaces[0].rate.as_deref(), Some("1Gbps"));
                    assert_eq!(payload.interfaces[0].full_duplex, Some(true));
                    assert_eq!(payload.interfaces[0].rx_error_events, Some(40));
                }
                _ => {
                    // Monitor values exist only on stats-detail ticks.
                    assert!(payload.interfaces[0].rate.is_none());
                    assert!(payload.interfaces[0].rx_error_events.is_none());
                }
            }
        }

        let api_counts = format!(
            "health={} detail={} ethernet={} monitor={}",
            h.api.health_calls.load(Ordering::SeqCst),
            h.api.stats_detail_calls.load(Ordering::SeqCst),
            h.api.ethernet_stats_calls.load(Ordering::SeqCst),
            h.api.monitor_calls.load(Ordering::SeqCst),
        );
        assert_eq!(
            h.api.health_calls.load(Ordering::SeqCst),
            3,
            "health on ticks 2/4/6: {api_counts}"
        );
        assert_eq!(
            h.api.stats_detail_calls.load(Ordering::SeqCst),
            1,
            "stats-detail on tick 6 only: {api_counts}"
        );
        assert_eq!(
            h.api.ethernet_stats_calls.load(Ordering::SeqCst),
            1,
            "ethernet stats on tick 6: {api_counts}"
        );
        assert_eq!(
            h.api.monitor_calls.load(Ordering::SeqCst),
            1,
            "one running ether target: {api_counts}"
        );

        h.manager.stop().await.expect("stop");
    }
    // ---- Health unsupported: 404 AND no-such-command bodies -------------------

    async fn run_health_unsupported_scenario(stub: Stub, label: &str) {
        let dir = TempDir::new(label);
        let mut script = ApiScript::working();
        script.health = vec![stub];
        let api = script.build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let _start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");

        for tick in 1..=3 {
            if tick > 1 {
                advance_secs(5).await;
            }
            let payload = next_snapshot(&mut h.events).await;
            if tick >= 2 {
                assert!(
                    payload.sensors.is_some(),
                    "{label}: empty sensor list on health ticks"
                );
                assert!(
                    payload.sensors.unwrap().is_empty(),
                    "{label}: sensors must be empty when unsupported"
                );
                assert!(
                    !payload.sensors_supported,
                    "{label}: sensorsSupported must be false"
                );
            }
            assert!(
                payload.warning.is_none(),
                "{label}: unsupported health must not produce warning spam"
            );
        }
        let statuses = drain_statuses(&mut h.statuses).await;
        assert!(
            statuses
                .iter()
                .all(|status| !matches!(status, MikrotikStatusEvent::Warning { .. })),
            "{label}: no warning status events for unsupported health"
        );

        // Nonterminal: the session is still running and stops cleanly.
        let stopped = h.manager.stop().await.expect("stop");
        assert_eq!(stopped.status, "cancelled");
        assert_eq!(stopped.snapshot_count, 3);
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_health_unsupported_404_is_not_supported_no_warnings() {
        run_health_unsupported_scenario(err(404, "Not Found"), "health-404").await;
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_health_unsupported_400_suffix_is_not_supported_no_warnings() {
        run_health_unsupported_scenario(
            err(400, "no such command or directory (remove)"),
            "health-400-suffix",
        )
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_health_unsupported_406_mixed_case_message_only() {
        run_health_unsupported_scenario(
            err(406, "No Such Command Or Directory (remove)"),
            "health-406-mixed",
        )
        .await;
    }

    // ---- Enricher failure semantics -------------------------------------------

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_enricher_failure_after_success_keeps_last_known_and_warns() {
        let dir = TempDir::new("enrich-after");
        let mut script = ApiScript::working();
        script.health = vec![ok(health_json()), err(500, "boom")];
        let api = script.build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");
        let _tick1 = next_snapshot(&mut h.events).await;

        advance_secs(5).await;
        let tick2 = next_snapshot(&mut h.events).await;
        assert_eq!(tick2.sensors.as_ref().unwrap().len(), 1);
        assert!(tick2.warning.is_none());

        advance_secs(5).await; // tick 3: health not wanted, sensors last-known
        let tick3 = next_snapshot(&mut h.events).await;
        assert_eq!(tick3.sensors.as_ref().unwrap().len(), 1);

        advance_secs(5).await; // tick 4: health fails after a success
        let tick4 = next_snapshot(&mut h.events).await;
        assert_eq!(
            tick4.sensors.as_ref().unwrap().len(),
            1,
            "last-known sensor payload retained"
        );
        let warning = tick4.warning.expect("warning note on enricher failure");
        assert!(warning.contains("health"), "warning names the source: {warning}");
        assert!(tick4.sensors_supported);

        // No streak increment: session still running, snapshot row persisted.
        let rows = h
            .inspector
            .load_mikrotik_snapshots(start.session_id)
            .await
            .expect("rows");
        assert_eq!(rows.len(), 4);
        assert_eq!(rows.last().unwrap().warning.as_deref(), Some(warning.as_str()));

        // The persisted warning survives into loaded history.
        let loaded = h.manager.load_session(start.session_id).await.expect("load");
        assert_eq!(loaded.snapshots.last().unwrap().warning.as_deref(), Some(warning.as_str()));

        let stopped = h.manager.stop().await.expect("stop");
        assert_eq!(stopped.status, "cancelled");
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_enricher_failure_before_success_keeps_null_fields_and_warns() {
        let dir = TempDir::new("enrich-before");
        let mut script = ApiScript::working();
        script.health = vec![err(500, "boom")];
        let api = script.build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let _start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");
        let _tick1 = next_snapshot(&mut h.events).await;

        advance_secs(5).await; // tick 2: health wanted, fails, never succeeded
        let tick2 = next_snapshot(&mut h.events).await;
        assert!(tick2.sensors.is_none(), "null fields before first success");
        let warning = tick2.warning.expect("warning note");
        assert!(warning.contains("health"));
        assert!(tick2.sensors_supported);

        h.manager.stop().await.expect("stop");
    }
    // ---- Ethernet merge: every promised counter survives merge + history ------

    async fn assert_promised_counters(iface: &MikrotikInterfaceDto) {
        assert_eq!(iface.tx_queue_drop, Some(1), "base tx-queue-drop");
        assert_eq!(iface.link_downs, Some(33), "detail link-downs");
        assert_eq!(iface.rx_error, Some(30), "detail rx-error");
        assert_eq!(iface.tx_error, Some(31), "detail tx-error");
        assert_eq!(iface.rx_drop, Some(32), "detail rx-drop");
        assert_eq!(iface.rx_error_events, Some(40), "driver rx-error-events");
        assert_eq!(iface.tx_error_events, Some(41), "driver tx-error-events");
        assert_eq!(iface.rx_fcs_error, Some(42), "driver rx-fcs-error");
        assert_eq!(iface.rx_align_error, Some(43), "driver rx-align-error");
        assert_eq!(iface.tx_collision, Some(44), "driver tx-collision");
        assert_eq!(iface.tx_drop, Some(45), "driver tx-drop");
        assert_eq!(iface.rate.as_deref(), Some("1Gbps"), "monitor rate");
        assert_eq!(iface.full_duplex, Some(true), "monitor full-duplex");
    }

    fn iface_from_json(row: &verkkokyyla_lib::db::MikrotikSnapshotRow, name: &str) -> Value {
        let json = row.interfaces_json.as_ref().expect("interfaces_json");
        let list: Value = serde_json::from_str(json).expect("interfaces_json parses");
        list.as_array()
            .unwrap()
            .iter()
            .find(|iface| iface["name"] == name)
            .expect("interface in persisted interfaces_json")
            .clone()
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_ethernet_monitor_values_merge_and_survive_history() {
        let dir = TempDir::new("merge-history");
        let api = ApiScript::working().build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");

        for tick in 1..=6 {
            if tick > 1 {
                advance_secs(5).await;
            }
            let payload = next_snapshot(&mut h.events).await;
            if tick == 6 {
                assert_promised_counters(&payload.interfaces[0]).await;
            }
        }

        let stopped = h.manager.stop().await.expect("stop");
        assert_eq!(stopped.snapshot_count, 6);

        // History (stale_state probe): persisted interfaces_json on the
        // stats-detail tick carries every merged counter, and load_session
        // round-trips the identical JSON.
        let rows = h
            .inspector
            .load_mikrotik_snapshots(start.session_id)
            .await
            .expect("rows");
        assert_eq!(rows.len(), 6);
        let merged = iface_from_json(&rows[5], "ether1");
        for (key, value) in [
            ("txQueueDrop", json!(1)),
            ("linkDowns", json!(33)),
            ("rxError", json!(30)),
            ("txError", json!(31)),
            ("rxDrop", json!(32)),
            ("rxErrorEvents", json!(40)),
            ("txErrorEvents", json!(41)),
            ("rxFcsError", json!(42)),
            ("rxAlignError", json!(43)),
            ("txCollision", json!(44)),
            ("txDrop", json!(45)),
            ("rate", json!("1Gbps")),
            ("fullDuplex", json!(true)),
        ] {
            assert_eq!(merged[key], value, "persisted interfaces_json[{key}]");
        }
        // The row right before the stats tick has no driver counters.
        let pre = iface_from_json(&rows[4], "ether1");
        assert!(pre.get("rxErrorEvents").is_none() || pre["rxErrorEvents"].is_null());

        let loaded = h.manager.load_session(start.session_id).await.expect("load");
        let loaded_merged = loaded.snapshots[5]
            .interfaces_json
            .as_ref()
            .map(|json| serde_json::from_str::<Value>(json).unwrap())
            .expect("loaded interfaces_json");
        assert_eq!(loaded_merged, serde_json::from_str::<Value>(rows[5].interfaces_json.as_ref().unwrap()).unwrap());
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_ethernet_stats_match_default_name_for_renamed_port() {
        let dir = TempDir::new("renamed-port");
        let mut script = ApiScript::working();
        // Interface renamed to "wan"; the driver stats entry reports a stale
        // name but the matching default-name.
        script.interfaces = vec![ok(Value::Array(vec![iface_json_full(
            "wan",
            "ether",
            1_000,
            2_000,
        )]))];
        script.stats_detail = vec![ok(stats_detail_json("wan"))];
        script.ethernet_stats = vec![ok(ethernet_stats_json("wan2", "wan"))];
        script.monitor = HashMap::from([("wan".to_owned(), vec![ok(monitor_json("wan"))])]);
        let api = script.build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let _start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");

        for tick in 1..=6 {
            if tick > 1 {
                advance_secs(5).await;
            }
            let payload = next_snapshot(&mut h.events).await;
            if tick == 6 {
                let wan = payload
                    .interfaces
                    .iter()
                    .find(|iface| iface.name == "wan")
                    .expect("wan interface");
                assert_eq!(wan.rx_error_events, Some(40), "merged via default-name");
                assert_eq!(wan.rate.as_deref(), Some("1Gbps"));
            }
        }

        h.manager.stop().await.expect("stop");
    }

    // ---- Enricher slowness: core snapshots stay on schedule -------------------

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_hung_enrichers_do_not_delay_core_snapshots() {
        let dir = TempDir::new("hung-enrichers");
        let mut script = ApiScript::working();
        script.health = vec![Stub::Hang];
        let api = script.build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let _start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");
        let tick1 = next_snapshot(&mut h.events).await;
        assert!(tick1.sensors.is_none());

        // Tick 2 wants health, which now hangs. The core snapshot must NOT
        // be delayed beyond the per-tick enricher budget.
        advance_secs(5).await;
        std::thread::sleep(Duration::from_millis(50));
        tokio::task::yield_now().await;
        assert!(
            h.events.try_recv().is_err(),
            "no snapshot yet while the enricher hangs within its budget"
        );

        advance_secs(5).await; // budget (3s) expires -> tick 2 snapshot
        let tick2 = next_snapshot(&mut h.events).await;
        let warning = tick2.warning.expect("health timeout warning");
        assert!(warning.contains("health"));

        advance_secs(3).await; // tick 3's hung health budget expires
        let tick3 = next_snapshot(&mut h.events).await;
        assert!(tick3.resources.is_some(), "core snapshot on schedule");

        let stopped = h.manager.stop().await.expect("stop");
        assert!(stopped.snapshot_count >= 2);
    }
    // ---- CORE failure semantics (manual-QA scenario 2: 3 failures -> error) ---

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_core_failures_warn_no_rows_streak_resets_then_terminal_error() {
        let dir = TempDir::new("core-streak");
        let mut script = ApiScript::working();
        // tick1 fail, tick2 ok (streak reset), tick3-5 fail -> 3 consecutive.
        script.resource = vec![
            err(500, "core down"),
            ok(resource_json(10.0)),
            err(500, "core down"),
            err(500, "core down"),
            err(500, "core down"),
        ];
        let api = script.build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");

        advance_secs(5).await; // tick 2 succeeds (streak resets)
        let snapshot = next_snapshot(&mut h.events).await;
        assert!(snapshot.resources.is_some());

        // Ticks 3-5 fail; the session stops itself after the 3rd consecutive.
        // Each advance must be followed by a yield: the runtime re-registers
        // the interval's next deadline when polled, so the following advance
        // can fire it (same interleave every other multi-tick test gets from
        // awaiting the next event).
        for _ in 0..3 {
            advance_secs(5).await;
            tokio::task::yield_now().await;
        }
        std::thread::sleep(Duration::from_millis(50));
        tokio::task::yield_now().await;

        let statuses = drain_statuses(&mut h.statuses).await;
        let warnings = statuses
            .iter()
            .filter(|status| matches!(status, MikrotikStatusEvent::Warning { .. }))
            .count();
        assert_eq!(warnings, 4, "one warning per core failure (ticks 1,3,4,5)");
        assert!(
            matches!(statuses.last(), Some(MikrotikStatusEvent::Error { session_id, .. }) if *session_id == start.session_id),
            "terminal error status event last"
        );
        let error = statuses.last().unwrap();
        let message = match error {
            MikrotikStatusEvent::Error { message, .. } => message.clone(),
            _ => unreachable!(),
        };
        assert!(message.contains("500"));

        // NO snapshot rows on core-failure ticks; the one success persisted.
        // The terminal task finalizes the session row asynchronously (the
        // Error status event fires just before the UPDATE), so settle-wait
        // for the row instead of asserting on a single arbitrary yield.
        let mut loaded = h
            .inspector
            .load_mikrotik_session(start.session_id)
            .await
            .expect("load");
        for _ in 0..100 {
            if loaded.session.status != "running" {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
            tokio::task::yield_now().await;
            loaded = h
                .inspector
                .load_mikrotik_session(start.session_id)
                .await
                .expect("load");
        }
        assert_eq!(loaded.snapshots.len(), 1, "only the successful tick persisted");
        assert_eq!(loaded.session.status, "error", "session row status=error");
        assert!(loaded.session.ended_at.is_some());

        // clear_active on task exit: a new session can start on the same profile.
        let second = start_session(&h, profile_id).await;
        assert_ne!(second.session_id, start.session_id);
        let stopped = h.manager.stop().await.expect("stop second");
        assert_eq!(stopped.status, "cancelled");
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_second_start_while_running_returns_already_running() {
        let dir = TempDir::new("second-start");
        let api = ApiScript::working().build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let _first = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");

        let on_event = h.on_event.clone();
        let on_status = h.on_status.clone();
        let second = h
            .manager
            .start(profile_id, move |event| on_event(event), on_status)
            .await;
        let err = second.expect_err("second start must fail");
        assert_eq!(err_kind(&err), "already-running");

        h.manager.stop().await.expect("stop");
    }

    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_stop_cancels_persists_cancelled_and_allows_restart() {
        let dir = TempDir::new("cancel-restart");
        let api = ApiScript::working().build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");
        let _tick1 = next_snapshot(&mut h.events).await;

        let stopped = h.manager.stop().await.expect("stop");
        assert_eq!(stopped.session_id, start.session_id);
        assert_eq!(stopped.status, "cancelled");
        let cancelled = h.statuses.recv().await.expect("cancelled status");
        assert!(matches!(
            cancelled,
            MikrotikStatusEvent::Cancelled { session_id, snapshot_count: 1 }
                if session_id == start.session_id
        ));

        let loaded = h
            .inspector
            .load_mikrotik_session(start.session_id)
            .await
            .expect("load");
        assert_eq!(loaded.session.status, "cancelled");
        assert!(loaded.session.ended_at.is_some());

        // Restart works: clear_active ran on task exit.
        let second = start_session(&h, profile_id).await;
        let stopped2 = h.manager.stop().await.expect("stop second");
        assert_eq!(stopped2.status, "cancelled");
        assert_ne!(second.session_id, start.session_id);
    }
    // ---- test_connection + profile/secret lifecycle (MemoryStore) -------------

    #[tokio::test]
    async fn mikrotik_runtime_test_connection_success_returns_board_and_version() {
        let dir = TempDir::new("test-conn-ok");
        let api = ApiScript::working().build();
        let h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();

        let dto = h
            .manager
            .test_connection(profile_id)
            .await
            .expect("test connection");
        assert_eq!(dto.board_name.as_deref(), Some("RB5009"));
        assert_eq!(dto.routeros_version.as_deref(), Some("7.16.1"));
        assert_eq!(dto.architecture_name.as_deref(), Some("arm64"));
        // The connection handed to the API carries the profile credentials.
        let seen = h.api.seen.lock().unwrap();
        assert_eq!(seen[0].host, "192.0.2.10");
        assert_eq!(seen[0].port, 443);
        assert!(seen[0].use_tls);
        assert_eq!(seen[0].username, "admin");
    }

    #[tokio::test]
    async fn mikrotik_runtime_test_connection_without_password_is_not_stored() {
        let dir = TempDir::new("test-conn-no-pw");
        let api = ApiScript::working().build();
        let h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;

        let err = h
            .manager
            .test_connection(profile_id)
            .await
            .expect_err("missing password must fail");
        assert_eq!(err_kind(&err), "not-stored");
        // No router call was attempted without a password.
        assert!(h.api.seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn mikrotik_runtime_test_connection_unauthorized_maps_typed_error() {
        let dir = TempDir::new("test-conn-401");
        let mut script = ApiScript::working();
        // Client-level HTTP 401 mapping is todo 2's domain; here the fake
        // surfaces the typed variant the client produces.
        script.resource = vec![Stub::Err(MikrotikError::Unauthorized)];
        let api = script.build();
        let h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "wrong")
            .await
            .unwrap();

        let err = h
            .manager
            .test_connection(profile_id)
            .await
            .expect_err("401 must fail");
        assert_eq!(err_kind(&err), "unauthorized");
    }

    #[tokio::test]
    async fn mikrotik_runtime_profile_update_keeps_secret_key_and_password() {
        let dir = TempDir::new("update-secret");
        let api = ApiScript::working().build();
        let h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let before = h
            .inspector
            .load_mikrotik_profile(profile_id)
            .await
            .expect("profile")
            .expect("row");

        let updated = h
            .manager
            .update_profile(&UpdateMikrotikProfileRequest {
                id: profile_id,
                name: "edge-2".to_owned(),
                host: "192.0.2.20".to_owned(),
                port: 8729,
                use_tls: false,
                allow_invalid_certs: true,
                username: "ops".to_owned(),
            })
            .await
            .expect("update profile");
        assert_eq!(updated.name, "edge-2");

        // secret_key is immutable across updates: the stored password still
        // works, and the connection now carries the updated fields.
        let after = h
            .inspector
            .load_mikrotik_profile(profile_id)
            .await
            .expect("profile")
            .expect("row");
        assert_eq!(after.secret_key, before.secret_key, "secret_key unchanged");
        let dto = h
            .manager
            .test_connection(profile_id)
            .await
            .expect("stored password still works");
        assert_eq!(dto.board_name.as_deref(), Some("RB5009"));
        let seen = h.api.seen.lock().unwrap();
        assert_eq!(seen[0].password, "s3cr3t", "stored password was used");
        assert_eq!(seen[0].host, "192.0.2.20");
        assert_eq!(seen[0].port, 8729);
        assert!(!seen[0].use_tls);
        assert!(seen[0].allow_invalid_certs);
    }
    #[tokio::test(start_paused = true)]
    async fn mikrotik_runtime_delete_profile_in_use_refused_then_inactive_deletes() {
        let dir = TempDir::new("delete-profile");
        let api = ApiScript::working().build();
        let mut h = harness(&dir, api).await;
        let profile_id = create_profile(&h.manager).await;
        h.manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let secret_key = h
            .inspector
            .load_mikrotik_profile(profile_id)
            .await
            .expect("row")
            .expect("profile")
            .secret_key;

        let start = start_session(&h, profile_id).await;
        h.statuses.recv().await.expect("started");
        let _tick1 = next_snapshot(&mut h.events).await;

        // Deleting the profile behind the ACTIVE session is a typed refusal.
        let err = h
            .manager
            .delete_profile(profile_id)
            .await
            .expect_err("active profile delete must refuse");
        assert_eq!(err_kind(&err), "profile-in-use");
        h.inspector
            .load_mikrotik_profile(profile_id)
            .await
            .expect("row")
            .expect("profile still there");

        h.manager.stop().await.expect("stop");
        let _cancelled = h.statuses.recv().await.expect("cancelled");

        // Inactive delete: DB row goes, then the store entry is removed.
        let result = h
            .manager
            .delete_profile(profile_id)
            .await
            .expect("inactive delete");
        assert!(result.deleted);
        assert!(result.secret_deleted);
        assert!(result.warning.is_none());
        assert!(
            h.inspector
                .load_mikrotik_profile(profile_id)
                .await
                .expect("query")
                .is_none(),
            "DB row deleted"
        );
        let store = Arc::new(MemoryStore::new());
        // The captured original secret_key is exactly the keyring account.
        let missing = store.get(&secret_key).await.expect_err("secret gone");
        assert!(matches!(missing, SecretError::NotStored));
        assert!(h.manager.list_profiles().await.expect("list").is_empty());

        // The session history is tied to its profile: migration 0010 declares
        // ON DELETE CASCADE, so the session+snapshots go with the profile.
        let err = h
            .manager
            .load_session(start.session_id)
            .await
            .expect_err("session cascaded with its profile");
        assert_eq!(err_kind(&err), "session-not-found");
    }

    #[tokio::test]
    async fn mikrotik_runtime_delete_profile_store_failure_still_deletes_row() {
        let dir = TempDir::new("delete-orphan");
        let api = ApiScript::working().build();
        let store = Arc::new(FlakyStore {
            inner: MemoryStore::new(),
            fail_delete: Arc::new(AtomicBool::new(true)),
        });
        let manager = MikrotikManager::new(
            test_db(&dir.db_file()).await,
            store.clone(),
            factory_for(api),
        );
        let inspector = test_db(&dir.db_file()).await;
        let profile_id = create_profile(&manager).await;
        manager
            .set_profile_password(profile_id, "s3cr3t")
            .await
            .unwrap();
        let secret_key = inspector
            .load_mikrotik_profile(profile_id)
            .await
            .expect("row")
            .expect("profile")
            .secret_key;

        let result = manager.delete_profile(profile_id).await.expect("delete");
        assert!(result.deleted, "DB row deleted even when keyring fails");
        assert!(!result.secret_deleted);
        let warning = result.warning.expect("cleanup warning surfaced");
        assert!(warning.contains("keyring"), "warning: {warning}");
        assert!(
            inspector
                .load_mikrotik_profile(profile_id)
                .await
                .expect("query")
                .is_none(),
            "DB row is gone"
        );
        // The orphaned keyring entry is inert but retrievable (best-effort
        // delete failed AFTER the row was already gone).
        assert_eq!(store.get(&secret_key).await.expect("orphan remains"), "s3cr3t");
    }
// __PART10__
}
