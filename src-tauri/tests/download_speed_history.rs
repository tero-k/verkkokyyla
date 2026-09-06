use verkkokyyla_lib::db::{
    now_rfc3339, Database, LoadedDownloadSpeedSession, NewDownloadSpeedSession,
};

struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "verkkokyyla-download-speed-{name}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    fn db_file(&self) -> std::path::PathBuf {
        self.0.join("nested").join("test.db")
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn sample_new() -> NewDownloadSpeedSession {
    NewDownloadSpeedSession {
        url: "https://example.com/file.bin".to_string(),
        mode: "single".to_string(),
        http_settings_json: r#"{"version":"auto"}"#.to_string(),
        started_at: now_rfc3339(),
    }
}

#[tokio::test]
async fn download_speed_session_round_trip_and_delete() {
    let dir = TestDir::new("round-trip");
    let db = Database::connect(&dir.db_file()).await.expect("connect");

    let id = db
        .create_download_speed_session(&sample_new())
        .await
        .expect("create session");

    db.finish_download_speed_session(
        id,
        &now_rfc3339(),
        "completed",
        8.39,
        1000,
        r#"{"url":"https://example.com/file.bin","averageMbps":8.39}"#,
    )
    .await
    .expect("finish session");

    let sessions = db
        .list_download_speed_sessions()
        .await
        .expect("list sessions");
    assert_eq!(sessions.len(), 1);
    let session = &sessions[0];
    assert_eq!(session.id, id);
    assert_eq!(session.url, "https://example.com/file.bin");
    assert_eq!(session.mode, "single");
    assert_eq!(session.status, "completed");
    assert!((session.average_mbps - 8.39).abs() < 1e-9);
    assert_eq!(session.total_time_ms, 1000);

    let loaded: LoadedDownloadSpeedSession = db
        .load_download_speed_session(id)
        .await
        .expect("load session");
    assert_eq!(loaded.session, *session);
    assert!(loaded.result_json.contains("averageMbps"));

    db.delete_download_speed_session(id)
        .await
        .expect("delete session");
    assert!(db
        .list_download_speed_sessions()
        .await
        .expect("list after delete")
        .is_empty());
}

#[tokio::test]
async fn migration_adds_download_speed_sessions_table() {
    let dir = TestDir::new("migration");
    let db = Database::connect(&dir.db_file()).await.expect("connect");

    // The migration ran successfully if list_download_speed_sessions works.
    db.list_download_speed_sessions().await.expect("list sessions");
}
