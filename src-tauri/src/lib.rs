pub mod db;
pub mod download;
pub mod engine;
pub mod session;
pub mod stats;

use std::sync::Arc;

use tauri::Manager;

use session::{
    LoadedSessionDto, ProbeEvent, SessionError, SessionManager, SessionSummaryDto, SnapshotDto,
    StartInfoDto, StatusEvent, StoppedSessionDto,
};

#[tauri::command]
async fn run_download_speed_test(
    url: String,
    on_progress: tauri::ipc::Channel<download::DownloadProgressEvent>,
) -> Result<download::DownloadSpeedResultDto, download::DownloadError> {
    download::run_download_speed_test(&url, move |event| {
        let _ = on_progress.send(event);
    })
    .await
}

#[tauri::command]
async fn start_session(
    manager: tauri::State<'_, SessionManager>,
    target: String,
    family: String,
    payload_size: usize,
    dont_fragment: bool,
    on_probe: tauri::ipc::Channel<ProbeEvent>,
    on_status: tauri::ipc::Channel<StatusEvent>,
) -> Result<StartInfoDto, SessionError> {
    manager
        .start(
            &target,
            &family,
            payload_size,
            dont_fragment,
            move |event| {
                let _ = on_probe.send(event);
            },
            Arc::new(move |event| {
                let _ = on_status.send(event);
            }),
        )
        .await
}

#[tauri::command]
async fn stop_session(
    manager: tauri::State<'_, SessionManager>,
    session_id: i64,
) -> Result<StoppedSessionDto, SessionError> {
    manager.stop(session_id).await
}

#[tauri::command]
fn get_snapshot(
    manager: tauri::State<'_, SessionManager>,
    session_id: i64,
) -> SnapshotDto {
    manager.snapshot(session_id)
}

#[tauri::command]
fn list_active_sessions(manager: tauri::State<'_, SessionManager>) -> Vec<i64> {
    manager.active_session_ids()
}

#[tauri::command]
async fn list_sessions(
    manager: tauri::State<'_, SessionManager>,
) -> Result<Vec<SessionSummaryDto>, SessionError> {
    manager.list_sessions().await
}

#[tauri::command]
async fn load_session(
    manager: tauri::State<'_, SessionManager>,
    id: i64,
) -> Result<LoadedSessionDto, SessionError> {
    manager.load_session(id).await
}

#[tauri::command]
async fn delete_session(
    manager: tauri::State<'_, SessionManager>,
    id: i64,
) -> Result<(), SessionError> {
    manager.delete_session(id).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let db = tauri::async_runtime::block_on(db::Database::connect(&db::db_path(
                &data_dir,
            )))?;
            app.manage(SessionManager::new(db, session::default_engine_factory()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_session,
            stop_session,
            get_snapshot,
            list_active_sessions,
            list_sessions,
            load_session,
            delete_session,
            run_download_speed_test
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
