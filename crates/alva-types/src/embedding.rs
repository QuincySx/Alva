// INPUT:  async_trait, serde, crate::error::AgentError
// OUTPUT: pub trait EmbeddingModel, pub struct EmbeddingResult, pub struct EmbeddingUsage
// POS:    Trait and result types for text embedding models that map text to vector representations.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AgentError;

/// Interface for text embedding models.
///
/// Maps text to vectors (points in n-dimensional space). Similar texts
/// produce vectors that are close together. Pass one text for a query
/// embedding, or many for document embeddings.
#[async_trait]
pub trait EmbeddingModel: Send + Sync {
    fn model_id(&self) -> &str;
    fn dimensions(&self) -> Option<usize>;
    fn max_embeddings_per_call(&self) -> Option<usize>;

    async fn embed(&self, texts: &[&str]) -> Result<EmbeddingResult, AgentError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingResult {
    pub embeddings: Vec<Vec<f32>>,
    pub usage: Option<EmbeddingUsage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingUsage {
    pub tokens: u32,
}
