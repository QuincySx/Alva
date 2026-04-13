use async_trait::async_trait;
use crate::provider::ProviderError;

/// Abstraction for obtaining API credentials.
///
/// Implementations can be static (simple key), OAuth (refresh token),
/// or vault-backed (fetch from secret manager).
#[async_trait]
pub trait CredentialSource: Send + Sync {
    /// Get the current API key / bearer token.
    async fn get_api_key(&self) -> Result<String, ProviderError>;
}

/// Static credential — wraps a fixed API key string.
#[derive(Clone)]
pub struct StaticCredential(String);

impl StaticCredential {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }
}

#[async_trait]
impl CredentialSource for StaticCredential {
    async fn get_api_key(&self) -> Result<String, ProviderError> {
        Ok(self.0.clone())
    }
}
