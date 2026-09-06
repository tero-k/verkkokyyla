use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::db::{
    Database, DbError, DownloadSpeedSessionSummary, NewDownloadSpeedSession,
};

/// Errors for download speed history operations.
#[derive(Debug, thiserror::Error)]
pub enum DownloadSpeedHistoryError {
    #[error("session not found: {0}")]
    SessionNotFound(i64),
    #[error("database error: {0}")]
    Db(#[from] DbError),
    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Request payload for saving a completed download speed run.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveDownloadSpeedSessionRequest {
    pub url: String,
    pub mode: String,
    pub http_settings_json: String,
    pub result_json: String,
    pub average_mbps: f64,
    pub total_time_ms: i64,
}

/// Wire mirror of [`DownloadSpeedSessionSummary`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadSpeedSessionSummaryDto {
    pub id: i64,
    pub url: String,
    pub mode: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub status: String,
    pub average_mbps: f64,
    pub total_time_ms: i64,
}

impl From<DownloadSpeedSessionSummary> for DownloadSpeedSessionSummaryDto {
    fn from(row: DownloadSpeedSessionSummary) -> Self {
        Self {
            id: row.id,
            url: row.url,
            mode: row.mode,
            started_at: row.started_at,
            ended_at: row.ended_at,
            status: row.status,
            average_mbps: row.average_mbps,
            total_time_ms: row.total_time_ms,
        }
    }
}

/// Loaded session with the result as a JSON string.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedDownloadSpeedSessionDto {
    pub session: DownloadSpeedSessionSummaryDto,
    pub result_json: String,
}

/// Persists and retrieves download speed test history.
pub struct DownloadSpeedManager {
    db: Arc<Database>,
}

impl DownloadSpeedManager {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn save_session(
        &self,
        request: SaveDownloadSpeedSessionRequest,
    ) -> Result<DownloadSpeedSessionSummaryDto, DownloadSpeedHistoryError> {
        let started_at = crate::db::now_rfc3339();
        let id = self
            .db
            .create_download_speed_session(&NewDownloadSpeedSession {
                url: request.url,
                mode: request.mode,
                http_settings_json: request.http_settings_json,
                started_at,
            })
            .await?;
        self.db
            .finish_download_speed_session(
                id,
                &crate::db::now_rfc3339(),
                "completed",
                request.average_mbps,
                request.total_time_ms,
                &request.result_json,
            )
            .await?;
        self.load_session_summary(id).await
    }

    pub async fn list_sessions(
        &self,
    ) -> Result<Vec<DownloadSpeedSessionSummaryDto>, DownloadSpeedHistoryError> {
        let rows = self.db.list_download_speed_sessions().await?;
        Ok(rows.into_iter().map(DownloadSpeedSessionSummaryDto::from).collect())
    }

    pub async fn load_session(
        &self,
        id: i64,
    ) -> Result<LoadedDownloadSpeedSessionDto, DownloadSpeedHistoryError> {
        let loaded = self.db.load_download_speed_session(id).await?;
        Ok(LoadedDownloadSpeedSessionDto {
            session: DownloadSpeedSessionSummaryDto::from(loaded.session),
            result_json: loaded.result_json,
        })
    }

    pub async fn delete_session(&self, id: i64) -> Result<(), DownloadSpeedHistoryError> {
        self.db.delete_download_speed_session(id).await?;
        Ok(())
    }

    async fn load_session_summary(
        &self,
        id: i64,
    ) -> Result<DownloadSpeedSessionSummaryDto, DownloadSpeedHistoryError> {
        let sessions = self.db.list_download_speed_sessions().await?;
        sessions
            .into_iter()
            .find(|s| s.id == id)
            .map(DownloadSpeedSessionSummaryDto::from)
            .ok_or(DownloadSpeedHistoryError::SessionNotFound(id))
    }
}

impl Serialize for DownloadSpeedHistoryError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("DownloadSpeedHistoryError", 2)?;
        state.serialize_field(
            "kind",
            match self {
                Self::SessionNotFound(_) => "session-not-found",
                Self::Db(_) => "db",
                Self::Serialize(_) => "serialize",
            },
        )?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}
