pub mod db;
pub mod download;
pub mod engine;
pub mod http_client;
pub mod page_speed;
pub mod session;
pub mod trace;
pub mod stats;

use std::net::IpAddr;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use tauri::Manager;
use tokio::task::spawn_blocking;

use session::{
    LoadedSessionDto, ProbeEvent, SessionError, SessionManager, SessionSummaryDto, SnapshotDto,
    StartInfoDto, StatusEvent, StoppedSessionDto,
};
#[cfg(unix)]
use engine::TracePosix;
#[cfg(windows)]
use engine::TracertWin;
use trace::{
    LoadedTraceDto, StartTraceDto, StoppedTraceDto, TraceError, TraceEvent, TraceManager,
    TraceStatusEvent, TraceSummaryDto,
};

impl trace::TraceStream for engine::RawHopStream {
    fn next<'a>(&'a mut self) -> BoxFuture<'a, Result<Option<engine::trace_parse::RawHop>, engine::TraceEngineError>> {
        Box::pin(async move { engine::RawHopStream::next(self).await })
    }
}

fn os_trace_factory() -> trace::TraceFactory {
    Arc::new(move |address: IpAddr| {
        Box::pin(async move {
            #[cfg(windows)]
            {
                Ok(Box::new(TracertWin::new(address)?.start()?) as Box<dyn trace::TraceStream>)
            }

            #[cfg(unix)]
            {
                Ok(Box::new(TracePosix::new(address)?.start()?) as Box<dyn trace::TraceStream>)
            }
        })
    })
}

fn trace_resolver() -> trace::TraceResolver {
    Arc::new(move |address: IpAddr| {
        Box::pin(async move {
            match spawn_blocking(move || dns_lookup::lookup_addr(&address)).await {
                Ok(Ok(name)) => Some(name),
                _ => None,
            }
        })
    })
}

#[tauri::command]
async fn run_download_speed_test(
    url: String,
    settings: http_client::HttpSettingsDto,
    on_progress: tauri::ipc::Channel<download::DownloadProgressEvent>,
) -> Result<download::DownloadSpeedResultDto, download::DownloadError> {
    download::run_download_speed_test(&url, settings, move |event| {
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
async fn start_trace(
    manager: tauri::State<'_, TraceManager>,
    target: String,
    family: String,
    on_event: tauri::ipc::Channel<TraceEvent>,
    on_status: tauri::ipc::Channel<TraceStatusEvent>,
) -> Result<StartTraceDto, TraceError> {
    manager
        .start(
            &target,
            &family,
            move |event| {
                let _ = on_event.send(event);
            },
            Arc::new(move |event| {
                let _ = on_status.send(event);
            }),
        )
        .await
}

#[tauri::command]
async fn stop_trace(
    manager: tauri::State<'_, TraceManager>,
) -> Result<StoppedTraceDto, TraceError> {
    manager.stop().await
}

#[tauri::command]
async fn list_traces(
    manager: tauri::State<'_, TraceManager>,
) -> Result<Vec<TraceSummaryDto>, TraceError> {
    manager.list_traces().await
}

#[tauri::command]
async fn load_trace(
    manager: tauri::State<'_, TraceManager>,
    id: i64,
) -> Result<LoadedTraceDto, TraceError> {
    manager.load_trace(id).await
}

#[tauri::command]
async fn delete_trace(
    manager: tauri::State<'_, TraceManager>,
    id: i64,
) -> Result<(), TraceError> {
    manager.delete_trace(id).await
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

#[tauri::command]
async fn run_page_speed_test(
    url: String,
    settings: http_client::HttpSettingsDto,
    on_progress: tauri::ipc::Channel<page_speed::PageProgressEvent>,
) -> Result<page_speed::PageSpeedResultDto, download::DownloadError> {
    page_speed::run_page_speed_test(&url, settings, move |event| {
        let _ = on_progress.send(event);
    })
    .await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let db_path = db::db_path(&data_dir);
            let session_db = tauri::async_runtime::block_on(db::Database::connect(&db_path))?;
            let trace_db = tauri::async_runtime::block_on(db::Database::connect(&db_path))?;
            app.manage(SessionManager::new(session_db, session::default_engine_factory()));
            app.manage(TraceManager::new(trace_db, os_trace_factory(), trace_resolver()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_session,
            stop_session,
            start_trace,
            stop_trace,
            list_traces,
            load_trace,
            delete_trace,
            get_snapshot,
            list_active_sessions,
            list_sessions,
            load_session,
            delete_session,
            run_download_speed_test,
            run_page_speed_test
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
