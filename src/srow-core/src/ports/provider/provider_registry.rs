// INPUT:  super::errors, super model traits
// OUTPUT: Provider (trait)
// POS:    Provider registry trait — factory for obtaining model instances by ID.
use super::errors::ProviderError;
use super::language_model::LanguageModel;
use super::embedding_model::EmbeddingModel;
use super::image_model::ImageModel;
use super::speech_model::SpeechModel;
use super::transcription_model::TranscriptionModel;
use super::reranking_model::RerankingModel;
use super::video_model::VideoModel;

/// A provider registry that can create model instances by ID.
///
/// Language and embedding models are required; other model types
/// default to returning `UnsupportedFunctionality`.
pub trait Provider: Send + Sync {
    /// The specification version this provider implements.
    fn specification_version(&self) -> &str {
        "v4"
    }

    /// Create a language model instance for the given model ID.
    fn language_model(&self, model_id: &str) -> Result<Box<dyn LanguageModel>, ProviderError>;

    /// Create an embedding model instance for the given model ID.
    fn embedding_model(&self, model_id: &str) -> Result<Box<dyn EmbeddingModel>, ProviderError>;

    /// Create an image generation model instance for the given model ID.
    fn image_model(&self, _model_id: &str) -> Result<Box<dyn ImageModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "image model".into(),
        ))
    }

    /// Create a speech synthesis model instance for the given model ID.
    fn speech_model(&self, _model_id: &str) -> Result<Box<dyn SpeechModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "speech model".into(),
        ))
    }

    /// Create a transcription model instance for the given model ID.
    fn transcription_model(
        &self,
        _model_id: &str,
    ) -> Result<Box<dyn TranscriptionModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "transcription model".into(),
        ))
    }

    /// Create a reranking model instance for the given model ID.
    fn reranking_model(
        &self,
        _model_id: &str,
    ) -> Result<Box<dyn RerankingModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "reranking model".into(),
        ))
    }

    /// Create a video generation model instance for the given model ID.
    fn video_model(&self, _model_id: &str) -> Result<Box<dyn VideoModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "video model".into(),
        ))
    }
}
