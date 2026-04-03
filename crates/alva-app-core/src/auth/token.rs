//! Token persistence.
//!
//! Stores OAuth tokens on disk at `~/.alva/auth.json` so that the user does
//! not need to re-authenticate on every CLI invocation.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Errors ──────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum TokenError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("no token stored")]
    NotFound,
}

// ── StoredToken ─────────────────────────────────────────────────────

/// A token persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredToken {
    pub access_token: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
    /// When the token expires (UTC). `None` means no known expiry.
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl StoredToken {
    /// Whether the token has expired (with a 30-second safety margin).
    pub fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(exp) => Utc::now() >= exp - chrono::Duration::seconds(30),
            None => false,
        }
    }
}

// ── TokenStore ──────────────────────────────────────────────────────

/// Handles reading / writing [`StoredToken`] to a JSON file on disk.
pub struct TokenStore {
    path: PathBuf,
}

impl TokenStore {
    /// Create a store using the default path (`~/.alva/auth.json`).
    pub fn new() -> Self {
        let dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".alva");
        Self {
            path: dir.join("auth.json"),
        }
    }

    /// Create a store at a custom path (useful for tests).
    pub fn with_path(path: PathBuf) -> Self {
        Self { path }
    }

    /// The path where tokens are stored.
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Load the stored token from disk.
    pub async fn load(&self) -> Result<StoredToken, TokenError> {
        let path = self.path.clone();
        let data = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    TokenError::NotFound
                } else {
                    TokenError::Io(e)
                }
            })?;
        let token: StoredToken = serde_json::from_str(&data)?;
        Ok(token)
    }

    /// Save a token to disk, creating parent directories as needed.
    pub async fn save(&self, token: &StoredToken) -> Result<(), TokenError> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let data = serde_json::to_string_pretty(token)?;
        tokio::fs::write(&self.path, data.as_bytes()).await?;
        Ok(())
    }

    /// Delete the stored token file.
    pub async fn delete(&self) -> Result<(), TokenError> {
        match tokio::fs::remove_file(&self.path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(TokenError::Io(e)),
        }
    }
}

impl Default for TokenStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_not_expired_without_expiry() {
        let t = StoredToken {
            access_token: "tok".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
        };
        assert!(!t.is_expired());
    }

    #[test]
    fn token_expired_in_past() {
        let t = StoredToken {
            access_token: "tok".into(),
            refresh_token: None,
            expires_at: Some(Utc::now() - chrono::Duration::hours(1)),
            scopes: vec![],
        };
        assert!(t.is_expired());
    }

    #[test]
    fn token_not_expired_in_future() {
        let t = StoredToken {
            access_token: "tok".into(),
            refresh_token: None,
            expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
            scopes: vec![],
        };
        assert!(!t.is_expired());
    }

    #[tokio::test]
    async fn roundtrip_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::with_path(dir.path().join("auth.json"));

        let token = StoredToken {
            access_token: "abc123".into(),
            refresh_token: Some("refresh".into()),
            expires_at: None,
            scopes: vec!["read".into()],
        };

        store.save(&token).await.unwrap();
        let loaded = store.load().await.unwrap();
        assert_eq!(loaded.access_token, "abc123");
        assert_eq!(loaded.refresh_token.as_deref(), Some("refresh"));
    }

    #[tokio::test]
    async fn delete_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::with_path(dir.path().join("auth.json"));

        let token = StoredToken {
            access_token: "x".into(),
            refresh_token: None,
            expires_at: None,
            scopes: vec![],
        };

        store.save(&token).await.unwrap();
        store.delete().await.unwrap();

        match store.load().await {
            Err(TokenError::NotFound) => {}
            other => panic!("expected NotFound, got {:?}", other),
        }
    }
}
