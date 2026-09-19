//! Session manager for MikroTik monitoring: MULTIPLE concurrent sessions,
//! one per profile (second start of the same profile → typed
//! `AlreadyRunningForProfile`), a global cap of [`MAX_CONCURRENT_SESSIONS`],
//! per-session watch<bool> cancel channels, and spawned runtime tasks whose
//! exit clears their own slot (mirrors `trace/manager.rs` per-session shape).

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;

use super::runtime::{run_mikrotik_session, MikrotikRunContext};
use super::secrets::SecretStore;
use super::types::{
    ActiveSessionDto, CreateMikrotikProfileRequest, DeleteProfileResultDto,
    LoadedMikrotikSessionDto, MikrotikApiFactory, MikrotikManagerError, MikrotikProfileDto,
    MikrotikSessionSummaryDto, MikrotikSnapshotDto, MikrotikStartDto, MikrotikStatusEvent,
    MikrotikStatusSink, MikrotikStoppedDto, MikrotikTestConnectionDto,
    UpdateMikrotikProfileRequest,
};
use super::version::{self, ActiveVersionTarget, VersionFirmwareResultDto};
use crate::db::{
    now_rfc3339, Database, MikrotikProfile, MikrotikSessionSummary, NewMikrotikProfile,
    NewMikrotikSession,
};
use crate::mikrotik::client::MikrotikConnection;
use crate::mikrotik::error::MikrotikError;

/// Upper bound on concurrent monitoring sessions. Each session polls the
/// router every 5s, so unbounded growth would eventually hurt both this app
/// and the managed devices.
pub const MAX_CONCURRENT_SESSIONS: usize = 8;

#[derive(Clone)]
pub struct MikrotikManager {
    db: Arc<Database>,
    store: Arc<dyn SecretStore>,
    factory: MikrotikApiFactory,
    inner: Arc<Mutex<MikrotikInner>>,
}

/// Live sessions keyed by `profile_id`: one running session per device.
#[derive(Default)]
struct MikrotikInner {
    active: HashMap<i64, ActiveSession>,
}

struct ActiveSession {
    session_id: i64,
    profile_id: i64,
    on_status: MikrotikStatusSink,
    stop_tx: watch::Sender<bool>,
    join_handle: JoinHandle<Result<MikrotikStoppedDto, MikrotikManagerError>>,
}

impl MikrotikManager {
    pub fn new(db: Database, store: Arc<dyn SecretStore>, factory: MikrotikApiFactory) -> Self {
        Self {
            db: Arc::new(db),
            store,
            factory,
            inner: Arc::new(Mutex::new(MikrotikInner::default())),
        }
    }

    /// Whether a live session currently backs this profile.
    pub async fn is_profile_active(&self, profile_id: i64) -> bool {
        self.inner.lock().await.active.contains_key(&profile_id)
    }

    /// The API factory — used by sibling managers (e.g. the log stream
    /// manager) that build their own per-task API handle from a profile
    /// connection.
    pub(crate) fn api_factory(&self) -> MikrotikApiFactory {
        Arc::clone(&self.factory)
    }

    pub(crate) async fn require_profile(
        &self,
        id: i64,
    ) -> Result<MikrotikProfile, MikrotikManagerError> {
        self.db
            .load_mikrotik_profile(id)
            .await?
            .ok_or(MikrotikManagerError::ProfileNotFound(id))
    }

    pub(crate) async fn connection_for(
        &self,
        profile: &MikrotikProfile,
    ) -> Result<MikrotikConnection, MikrotikManagerError> {
        let password = self.store.get(&profile.secret_key).await?;
        Ok(MikrotikConnection {
            host: profile.host.clone(),
            port: u16::try_from(profile.port).map_err(|_| {
                MikrotikManagerError::Api(MikrotikError::Connect(format!(
                    "invalid port {}",
                    profile.port
                )))
            })?,
            use_tls: profile.use_tls,
            allow_invalid_certs: profile.allow_invalid_certs,
            username: profile.username.clone(),
            password,
        })
    }
    // ---- Profiles ------------------------------------------------------------

    pub async fn list_profiles(&self) -> Result<Vec<MikrotikProfileDto>, MikrotikManagerError> {
        let mut dtos = Vec::new();
        for profile in self.db.list_mikrotik_profiles().await? {
            dtos.push(self.profile_dto(profile).await);
        }
        Ok(dtos)
    }

    pub async fn create_profile(
        &self,
        request: &CreateMikrotikProfileRequest,
    ) -> Result<MikrotikProfileDto, MikrotikManagerError> {
        let profile = self
            .db
            .create_mikrotik_profile(&NewMikrotikProfile {
                name: request.name.clone(),
                host: request.host.clone(),
                port: request.port,
                use_tls: request.use_tls,
                allow_invalid_certs: request.allow_invalid_certs,
                username: request.username.clone(),
                created_at: now_rfc3339(),
            })
            .await?;
        Ok(self.profile_dto(profile).await)
    }

    pub async fn update_profile(
        &self,
        request: &UpdateMikrotikProfileRequest,
    ) -> Result<MikrotikProfileDto, MikrotikManagerError> {
        let existing = self.require_profile(request.id).await?;
        self.db
            .update_mikrotik_profile(
                request.id,
                &NewMikrotikProfile {
                    name: request.name.clone(),
                    host: request.host.clone(),
                    port: request.port,
                    use_tls: request.use_tls,
                    allow_invalid_certs: request.allow_invalid_certs,
                    username: request.username.clone(),
                    created_at: existing.created_at.clone(),
                },
            )
            .await?;
        // `secret_key` is immutable across updates — the stored password keeps
        // working unchanged.
        let profile = self.require_profile(request.id).await?;
        Ok(self.profile_dto(profile).await)
    }

    /// Delete is refused with a typed `ProfileInUse` error while the profile
    /// backs the ACTIVE session. Otherwise the DB row goes FIRST and the
    /// keyring delete is best-effort (a failure never blocks the delete).
    pub async fn delete_profile(
        &self,
        id: i64,
    ) -> Result<DeleteProfileResultDto, MikrotikManagerError> {
        if self.is_profile_active(id).await {
            return Err(MikrotikManagerError::ProfileInUse(id));
        }
        let profile = self.require_profile(id).await?;
        self.db.delete_mikrotik_profile(id).await?;
        match self.store.delete(&profile.secret_key).await {
            Ok(()) => Ok(DeleteProfileResultDto {
                deleted: true,
                secret_deleted: true,
                warning: None,
            }),
            Err(err) => Ok(DeleteProfileResultDto {
                deleted: true,
                secret_deleted: false,
                warning: Some(format!("profile deleted; keyring cleanup failed: {err}")),
            }),
        }
    }

    pub async fn set_profile_password(
        &self,
        id: i64,
        password: &str,
    ) -> Result<(), MikrotikManagerError> {
        let profile = self.require_profile(id).await?;
        self.store.set(&profile.secret_key, password).await?;
        Ok(())
    }

    /// Probe `/rest/system/resource` and return board/version.
    pub async fn test_connection(
        &self,
        id: i64,
    ) -> Result<MikrotikTestConnectionDto, MikrotikManagerError> {
        let profile = self.require_profile(id).await?;
        let conn = self.connection_for(&profile).await?;
        let api = (self.factory)(conn).await?;
        let resource = api.get_resource().await?;
        Ok(MikrotikTestConnectionDto {
            board_name: resource.board_name,
            routeros_version: resource.version,
            architecture_name: resource.architecture_name,
        })
    }

    async fn profile_dto(&self, profile: MikrotikProfile) -> MikrotikProfileDto {
        let has_password = self.store.get(&profile.secret_key).await.is_ok();
        MikrotikProfileDto {
            id: profile.id,
            name: profile.name,
            host: profile.host,
            port: profile.port,
            use_tls: profile.use_tls,
            allow_invalid_certs: profile.allow_invalid_certs,
            username: profile.username,
            has_password,
            created_at: profile.created_at,
        }
    }
    // ---- Sessions ------------------------------------------------------------

    pub async fn start<E>(
        &self,
        profile_id: i64,
        on_event: E,
        on_status: MikrotikStatusSink,
    ) -> Result<MikrotikStartDto, MikrotikManagerError>
    where
        E: Fn(crate::mikrotik::types::MikrotikEvent) + Send + Sync + 'static,
    {
        {
            let inner = self.inner.lock().await;
            if inner.active.contains_key(&profile_id) {
                return Err(MikrotikManagerError::AlreadyRunningForProfile(profile_id));
            }
            if inner.active.len() >= MAX_CONCURRENT_SESSIONS {
                return Err(MikrotikManagerError::TooManySessions);
            }
        }
        // The lifecycle lock is NOT held across the awaits below: a slow
        // connect must not block stop/list for unrelated sessions.
        let profile = self.require_profile(profile_id).await?;
        let conn = self.connection_for(&profile).await?;
        let api = match (self.factory)(conn).await {
            Ok(api) => api,
            Err(err) => {
                on_status(MikrotikStatusEvent::Error {
                    session_id: 0,
                    message: format!("{err:?}"),
                });
                return Err(err.into());
            }
        };

        // Re-validate under the lock: another start may have claimed the
        // profile (or the last slot) while we were connecting.
        let mut inner = self.inner.lock().await;
        if inner.active.contains_key(&profile_id) {
            return Err(MikrotikManagerError::AlreadyRunningForProfile(profile_id));
        }
        if inner.active.len() >= MAX_CONCURRENT_SESSIONS {
            return Err(MikrotikManagerError::TooManySessions);
        }
        let session_id = self
            .db
            .create_mikrotik_session(&NewMikrotikSession {
                profile_id,
                started_at: now_rfc3339(),
                status: "running".to_owned(),
            })
            .await?;
        let (stop_tx, stop_rx) = watch::channel(false);
        let manager = self.clone();
        let db = Arc::clone(&self.db);
        let active_status = Arc::clone(&on_status);
        let on_event: Arc<dyn Fn(crate::mikrotik::types::MikrotikEvent) + Send + Sync> =
            Arc::new(on_event);
        let join_handle = tokio::spawn(async move {
            run_mikrotik_session(MikrotikRunContext {
                manager,
                db,
                api,
                session_id,
                profile_id,
                on_event,
                on_status,
                stop_rx,
                run_version_probe: Some(version::run_version_probe()),
            })
            .await
        });
        inner.active.insert(
            profile_id,
            ActiveSession {
                session_id,
                profile_id,
                on_status: active_status,
                stop_tx,
                join_handle,
            },
        );
        Ok(MikrotikStartDto {
            session_id,
            profile_id,
        })
    }

    /// Cancel ONE session by id and wait for its task. The entry is removed
    /// under the lock before awaiting the join handle, so concurrent stops
    /// of other sessions never block on this one's shutdown.
    pub async fn stop(&self, session_id: i64) -> Result<MikrotikStoppedDto, MikrotikManagerError> {
        let active = {
            let mut inner = self.inner.lock().await;
            let key = inner
                .active
                .iter()
                .find(|(_, a)| a.session_id == session_id)
                .map(|(k, _)| *k);
            match key {
                Some(key) => inner
                    .active
                    .remove(&key)
                    .expect("key just found in active map"),
                None => return Err(MikrotikManagerError::NoActiveSession),
            }
        };
        let _ = active.stop_tx.send(true);
        active.join_handle.await.map_err(|err| {
            MikrotikManagerError::Api(MikrotikError::Connect(format!(
                "mikrotik task panicked: {err}"
            )))
        })?
    }

    /// The currently live sessions, for rehydrating the UI after a reload.
    pub async fn list_active(&self) -> Vec<ActiveSessionDto> {
        self.inner
            .lock()
            .await
            .active
            .values()
            .map(|a| ActiveSessionDto {
                session_id: a.session_id,
                profile_id: a.profile_id,
            })
            .collect()
    }

    pub async fn list_sessions(
        &self,
    ) -> Result<Vec<MikrotikSessionSummaryDto>, MikrotikManagerError> {
        Ok(self
            .db
            .list_mikrotik_sessions()
            .await?
            .into_iter()
            .map(session_summary_dto)
            .collect())
    }

    pub async fn load_session(
        &self,
        id: i64,
    ) -> Result<LoadedMikrotikSessionDto, MikrotikManagerError> {
        let loaded = self.db.load_mikrotik_session(id).await?;
        if loaded.session.started_at.is_empty() {
            return Err(MikrotikManagerError::SessionNotFound(id));
        }
        Ok(LoadedMikrotikSessionDto {
            session: session_summary_dto(loaded.session),
            snapshots: loaded
                .snapshots
                .into_iter()
                .map(|row| MikrotikSnapshotDto {
                    id: row.id,
                    session_id: row.session_id,
                    at: row.at,
                    cpu_load: row.cpu_load,
                    mem_used_bytes: row.mem_used_bytes,
                    mem_total_bytes: row.mem_total_bytes,
                    uptime: row.uptime,
                    warning: row.warning,
                    sensors_json: row.sensors_json,
                    interfaces_json: row.interfaces_json,
                    vlans_json: row.vlans_json,
                    bridge_vlans_json: row.bridge_vlans_json,
                })
                .collect(),
        })
    }

    pub async fn delete_session(&self, id: i64) -> Result<(), MikrotikManagerError> {
        let loaded = self.db.load_mikrotik_session(id).await?;
        if loaded.session.started_at.is_empty() {
            return Err(MikrotikManagerError::SessionNotFound(id));
        }
        self.db.delete_mikrotik_session(id).await?;
        Ok(())
    }

    pub(crate) async fn clear_active(&self, session_id: i64) {
        let mut inner = self.inner.lock().await;
        inner.active.retain(|_, a| a.session_id != session_id);
    }

    pub(crate) async fn active_version_target(
        &self,
        profile_id: i64,
    ) -> Option<ActiveVersionTarget> {
        self.inner
            .lock()
            .await
            .active
            .get(&profile_id)
            .map(|active| ActiveVersionTarget {
                session_id: active.session_id,
                on_status: Arc::clone(&active.on_status),
            })
    }

    pub(crate) async fn is_active_session(&self, session_id: i64) -> bool {
        self.inner
            .lock()
            .await
            .active
            .values()
            .any(|active| active.session_id == session_id)
    }

    pub async fn check_updates(
        &self,
        profile_id: i64,
    ) -> Result<VersionFirmwareResultDto, MikrotikManagerError> {
        let target = self.active_version_target(profile_id).await;
        let profile = self.require_profile(profile_id).await?;
        let conn = self.connection_for(&profile).await?;
        let api = (self.factory)(conn).await?;
        Ok(version::manual_check(self.clone(), Arc::clone(&self.db), api, target).await?)
    }
}

fn session_summary_dto(row: MikrotikSessionSummary) -> MikrotikSessionSummaryDto {
    MikrotikSessionSummaryDto {
        id: row.id,
        profile_id: row.profile_id,
        started_at: row.started_at,
        ended_at: row.ended_at,
        status: row.status,
        board_name: row.board_name,
        routeros_version: row.routeros_version,
        architecture_name: row.architecture_name,
        update_status_json: row.update_status_json,
        firmware_status_json: row.firmware_status_json,
        snapshot_count: row.snapshot_count,
    }
}
