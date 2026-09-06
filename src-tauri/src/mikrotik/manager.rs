//! Session manager for MikroTik monitoring: a single active session at a
//! time (second start → typed `AlreadyRunning`), a watch<bool> cancel
//! channel, and a spawned runtime task whose exit clears the active slot
//! (mirrors `trace/manager.rs`).

use std::sync::Arc;

use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;

use super::runtime::{run_mikrotik_session, MikrotikRunContext};
use super::types::{
    CreateMikrotikProfileRequest, DeleteProfileResultDto, LoadedMikrotikSessionDto,
    MikrotikApiFactory, MikrotikManagerError, MikrotikProfileDto, MikrotikSessionSummaryDto,
    MikrotikSnapshotDto, MikrotikStartDto, MikrotikStatusEvent, MikrotikStatusSink,
    MikrotikStoppedDto, MikrotikTestConnectionDto, UpdateMikrotikProfileRequest,
};
use super::secrets::SecretStore;
use crate::db::{
    now_rfc3339, Database, MikrotikProfile, MikrotikSessionSummary, NewMikrotikProfile,
    NewMikrotikSession,
};
use crate::mikrotik::client::MikrotikConnection;
use crate::mikrotik::error::MikrotikError;

#[derive(Clone)]
pub struct MikrotikManager {
    db: Arc<Database>,
    store: Arc<dyn SecretStore>,
    factory: MikrotikApiFactory,
    inner: Arc<Mutex<MikrotikInner>>,
}

#[derive(Default)]
struct MikrotikInner {
    active: Option<ActiveSession>,
}

struct ActiveSession {
    session_id: i64,
    profile_id: i64,
    stop_tx: watch::Sender<bool>,
    join_handle: JoinHandle<Result<MikrotikStoppedDto, MikrotikManagerError>>,
}

impl MikrotikManager {
    pub fn new(db: Database, store: Arc<dyn SecretStore>, factory: MikrotikApiFactory) -> Self {
        Self {
            db: Arc::new(db),
            store,
            factory,
            inner: Arc::new(Mutex::new(MikrotikInner { active: None })),
        }
    }

    /// The profile backing the currently active session, if any.
    pub async fn active_profile_id(&self) -> Option<i64> {
        self.inner.lock().await.active.as_ref().map(|a| a.profile_id)
    }

    async fn require_profile(&self, id: i64) -> Result<MikrotikProfile, MikrotikManagerError> {
        self.db
            .load_mikrotik_profile(id)
            .await?
            .ok_or(MikrotikManagerError::ProfileNotFound(id))
    }

    async fn connection_for(
        &self,
        profile: &MikrotikProfile,
    ) -> Result<MikrotikConnection, MikrotikManagerError> {
        let password = self.store.get(&profile.secret_key).await?;
        Ok(MikrotikConnection {
            host: profile.host.clone(),
            port: u16::try_from(profile.port)
                .map_err(|_| MikrotikManagerError::Api(MikrotikError::Connect(format!(
                    "invalid port {}",
                    profile.port
                ))))?,
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
        if self.active_profile_id().await == Some(id) {
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
        let has_password = matches!(self.store.get(&profile.secret_key).await, Ok(_));
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
        let mut inner = self.inner.lock().await;
        if inner.active.is_some() {
            return Err(MikrotikManagerError::AlreadyRunning);
        }
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
                run_version_probe: None,
            })
            .await
        });
        inner.active = Some(ActiveSession {
            session_id,
            profile_id,
            stop_tx,
            join_handle,
        });
        Ok(MikrotikStartDto {
            session_id,
            profile_id,
        })
    }

    pub async fn stop(&self) -> Result<MikrotikStoppedDto, MikrotikManagerError> {
        let active = {
            self.inner
                .lock()
                .await
                .active
                .take()
                .ok_or(MikrotikManagerError::NoActiveSession)?
        };
        let _ = active.stop_tx.send(true);
        active
            .join_handle
            .await
            .map_err(|err| MikrotikManagerError::Api(MikrotikError::Connect(format!(
                "mikrotik task panicked: {err}"
            ))))?
    }

    pub async fn list_sessions(&self) -> Result<Vec<MikrotikSessionSummaryDto>, MikrotikManagerError> {
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
        if inner
            .active
            .as_ref()
            .is_some_and(|active| active.session_id == session_id)
        {
            inner.active.take();
        }
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
