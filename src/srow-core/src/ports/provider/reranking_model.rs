// INPUT:  async_trait, super::types, super::errors
// OUTPUT: RerankingModel (trait), RerankingCallOptions, RerankingResult, RankedDocument
// POS:    Reranking model trait and associated types for Provider V4.
use async_trait::async_trait;
use super::types::*;
use super::errors::ProviderError;

/// Abstract reranking model interface (Provider V4 specification).
#[async_trait]
pub trait RerankingModel: Send + Sync {
    /// The specification version this model implements.
    fn specification_version(&self) -> &str {
        "v4"
    }

    /// The provider identifier.
    fn provider(&self) -> &str;

    /// The model identifier (e.g. "rerank-2").
    fn model_id(&self) -> &str;

    /// Rerank documents by relevance to a query.
    async fn do_rerank(
        &self,
        options: RerankingCallOptions,
    ) -> Result<RerankingResult, ProviderError>;
}

/// Options for a reranking call.
pub struct RerankingCallOptions {
    pub query: String,
    pub documents: Vec<String>,
    pub top_n: Option<u32>,
    pub provider_options: Option<ProviderOptions>,
    pub headers: Option<ProviderHeaders>,
}

/// The result of a reranking call.
pub struct RerankingResult {
    pub results: Vec<RankedDocument>,
    pub provider_metadata: Option<ProviderMetadata>,
    pub warnings: Vec<ProviderWarning>,
}

/// A document with its relevance score after reranking.
pub struct RankedDocument {
    /// Index of this document in the original input list.
    pub index: usize,
    /// Relevance score (higher is more relevant).
    pub relevance_score: f64,
    /// The document text.
    pub document: String,
}
