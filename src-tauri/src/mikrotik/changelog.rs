use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use thiserror::Error;
use tokio::sync::Mutex;

const CHANGELOG_TIMEOUT: Duration = Duration::from_secs(10);

#[async_trait::async_trait]
pub trait ChangelogFetcher: Send + Sync {
    async fn fetch_changelog(&self, version: &str) -> Result<String, ChangelogError>;
}

#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ChangelogError {
    #[error("changelog not found for RouterOS {0}")]
    ChangelogNotFound(String),
    #[error("changelog timeout: {0}")]
    Timeout(String),
    #[error("changelog fetch failed: {0}")]
    Fetch(String),
}

impl ChangelogError {
    fn kind(&self) -> &'static str {
        match self {
            Self::ChangelogNotFound(_) => "changelog-not-found",
            Self::Timeout(_) => "timeout",
            Self::Fetch(_) => "fetch",
        }
    }
}

impl Serialize for ChangelogError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("ChangelogError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("message", &self.to_string())?;
        state.end()
    }
}

#[derive(Clone)]
pub struct CachedChangelogFetcher {
    client: reqwest::Client,
    base_url: String,
    cache: Arc<Mutex<HashMap<String, String>>>,
}

impl CachedChangelogFetcher {
    pub fn new() -> Result<Self, ChangelogError> {
        let client = reqwest::Client::builder()
            .timeout(CHANGELOG_TIMEOUT)
            .build()
            .map_err(|err| ChangelogError::Fetch(err.to_string()))?;
        Ok(Self {
            client,
            base_url: "https://download.mikrotik.com/routeros".to_owned(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn new_for_base_url(base_url: String) -> Result<Self, ChangelogError> {
        let mut fetcher = Self::new()?;
        fetcher.base_url = base_url;
        Ok(fetcher)
    }

    async fn fetch_uncached(&self, version: &str) -> Result<String, ChangelogError> {
        let url = format!("{}/{version}/CHANGELOG", self.base_url.trim_end_matches('/'));
        let response = self.client.get(url).send().await.map_err(|err| {
            if err.is_timeout() {
                ChangelogError::Timeout(err.to_string())
            } else {
                ChangelogError::Fetch(err.to_string())
            }
        })?;
        match response.status().as_u16() {
            200..=299 => response
                .text()
                .await
                .map_err(|err| ChangelogError::Fetch(err.to_string())),
            404 => Err(ChangelogError::ChangelogNotFound(version.to_owned())),
            status => Err(ChangelogError::Fetch(format!("HTTP {status}"))),
        }
    }
}

#[async_trait::async_trait]
impl ChangelogFetcher for CachedChangelogFetcher {
    async fn fetch_changelog(&self, version: &str) -> Result<String, ChangelogError> {
        if let Some(text) = self.cache.lock().await.get(version).cloned() {
            return Ok(text);
        }
        let text = self.fetch_uncached(version).await?;
        self.cache
            .lock()
            .await
            .insert(version.to_owned(), text.clone());
        Ok(text)
    }
}

#[derive(Clone)]
pub struct ChangelogService {
    fetcher: Arc<dyn ChangelogFetcher>,
}

impl ChangelogService {
    pub fn new(fetcher: Arc<dyn ChangelogFetcher>) -> Self {
        Self { fetcher }
    }

    pub async fn fetch(&self, version: &str) -> Result<ChangelogDto, ChangelogError> {
        Ok(ChangelogDto {
            version: version.to_owned(),
            changelog: self.fetcher.fetch_changelog(version).await?,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangelogDto {
    pub version: String,
    pub changelog: String,
}

#[tauri::command]
pub async fn mikrotik_changelog(
    changelogs: tauri::State<'_, ChangelogService>,
    version: String,
) -> Result<ChangelogDto, ChangelogError> {
    changelogs.fetch(&version).await
}
