// INPUT:  async_trait, serde, crate::error::AgentError
// OUTPUT: pub trait RerankingModel, pub struct RerankConfig, pub struct RerankResult, pub struct RankEntry
// POS:    Trait and wire types for document reranking models that score query-document relevance.
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AgentError;

/// Interface for reranking models.
///
/// Given a query and a list of documents, returns relevance scores.
/// Caller keeps the original documents slice and uses `index` to look up.
#[async_trait]
pub trait RerankingModel: Send + Sync {
    fn model_id(&self) -> &str;

    async fn rerank(
        &self,
        query: &str,
        documents: &[&str],
        config: &RerankConfig,
    ) -> Result<RerankResult, AgentError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankConfig {
    pub top_n: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankResult {
    pub rankings: Vec<RankEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankEntry {
    /// Index into the original documents slice.
    pub index: usize,
    pub relevance_score: f64,
}
