//! OS keyring credential store for device profile secrets (MikroTik
//! passwords, OPNsense API key/secret pairs, ...).
//!
//! Secrets are stored in the operating system's keyring, keyed by the
//! profile's `secret_key` column value (a random namespaced string generated
//! at profile creation) — never by the SQLite rowid, which SQLite reuses
//! after deleting the max id and could attach a stale secret to a new
//! profile. A missing entry yields [`SecretError::NotStored`] so the UI can
//! prompt "credentials not stored — re-enter".
//!
//! The `keyring` crate performs blocking platform I/O, so every call is
//! offloaded via [`tokio::task::spawn_blocking`] (same offload concern as
//! the std::process spawning in `engine::trace_os`). Plaintext secrets are
//! never persisted to logs or any other store.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use thiserror::Error;

/// Keyring service name under which all device profile secrets live.
pub const KEYRING_SERVICE: &str = "com.verkkokyyla.app";

/// Typed errors produced by a [`SecretStore`] implementation.
#[derive(Debug, Error)]
pub enum SecretError {
    /// No secret is stored for the given secret key. The UI can prompt
    /// the user to re-enter the credentials.
    #[error("credentials not stored — re-enter")]
    NotStored,

    /// The OS keyring backend failed or is unavailable; the message carries
    /// the platform error text (never a secret).
    #[error("keyring error: {0}")]
    Keyring(String),
}

impl From<keyring::Error> for SecretError {
    fn from(err: keyring::Error) -> Self {
        match err {
            keyring::Error::NoEntry => SecretError::NotStored,
            other => SecretError::Keyring(other.to_string()),
        }
    }
}

/// Async store for device profile secrets, keyed by `secret_key`.
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Retrieve the secret stored under `secret_key`.
    async fn get(&self, secret_key: &str) -> Result<String, SecretError>;

    /// Store `secret` under `secret_key`, replacing any existing entry.
    async fn set(&self, secret_key: &str, secret: &str) -> Result<(), SecretError>;

    /// Delete the entry under `secret_key`.
    async fn delete(&self, secret_key: &str) -> Result<(), SecretError>;
}

/// [`SecretStore`] backed by the operating system's keyring
/// (service `com.verkkokyyla.app`, account = the profile's `secret_key`).
pub struct KeyringStore;

impl KeyringStore {
    pub fn new() -> Self {
        KeyringStore
    }

    /// Run blocking keyring I/O off the async executor thread.
    async fn run<T, F>(&self, f: F) -> Result<T, SecretError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, SecretError> + Send + 'static,
    {
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|err| SecretError::Keyring(format!("keyring task failed: {err}")))?
    }
}

#[async_trait]
impl SecretStore for KeyringStore {
    async fn get(&self, secret_key: &str) -> Result<String, SecretError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, secret_key).map_err(SecretError::from)?;
        self.run(move || entry.get_password().map_err(SecretError::from))
            .await
    }

    async fn set(&self, secret_key: &str, secret: &str) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, secret_key).map_err(SecretError::from)?;
        let secret = secret.to_owned();
        self.run(move || entry.set_password(&secret).map_err(SecretError::from))
            .await
    }

    async fn delete(&self, secret_key: &str) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, secret_key).map_err(SecretError::from)?;
        self.run(move || entry.delete_credential().map_err(SecretError::from))
            .await
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

/// In-memory [`SecretStore`] for tests and headless CI environments where no
/// OS keyring/desktop session exists.
#[derive(Clone, Default)]
pub struct MemoryStore {
    inner: Arc<Mutex<HashMap<String, String>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        MemoryStore::default()
    }
}

#[async_trait]
impl SecretStore for MemoryStore {
    async fn get(&self, secret_key: &str) -> Result<String, SecretError> {
        let inner = self
            .inner
            .lock()
            .map_err(|err| SecretError::Keyring(format!("memory store lock poisoned: {err}")))?;
        inner.get(secret_key).cloned().ok_or(SecretError::NotStored)
    }

    async fn set(&self, secret_key: &str, secret: &str) -> Result<(), SecretError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|err| SecretError::Keyring(format!("memory store lock poisoned: {err}")))?;
        inner.insert(secret_key.to_owned(), secret.to_owned());
        Ok(())
    }

    async fn delete(&self, secret_key: &str) -> Result<(), SecretError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|err| SecretError::Keyring(format!("memory store lock poisoned: {err}")))?;
        match inner.remove(secret_key) {
            Some(_) => Ok(()),
            None => Err(SecretError::NotStored),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn secrets_memory_roundtrip() {
        let store = MemoryStore::new();

        store.set("key-a", "hunter2").await.unwrap();
        assert_eq!(store.get("key-a").await.unwrap(), "hunter2");

        // set replaces an existing entry for the same secret key
        store.set("key-a", "hunter3").await.unwrap();
        assert_eq!(store.get("key-a").await.unwrap(), "hunter3");

        // other keys are unaffected
        assert!(matches!(
            store.get("key-b").await,
            Err(SecretError::NotStored)
        ));

        store.delete("key-a").await.unwrap();
        assert!(matches!(
            store.get("key-a").await,
            Err(SecretError::NotStored)
        ));
    }

    #[tokio::test]
    async fn secrets_memory_get_unknown_key_is_not_stored() {
        let store = MemoryStore::new();
        let err = store.get("never-stored").await.unwrap_err();
        assert!(matches!(err, SecretError::NotStored));
    }

    #[tokio::test]
    async fn secrets_memory_delete_unknown_key_is_not_stored() {
        let store = MemoryStore::new();
        let err = store.delete("never-stored").await.unwrap_err();
        assert!(matches!(err, SecretError::NotStored));
    }

    /// Live keyring test — requires a desktop session with an unlocked OS
    /// keyring. Excluded from CI/headless runs via #[ignore]; run manually:
    /// `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored keyring`
    #[tokio::test]
    #[ignore = "requires a desktop session with an unlocked OS keyring"]
    async fn secrets_keyring_live_roundtrip() {
        let store = KeyringStore::new();
        let key = format!("test-{}-live", std::process::id());

        store.set(&key, "live-password").await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), "live-password");
        store.delete(&key).await.unwrap();
        assert!(matches!(store.get(&key).await, Err(SecretError::NotStored)));
    }
}
