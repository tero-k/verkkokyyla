use std::sync::Arc;

use crate::scan::{
    InterfaceDto, LoadedScanDto, ScanError, ScanEvent, ScanManager, ScanStatusEvent,
    ScanSummaryDto, StartScanDto, StoppedScanDto,
};

#[tauri::command]
pub fn list_interfaces() -> Result<Vec<InterfaceDto>, ScanError> {
    Ok(crate::scan::interfaces::list_interfaces()?)
}

#[tauri::command]
pub async fn start_scan(
    manager: tauri::State<'_, ScanManager>,
    interface_name: String,
    cidr: String,
    tcp_fallback: bool,
    ports_enabled: Option<bool>,
    on_event: tauri::ipc::Channel<ScanEvent>,
    on_status: tauri::ipc::Channel<ScanStatusEvent>,
) -> Result<StartScanDto, ScanError> {
    manager
        .start(
            crate::scan::manager::StartScanRequest {
                interface_name,
                cidr,
                tcp_fallback,
                ports_enabled: ports_enabled.unwrap_or(false),
            },
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
pub async fn stop_scan(
    manager: tauri::State<'_, ScanManager>,
) -> Result<StoppedScanDto, ScanError> {
    manager.stop().await
}

#[tauri::command]
pub async fn list_scans(
    manager: tauri::State<'_, ScanManager>,
) -> Result<Vec<ScanSummaryDto>, ScanError> {
    manager.list_scans().await
}

#[tauri::command]
pub async fn load_scan(
    manager: tauri::State<'_, ScanManager>,
    id: i64,
) -> Result<LoadedScanDto, ScanError> {
    manager.load_scan(id).await
}

#[tauri::command]
pub async fn delete_scan(manager: tauri::State<'_, ScanManager>, id: i64) -> Result<(), ScanError> {
    manager.delete_scan(id).await
}

#[tauri::command]
pub fn export_scan_csv(path: String, contents: String) -> Result<(), ScanError> {
    std::fs::write(&path, contents)
        .map_err(|err| ScanError::Io(format!("could not write CSV export to {path}: {err}")))
}
