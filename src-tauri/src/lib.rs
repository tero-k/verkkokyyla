pub mod bench;
pub mod bench_stats;
pub mod db;
pub mod dns;
pub mod download;
pub mod download_manager;
pub mod engine;
pub mod http_client;
pub mod mikrotik;
pub mod page_speed;
pub mod scan;
mod scan_commands;
pub mod session;
pub mod stats;
pub mod trace;

use std::net::IpAddr;
use std::sync::Arc;

use futures_util::future::BoxFuture;
use tauri::Manager;
use tokio::task::spawn_blocking;

use dns::manager::DnsManager;
use download_manager::{
    DownloadSpeedHistoryError, DownloadSpeedManager, DownloadSpeedSessionSummaryDto,
    LoadedDownloadSpeedSessionDto, SaveDownloadSpeedSessionRequest,
};

#[cfg(unix)]
use engine::TracePosix;
#[cfg(windows)]
use engine::TracertWin;
use scan::ScanManager;
use session::{
    LoadedSessionDto, ProbeEvent, SessionError, SessionManager, SessionSummaryDto, SnapshotDto,
    StartInfoDto, StatusEvent, StoppedSessionDto,
};
use trace::{
    LoadedTraceDto, StartTraceDto, StoppedTraceDto, TraceError, TraceEvent, TraceManager,
    TraceStatusEvent, TraceSummaryDto,
};
use mikrotik::manager::MikrotikManager;

impl trace::TraceStream for engine::RawHopStream {
    fn next<'a>(
        &'a mut self,
    ) -> BoxFuture<'a, Result<Option<engine::trace_parse::RawHop>, engine::TraceEngineError>> {
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
async fn delete_trace(manager: tauri::State<'_, TraceManager>, id: i64) -> Result<(), TraceError> {
    manager.delete_trace(id).await
}

#[tauri::command]
async fn mikrotik_list_profiles(
    manager: tauri::State<'_, mikrotik::manager::MikrotikManager>,
) -> Result<Vec<mikrotik::types::MikrotikProfileDto>, mikrotik::types::MikrotikManagerError> {
    manager.list_profiles().await
}

#[tauri::command]
async fn mikrotik_create_profile(
    manager: tauri::State<'_, mikrotik::manager::MikrotikManager>,
    request: mikrotik::types::CreateMikrotikProfileRequest,
) -> Result<mikrotik::types::MikrotikProfileDto, mikrotik::types::MikrotikManagerError> {
    manager.create_profile(&request).await
}

#[tauri::command]
async fn mikrotik_update_profile(
    manager: tauri::State<'_, mikrotik::manager::MikrotikManager>,
    request: mikrotik::types::UpdateMikrotikProfileRequest,
) -> Result<mikrotik::types::MikrotikProfileDto, mikrotik::types::MikrotikManagerError> {
    manager.update_profile(&request).await
}

#[tauri::command]
async fn mikrotik_delete_profile(
    manager: tauri::State<'_, mikrotik::manager::MikrotikManager>,
    id: i64,
) -> Result<mikrotik::types::DeleteProfileResultDto, mikrotik::types::MikrotikManagerError> {
    manager.delete_profile(id).await
}

#[tauri::command]
async fn mikrotik_set_profile_password(
    manager: tauri::State<'_, mikrotik::manager::MikrotikManager>,
    id: i64,
    password: String,
) -> Result<(), mikrotik::types::MikrotikManagerError> {
    manager.set_profile_password(id, &password).await
}

#[tauri::command]
async fn mikrotik_test_connection(
    manager: tauri::State<'_, mikrotik::manager::MikrotikManager>,
    id: i64,
) -> Result<mikrotik::types::MikrotikTestConnectionDto, mikrotik::types::MikrotikManagerError>
{
    manager.test_connection(id).await
}

#[tauri::command]
async fn mikrotik_start(
    manager: tauri::State<'_, mikrotik::manager::MikrotikManager>,
    profile_id: i64,
    on_event: tauri::ipc::Channel<mikrotik::types::MikrotikEvent>,
    on_status: tauri::ipc::Channel<mikrotik::types::MikrotikStatusEvent>,
) -> Result<mikrotik::types::MikrotikStartDto, mikrotik::types::MikrotikManagerError> {
    manager
        .start(
            profile_id,
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
async fn mikrotik_stop(
    manager: tauri::State<'_, mikrotik::manager::MikrotikManager>,
) -> Result<mikrotik::types::MikrotikStoppedDto, mikrotik::types::MikrotikManagerError> {
    manager.stop().await
}

#[tauri::command]
async fn mikrotik_list_sessions(
    manager: tauri::State<'_, mikrotik::manager::MikrotikManager>,
) -> Result<Vec<mikrotik::types::MikrotikSessionSummaryDto>, mikrotik::types::MikrotikManagerError>
{
    manager.list_sessions().await
}

#[tauri::command]
async fn mikrotik_load_session(
    manager: tauri::State<'_, mikrotik::manager::MikrotikManager>,
    id: i64,
) -> Result<mikrotik::types::LoadedMikrotikSessionDto, mikrotik::types::MikrotikManagerError> {
    manager.load_session(id).await
}

#[tauri::command]
async fn mikrotik_delete_session(
    manager: tauri::State<'_, mikrotik::manager::MikrotikManager>,
    id: i64,
) -> Result<(), mikrotik::types::MikrotikManagerError> {
    manager.delete_session(id).await
}

#[tauri::command]
fn get_snapshot(manager: tauri::State<'_, SessionManager>, session_id: i64) -> SnapshotDto {
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

#[tauri::command]
async fn run_web_benchmark(
    config: bench::BenchmarkConfig,
) -> Result<bench::BenchmarkResult, bench::BenchmarkError> {
    bench::run_benchmark(config).await
}

#[tauri::command]
async fn dns_lookup(
    name: String,
    record_types: Vec<String>,
    endpoint: dns::client::ResolverEndpointDto,
    on_event: tauri::ipc::Channel<dns::manager::LookupEventDto>,
    manager: tauri::State<'_, DnsManager>,
) -> Result<dns::manager::LookupSummaryDto, dns::error::DnsError> {
    manager
        .run_lookup(&name, record_types, endpoint, move |event| {
            let _ = on_event.send(event);
        })
        .await
}

#[tauri::command]
async fn dns_diagnostics(
    endpoint: dns::client::ResolverEndpointDto,
    domain: String,
    manager: tauri::State<'_, DnsManager>,
) -> Result<dns::diagnostics::DnsDiagnosticsDto, dns::error::DnsError> {
    let report = manager.run_diagnostics(endpoint.clone(), &domain).await?;
    let _ = manager
        .persist_diagnostics(&endpoint.name, &domain, &report)
        .await;
    Ok(report)
}

#[tauri::command]
async fn dns_email_check(
    endpoint: dns::client::ResolverEndpointDto,
    domain: String,
    dkim_selectors: Vec<String>,
    manager: tauri::State<'_, DnsManager>,
) -> Result<dns::email::EmailSecurityReportDto, dns::error::DnsError> {
    manager
        .run_email_check(endpoint, &domain, dkim_selectors)
        .await
}

#[tauri::command]
async fn dns_benchmark(
    endpoint: dns::client::ResolverEndpointDto,
    profile_json: String,
    on_cell: tauri::ipc::Channel<dns::bench::scheduler::SampleCell>,
    manager: tauri::State<'_, DnsManager>,
) -> Result<dns::manager::BenchmarkRunDto, dns::error::DnsError> {
    let profile: dns::bench::profiles::BenchmarkProfile = serde_json::from_str(&profile_json)
        .map_err(|e| dns::error::DnsError::InvalidInput(format!("invalid profile: {e}")))?;
    manager
        .run_benchmark(endpoint, profile, move |cell| {
            let _ = on_cell.send(cell);
        })
        .await
}

#[tauri::command]
async fn list_dns_runs(
    manager: tauri::State<'_, DnsManager>,
) -> Result<Vec<dns::manager::DnsRunSummaryDto>, dns::error::DnsError> {
    manager.list_runs().await
}

#[tauri::command]
async fn load_dns_run(
    id: i64,
    manager: tauri::State<'_, DnsManager>,
) -> Result<dns::manager::LoadedDnsRunDto, dns::error::DnsError> {
    manager.load_run(id).await
}

#[tauri::command]
async fn delete_dns_run(
    id: i64,
    manager: tauri::State<'_, DnsManager>,
) -> Result<(), dns::error::DnsError> {
    manager.delete_run(id).await
}

#[tauri::command]
async fn save_download_speed_session(
    manager: tauri::State<'_, DownloadSpeedManager>,
    request: SaveDownloadSpeedSessionRequest,
) -> Result<DownloadSpeedSessionSummaryDto, DownloadSpeedHistoryError> {
    manager.save_session(request).await
}

#[tauri::command]
async fn list_download_speed_sessions(
    manager: tauri::State<'_, DownloadSpeedManager>,
) -> Result<Vec<DownloadSpeedSessionSummaryDto>, DownloadSpeedHistoryError> {
    manager.list_sessions().await
}

#[tauri::command]
async fn load_download_speed_session(
    manager: tauri::State<'_, DownloadSpeedManager>,
    id: i64,
) -> Result<LoadedDownloadSpeedSessionDto, DownloadSpeedHistoryError> {
    manager.load_session(id).await
}

#[tauri::command]
async fn delete_download_speed_session(
    manager: tauri::State<'_, DownloadSpeedManager>,
    id: i64,
) -> Result<(), DownloadSpeedHistoryError> {
    manager.delete_session(id).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let db_path = db::db_path(&data_dir);
            let session_db = tauri::async_runtime::block_on(db::Database::connect(&db_path))?;
            let trace_db = tauri::async_runtime::block_on(db::Database::connect(&db_path))?;
            let scan_db = tauri::async_runtime::block_on(db::Database::connect(&db_path))?;
            let dns_db = tauri::async_runtime::block_on(db::Database::connect(&db_path))?;
            app.manage(SessionManager::new(
                session_db,
                session::default_engine_factory(),
            ));
            app.manage(TraceManager::new(
                trace_db,
                os_trace_factory(),
                trace_resolver(),
            ));
            app.manage(ScanManager::new(scan_db));
            app.manage(DnsManager::new(Arc::new(dns_db)));
            app.manage(DownloadSpeedManager::new(Arc::new(
                tauri::async_runtime::block_on(db::Database::connect(&db_path))?,
            )));
            app.manage(mikrotik::backup::MikrotikBackupState::new(
                tauri::async_runtime::block_on(db::Database::connect(&db_path))?,
            ));
            app.manage(MikrotikManager::new(
                tauri::async_runtime::block_on(db::Database::connect(&db_path))?,
                Arc::new(mikrotik::secrets::KeyringStore::new()),
                Arc::new(move |conn| {
                    Box::pin(async move {
                        Ok(Arc::new(mikrotik::client::MikrotikClient::new(&conn)?)
                            as Arc<dyn mikrotik::types::MikrotikApi>)
                    })
                }),
            ));
            app.manage(mikrotik::changelog::ChangelogService::new(Arc::new(
                mikrotik::changelog::CachedChangelogFetcher::new()?,
            )));
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
            scan_commands::list_interfaces,
            scan_commands::start_scan,
            scan_commands::stop_scan,
            scan_commands::list_scans,
            scan_commands::load_scan,
            scan_commands::delete_scan,
            get_snapshot,
            list_active_sessions,
            list_sessions,
            load_session,
            delete_session,
            run_download_speed_test,
            run_page_speed_test,
            run_web_benchmark,
            save_download_speed_session,
            list_download_speed_sessions,
            load_download_speed_session,
            delete_download_speed_session,
            mikrotik_list_profiles,
            mikrotik_create_profile,
            mikrotik_update_profile,
            mikrotik_delete_profile,
            mikrotik_set_profile_password,
            mikrotik_test_connection,
            mikrotik_start,
            mikrotik_stop,
            mikrotik_list_sessions,
            mikrotik_load_session,
            mikrotik_delete_session,
            mikrotik::version::mikrotik_check_updates,
            mikrotik::changelog::mikrotik_changelog,
            dns_lookup,
            dns_diagnostics,
            dns_email_check,
            dns_benchmark,
            list_dns_runs,
            load_dns_run,
            delete_dns_run,
            mikrotik::backup::mikrotik_backup
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
