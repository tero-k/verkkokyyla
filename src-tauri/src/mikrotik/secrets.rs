//! OS keyring credential store for MikroTik profile passwords.
//!
//! Passwords are stored in the operating system's keyring, keyed by the
//! profile's `secret_key` column value (a random 128-bit hex string generated
//! at profile creation) — never by the SQLite rowid, which SQLite reuses
//! after deleting the max id and could attach a stale secret to a new
//! profile. A missing entry yields [`SecretError::NotStored`] so the UI can
//! prompt "password not stored — re-enter".
//!
//! The `keyring` crate performs blocking platform I/O, so every call is
//! offloaded via [`tokio::task::spawn_blocking`] (same offload concern as
//! the std::process spawning in `engine::trace_os`). Plaintext passwords are
//! never persisted to logs or any other store.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use thiserror::Error;

/// Keyring service name under which all MikroTik profile passwords live.
pub const KEYRING_SERVICE: &str = "com.verkkokyyla.app";

/// Typed errors produced by a [`SecretStore`] implementation.
#[derive(Debug, Error)]
pub enum SecretError {
    /// No password is stored for the given secret key. The UI can prompt
    /// the user to re-enter the password.
    #[error("password not stored — re-enter")]
    NotStored,

    /// The OS keyring backend failed or is unavailable; the message carries
    /// the platform error text (never a password).
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

/// Async store for MikroTik profile passwords, keyed by `secret_key`.
#[async_trait]
pub trait SecretStore: Send + Sync {
    /// Retrieve the password stored under `secret_key`.
    async fn get(&self, secret_key: &str) -> Result<String, SecretError>;

    /// Store `password` under `secret_key`, replacing any existing entry.
    async fn set(&self, secret_key: &str, password: &str) -> Result<(), SecretError>;

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

    async fn set(&self, secret_key: &str, password: &str) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(KEYRING_SERVICE, secret_key).map_err(SecretError::from)?;
        let password = password.to_owned();
        self.run(move || entry.set_password(&password).map_err(SecretError::from))
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

    async fn set(&self, secret_key: &str, password: &str) -> Result<(), SecretError> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|err| SecretError::Keyring(format!("memory store lock poisoned: {err}")))?;
        inner.insert(secret_key.to_owned(), password.to_owned());
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
    async fn mikrotik_secrets_memory_roundtrip() {
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
    async fn mikrotik_secrets_memory_get_unknown_key_is_not_stored() {
        let store = MemoryStore::new();
        let err = store.get("never-stored").await.unwrap_err();
        assert!(matches!(err, SecretError::NotStored));
        assert_eq!(err.to_string(), "password not stored — re-enter");
    }

    #[tokio::test]
    async fn mikrotik_secrets_memory_delete_unknown_key_is_not_stored() {
        let store = MemoryStore::new();
        let err = store.delete("never-stored").await.unwrap_err();
        assert!(matches!(err, SecretError::NotStored));
    }

    #[tokio::test]
    async fn mikrotik_secrets_error_keyring_variant_display() {
        let err = SecretError::Keyring("backend down".to_owned());
        assert!(err.to_string().contains("backend down"));
    }

    /// Live keyring test — requires a desktop session with an unlocked OS
    /// keyring. Excluded from CI/headless runs via #[ignore]; run manually:
    /// `cargo test --manifest-path src-tauri/Cargo.toml -- --ignored keyring`
    #[tokio::test]
    #[ignore = "requires a desktop session with an unlocked OS keyring"]
    async fn mikrotik_secrets_keyring_live_roundtrip() {
        let store = KeyringStore::new();
        let key = format!("test-{}-live", std::process::id());

        store.set(&key, "live-password").await.unwrap();
        assert_eq!(store.get(&key).await.unwrap(), "live-password");
        store.delete(&key).await.unwrap();
        assert!(matches!(store.get(&key).await, Err(SecretError::NotStored)));
    }
}
