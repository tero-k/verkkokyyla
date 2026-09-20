use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use super::error::MikrotikError;
use super::parse::{RouterboardDto, UpdateStatusDto};
use super::types::{
    MikrotikApi, MikrotikManagerError, MikrotikStatusEvent, MikrotikStatusSink, RunVersionProbe,
    VersionProbeArgs,
};
use crate::db::{Database, MikrotikSessionVersionStatus};

const UPDATE_POLL: Duration = Duration::from_secs(2);
const UPDATE_POLL_LIMIT: usize = 15;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatusResultDto {
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub channel: Option<String>,
    pub status: String,
    pub state: UpdateState,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum UpdateState {
    UpdateAvailable,
    UpToDate,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirmwareStatusDto {
    pub state: FirmwareState,
    pub current_firmware: Option<String>,
    pub upgrade_firmware: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FirmwareState {
    Available,
    UpToDate,
    NotApplicable,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionFirmwareResultDto {
    pub update_status: UpdateStatusResultDto,
    pub firmware_status: FirmwareStatusDto,
}

#[derive(Clone)]
pub(crate) struct ActiveVersionTarget {
    pub session_id: i64,
    pub on_status: MikrotikStatusSink,
}

pub fn run_version_probe() -> RunVersionProbe {
    Arc::new(|args| {
        tokio::spawn(async move {
            if let Ok(result) = probe_once(Arc::clone(&args.api)).await {
                if should_deliver_spawned_probe(&result) {
                    let _ = persist_for_active_session(&args, &result).await;
                }
            }
        });
    })
}

pub(crate) async fn manual_check(
    manager: super::manager::MikrotikManager,
    db: Arc<Database>,
    api: Arc<dyn MikrotikApi>,
    target: Option<ActiveVersionTarget>,
) -> Result<VersionFirmwareResultDto, MikrotikError> {
    let result = probe_once(api).await?;
    if let Some(target) = target {
        if manager.is_active_session(target.session_id).await {
            persist_result(&db, target.session_id, &result).await?;
            (target.on_status)(MikrotikStatusEvent::VersionFirmware {
                session_id: target.session_id,
                update_status: result.update_status.clone(),
                firmware_status: result.firmware_status.clone(),
            });
        }
    }
    Ok(result)
}

async fn persist_for_active_session(
    args: &VersionProbeArgs,
    result: &VersionFirmwareResultDto,
) -> Result<(), MikrotikError> {
    if !args.manager.is_active_session(args.session_id).await {
        return Ok(());
    }
    persist_result(&args.db, args.session_id, result).await?;
    (args.on_status)(MikrotikStatusEvent::VersionFirmware {
        session_id: args.session_id,
        update_status: result.update_status.clone(),
        firmware_status: result.firmware_status.clone(),
    });
    Ok(())
}

async fn persist_result(
    db: &Database,
    session_id: i64,
    result: &VersionFirmwareResultDto,
) -> Result<(), MikrotikError> {
    let update_status_json = serde_json::to_string(&result.update_status)
        .map_err(|err| MikrotikError::Parse(format!("serialize update status: {err}")))?;
    let firmware_status_json = serde_json::to_string(&result.firmware_status)
        .map_err(|err| MikrotikError::Parse(format!("serialize firmware status: {err}")))?;
    db.set_mikrotik_session_version_status(
        session_id,
        &MikrotikSessionVersionStatus {
            update_status_json: Some(update_status_json),
            firmware_status_json: Some(firmware_status_json),
        },
    )
    .await
    .map_err(|err| MikrotikError::Connect(err.to_string()))
}

async fn probe_once(api: Arc<dyn MikrotikApi>) -> Result<VersionFirmwareResultDto, MikrotikError> {
    let (update_status, firmware_status) = tokio::join!(probe_update(&api), probe_firmware(&api));
    let update_status = match update_status {
        Ok(status) => status,
        Err(_) => unknown_update(),
    };
    Ok(VersionFirmwareResultDto {
        update_status,
        firmware_status,
    })
}

fn should_deliver_spawned_probe(result: &VersionFirmwareResultDto) -> bool {
    let update_unknown = result.update_status.state == UpdateState::Unknown
        && result.update_status.status == "unknown"
        && result.update_status.installed_version.is_none()
        && result.update_status.latest_version.is_none()
        && result.update_status.channel.is_none();
    !(update_unknown
        && matches!(
            result.firmware_status.state,
            FirmwareState::NotApplicable | FirmwareState::Unknown
        ))
}

async fn probe_update(api: &Arc<dyn MikrotikApi>) -> Result<UpdateStatusResultDto, MikrotikError> {
    let first = api.check_for_updates().await?;
    if is_terminal(first.status.as_deref()) {
        return Ok(update_result(first));
    }
    let mut current = first;
    for _ in 0..UPDATE_POLL_LIMIT {
        tokio::time::sleep(UPDATE_POLL).await;
        current = api.get_update_status().await?;
        if is_terminal(current.status.as_deref()) {
            break;
        }
    }
    Ok(update_result(current))
}

async fn probe_firmware(api: &Arc<dyn MikrotikApi>) -> FirmwareStatusDto {
    match api.get_routerboard().await {
        Ok(dto) => firmware_from_routerboard(dto),
        Err(err) if is_routerboard_not_applicable(&err) => FirmwareStatusDto {
            state: FirmwareState::NotApplicable,
            current_firmware: None,
            upgrade_firmware: None,
            model: None,
        },
        Err(_) => FirmwareStatusDto {
            state: FirmwareState::Unknown,
            current_firmware: None,
            upgrade_firmware: None,
            model: None,
        },
    }
}

fn update_result(dto: UpdateStatusDto) -> UpdateStatusResultDto {
    let state = update_state(dto.latest_version.as_deref(), dto.status.as_deref());
    UpdateStatusResultDto {
        installed_version: dto.installed_version,
        latest_version: dto.latest_version,
        channel: dto.channel,
        status: dto.status.unwrap_or_else(|| "unknown".to_owned()),
        state,
    }
}

fn unknown_update() -> UpdateStatusResultDto {
    UpdateStatusResultDto {
        installed_version: None,
        latest_version: None,
        channel: None,
        status: "unknown".to_owned(),
        state: UpdateState::Unknown,
    }
}

fn update_state(latest_version: Option<&str>, status: Option<&str>) -> UpdateState {
    let Some(status) = status else {
        return UpdateState::Unknown;
    };
    if latest_version.is_none() {
        return UpdateState::Unknown;
    }
    let lower = status.to_lowercase();
    if lower.contains("available") {
        UpdateState::UpdateAvailable
    } else if lower.contains("up to date") {
        UpdateState::UpToDate
    } else {
        UpdateState::Unknown
    }
}

fn firmware_from_routerboard(dto: RouterboardDto) -> FirmwareStatusDto {
    let state = match dto.routerboard {
        Some(true) => match (
            dto.current_firmware.as_deref(),
            dto.upgrade_firmware.as_deref(),
        ) {
            (Some(current), Some(upgrade)) if current == upgrade => FirmwareState::UpToDate,
            (Some(_), Some(_)) => FirmwareState::Available,
            (Some(_), None) | (None, Some(_)) | (None, None) => FirmwareState::Unknown,
        },
        Some(false) | None => FirmwareState::NotApplicable,
    };
    FirmwareStatusDto {
        state,
        current_firmware: dto.current_firmware,
        upgrade_firmware: dto.upgrade_firmware,
        model: dto.model,
    }
}

fn is_routerboard_not_applicable(err: &MikrotikError) -> bool {
    matches!(err, MikrotikError::Api { status: 404, .. }) || err.is_no_such_command()
}

fn is_terminal(status: Option<&str>) -> bool {
    let Some(status) = status else {
        return true;
    };
    let lower = status.to_lowercase();
    !(lower.starts_with("checking")
        || lower.starts_with("downloading")
        || lower.starts_with("installing")
        || lower.starts_with("finding out latest version"))
}

#[tauri::command]
pub async fn mikrotik_check_updates(
    manager: tauri::State<'_, super::manager::MikrotikManager>,
    profile_id: i64,
) -> Result<VersionFirmwareResultDto, MikrotikManagerError> {
    manager.check_updates(profile_id).await
}
