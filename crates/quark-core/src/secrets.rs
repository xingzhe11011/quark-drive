use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use serde_json::json;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("secret store io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("secret store json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("secret not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, SecretError>;

/// A small plaintext-at-rest store backed by a 0600 file.
///
/// This keeps the app usable on headless machines where Secret Service is
/// unavailable. A production build should prefer the OS keyring.
pub struct FileSecretStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileSecretStore {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            lock: Mutex::new(()),
        }
    }

    pub async fn set(&self, key: &str, value: &str) -> Result<()> {
        let _guard = self.lock.lock().await;
        let mut map = self.read_map()?;
        map.insert(key.to_string(), value.to_string());
        self.write_map(&map)
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let _guard = self.lock.lock().await;
        Ok(self.read_map()?.get(key).cloned())
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        let _guard = self.lock.lock().await;
        let mut map = self.read_map()?;
        map.remove(key);
        self.write_map(&map)
    }

    pub async fn require(&self, key: &str) -> Result<String> {
        self.get(key)
            .await?
            .ok_or_else(|| SecretError::NotFound(key.to_string()))
    }

    fn read_map(&self) -> Result<BTreeMap<String, String>> {
        if !self.path.exists() {
            return Ok(BTreeMap::new());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }
        let bytes = std::fs::read(&self.path)?;
        if bytes.is_empty() {
            return Ok(BTreeMap::new());
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn write_map(&self, map: &BTreeMap<String, String>) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("tmp");
        {
            let mut options = OpenOptions::new();
            options.write(true).create(true).truncate(true).mode(0o600);
            let mut file = options.open(&tmp)?;
            let body = serde_json::to_vec_pretty(&json!(map))?;
            file.write_all(&body)?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp, &self.path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }
}
