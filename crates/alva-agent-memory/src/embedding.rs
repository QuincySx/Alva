// INPUT:  async_trait, crate::error
// OUTPUT: EmbeddingProvider (trait), NoopEmbeddingProvider
// POS:    Embedding provider trait for vector search and a no-op placeholder implementation.
//! Embedding provider trait + placeholder implementation.
//!
//! Production usage should plug in an OpenAI-compatible `/embeddings` endpoint.

use async_trait::async_trait;

use crate::error::MemoryError;

/// Trait for embedding providers (OpenAI-compatible API).
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Return the model identifier (e.g. "text-embedding-3-small").
    fn model(&self) -> &str;

    /// Compute embeddings for a batch of texts.
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryError>;
}

// ---------------------------------------------------------------------------
// Placeholder (no-op) implementation
// ---------------------------------------------------------------------------

/// A no-op embedding provider that returns empty vectors.
/// Useful for development, testing, and FTS-only configurations.
pub struct NoopEmbeddingProvider;

impl NoopEmbeddingProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NoopEmbeddingProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EmbeddingProvider for NoopEmbeddingProvider {
    fn model(&self) -> &str {
        "noop"
    }

    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, MemoryError> {
        // Return an empty vector for each input text.
        Ok(texts.iter().map(|_| Vec::new()).collect())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_noop_provider() {
        let provider = NoopEmbeddingProvider::new();
        assert_eq!(provider.model(), "noop");

        let results = provider
            .embed(&["hello".into(), "world".into()])
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].is_empty());
        assert!(results[1].is_empty());
    }
}
