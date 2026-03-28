// INPUT:  std::collections::HashMap, std::sync::Arc, thiserror, crate::{EmbeddingModel, ImageModel, LanguageModel, ModerationModel, RerankingModel, SpeechModel, TranscriptionModel, VideoModel}
// OUTPUT: pub enum ProviderError, pub trait Provider, pub struct ProviderRegistry
// POS:    Provider abstraction and registry for obtaining model instances by provider and model ID.
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    EmbeddingModel, ImageModel, LanguageModel, ModerationModel, RerankingModel, SpeechModel,
    TranscriptionModel, VideoModel,
};

// ---------------------------------------------------------------------------
// ProviderError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, thiserror::Error)]
pub enum ProviderError {
    #[error("API call error: {message}")]
    ApiCall {
        message: String,
        url: String,
        status_code: Option<u16>,
        response_body: Option<String>,
        is_retryable: bool,
    },

    #[error("Empty response body")]
    EmptyResponseBody,

    #[error("Invalid argument '{argument}': {message}")]
    InvalidArgument { argument: String, message: String },

    #[error("Invalid prompt: {message}")]
    InvalidPrompt { message: String },

    #[error("Invalid response data: {message}")]
    InvalidResponseData { message: String },

    #[error("JSON parse error: {message}")]
    JsonParse { message: String, text: String },

    #[error("API key error: {message}")]
    LoadApiKey { message: String },

    #[error("Setting error: {message}")]
    LoadSetting { message: String },

    #[error("No content generated")]
    NoContentGenerated,

    #[error("No such {model_type}: {model_id}")]
    NoSuchModel {
        model_id: String,
        model_type: String,
    },

    #[error("Too many embedding values: {count} > {max}")]
    TooManyEmbeddingValues { count: usize, max: usize },

    #[error("Type validation error: {message}")]
    TypeValidation { message: String },

    #[error("Unsupported: {0}")]
    UnsupportedFunctionality(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Rate limited")]
    RateLimited { retry_after_ms: Option<u64> },
}

// ---------------------------------------------------------------------------
// Provider trait
// ---------------------------------------------------------------------------

/// Factory for obtaining model instances by provider+model ID.
///
/// Implementations wrap a specific LLM backend (e.g., OpenAI, Anthropic)
/// and produce `LanguageModel` instances on demand.
pub trait Provider: Send + Sync {
    /// Unique provider identifier (e.g., "openai", "anthropic").
    fn id(&self) -> &str;

    /// Create a language model instance for the given model ID.
    fn language_model(
        &self,
        model_id: &str,
    ) -> Result<Arc<dyn LanguageModel>, ProviderError>;

    /// Create an embedding model instance for the given model ID.
    fn embedding_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn EmbeddingModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "embedding models are not supported by this provider".to_string(),
        ))
    }

    /// Create a transcription model instance for the given model ID.
    fn transcription_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn TranscriptionModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "transcription models are not supported by this provider".to_string(),
        ))
    }

    /// Create a speech model instance for the given model ID.
    fn speech_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn SpeechModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "speech models are not supported by this provider".to_string(),
        ))
    }

    /// Create an image model instance for the given model ID.
    fn image_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn ImageModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "image models are not supported by this provider".to_string(),
        ))
    }

    /// Create a video model instance for the given model ID.
    fn video_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn VideoModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "video models are not supported by this provider".to_string(),
        ))
    }

    /// Create a reranking model instance for the given model ID.
    fn reranking_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn RerankingModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "reranking models are not supported by this provider".to_string(),
        ))
    }

    /// Create a moderation model instance for the given model ID.
    fn moderation_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn ModerationModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "moderation models are not supported by this provider".to_string(),
        ))
    }
}

// ---------------------------------------------------------------------------
// ProviderRegistry
// ---------------------------------------------------------------------------

/// Central registry of all available providers.
///
/// Supports lookup by provider ID and a convenience method for
/// `provider_id:model_id` shorthand strings.
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Register a provider. Replaces any existing provider with the same ID.
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.insert(provider.id().to_string(), provider);
    }

    /// Get a provider by ID.
    pub fn get(&self, provider_id: &str) -> Option<&Arc<dyn Provider>> {
        self.providers.get(provider_id)
    }

    /// Shorthand: obtain a language model from `provider_id:model_id`.
    ///
    /// Returns `ProviderError::NoSuchModel` if the provider is not registered.
    pub fn language_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn LanguageModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "language".to_string(),
            }
        })?;
        provider.language_model(model_id)
    }

    /// Shorthand: obtain an embedding model from `provider_id:model_id`.
    pub fn embedding_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn EmbeddingModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "embedding".to_string(),
            }
        })?;
        provider.embedding_model(model_id)
    }

    /// Shorthand: obtain a transcription model from `provider_id:model_id`.
    pub fn transcription_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn TranscriptionModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "transcription".to_string(),
            }
        })?;
        provider.transcription_model(model_id)
    }

    /// Shorthand: obtain a speech model from `provider_id:model_id`.
    pub fn speech_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn SpeechModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "speech".to_string(),
            }
        })?;
        provider.speech_model(model_id)
    }

    /// Shorthand: obtain an image model from `provider_id:model_id`.
    pub fn image_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn ImageModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "image".to_string(),
            }
        })?;
        provider.image_model(model_id)
    }

    /// Shorthand: obtain a video model from `provider_id:model_id`.
    pub fn video_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn VideoModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "video".to_string(),
            }
        })?;
        provider.video_model(model_id)
    }

    /// Shorthand: obtain a reranking model from `provider_id:model_id`.
    pub fn reranking_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn RerankingModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "reranking".to_string(),
            }
        })?;
        provider.reranking_model(model_id)
    }

    /// Shorthand: obtain a moderation model from `provider_id:model_id`.
    pub fn moderation_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn ModerationModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "moderation".to_string(),
            }
        })?;
        provider.moderation_model(model_id)
    }

    /// List all registered provider IDs.
    pub fn provider_ids(&self) -> Vec<&str> {
        self.providers.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
