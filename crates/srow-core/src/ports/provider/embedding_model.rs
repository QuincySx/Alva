// INPUT:  async_trait, super::types, super::errors
// OUTPUT: EmbeddingModel (trait), EmbeddingCallOptions, EmbeddingResult, EmbeddingUsage
// POS:    Embedding model trait and associated types for Provider V4.
use async_trait::async_trait;
use super::types::*;
use super::errors::ProviderError;

/// Abstract embedding model interface (Provider V4 specification).
#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    /// The specification version this model implements.
    fn specification_version(&self) -> &str {
        "v4"
    }

    /// The provider identifier (e.g. "openai", "anthropic").
    fn provider(&self) -> &str;

    /// The model identifier (e.g. "text-embedding-3-small").
    fn model_id(&self) -> &str;

    /// Maximum number of values that can be embedded in a single call.
    fn max_embeddings_per_call(&self) -> Option<u32>;

    /// Whether the model supports parallel embedding calls.
    fn supports_parallel_calls(&self) -> bool;

    /// Generate embeddings for the given values.
    async fn do_embed(
        &self,
        options: EmbeddingCallOptions,
    ) -> Result<EmbeddingResult, ProviderError>;
}

/// Options for an embedding model call.
pub struct EmbeddingCallOptions {
    pub values: Vec<String>,
    pub provider_options: Option<ProviderOptions>,
    pub headers: Option<ProviderHeaders>,
}

/// The result of an embedding model call.
pub struct EmbeddingResult {
    pub embeddings: Vec<Vec<f32>>,
    pub usage: Option<EmbeddingUsage>,
    pub provider_metadata: Option<ProviderMetadata>,
    pub warnings: Vec<ProviderWarning>,
}

/// Token usage for an embedding call.
pub struct EmbeddingUsage {
    pub tokens: u32,
}
