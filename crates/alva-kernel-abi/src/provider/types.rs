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

/// Bus Capability: central registry of all configured LLM providers.
///
/// **Provider**: `ProviderRegistryExtension::configure`
/// (`alva-app-core/src/extension/provider_registry.rs`). Fully opt-in —
/// no built-in default, no builder setter.
/// **Consumers**: `AgentSpawnTool` — when a `SpawnInput.model` carries a
/// `"provider/id"` override, the tool resolves it through this registry
/// to obtain an `Arc<dyn LanguageModel>` for the child agent.
/// **Why bus**: the spawn tool lives in `alva-app-core` but the
/// registry contents are assembled by the outer app (CLI / UI) from
/// user config. No static wiring connects them; the bus carries the
/// registry across that boundary, and its absence is a valid state
/// (children just inherit the parent's model).
///
/// Supports lookup by provider ID and a convenience method for
/// `provider_id:model_id` shorthand strings.
#[crate::bus_cap]
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

#[cfg(test)]
mod tests {
    //! Tests for ProviderRegistry CRUD + the shorthand methods' error
    //! formatting. A misformatted error message ("provider/model"
    //! instead of "provider:model") confuses users trying to find the
    //! right config entry; a register that doesn't replace on conflict
    //! silently leaves stale handlers wired.
    use super::*;

    /// Minimal Provider impl for registry tests. Only `id()` and
    /// `language_model()` are required by the trait; we deliberately
    /// reject all language_model lookups so we can prove the registry
    /// itself errors BEFORE calling the provider when the provider
    /// isn't registered (i.e. the failure path goes through the
    /// registry's own NoSuchModel branch).
    struct StubProvider {
        id: &'static str,
    }
    impl Provider for StubProvider {
        fn id(&self) -> &str {
            self.id
        }
        fn language_model(
            &self,
            _model_id: &str,
        ) -> Result<Arc<dyn LanguageModel>, ProviderError> {
            Err(ProviderError::UnsupportedFunctionality(
                "stub".to_string(),
            ))
        }
    }

    fn stub(id: &'static str) -> Arc<dyn Provider> {
        Arc::new(StubProvider { id })
    }

    // -- CRUD --------------------------------------------------------------

    #[test]
    fn new_is_empty() {
        let r = ProviderRegistry::new();
        assert!(r.get("anything").is_none());
        assert!(r.provider_ids().is_empty());
    }

    #[test]
    fn default_equals_new() {
        let r: ProviderRegistry = Default::default();
        assert!(r.get("anything").is_none());
    }

    #[test]
    fn register_then_get_returns_the_same_provider_handle() {
        let mut r = ProviderRegistry::new();
        r.register(stub("openai"));
        let p = r.get("openai").expect("registered provider must be retrievable");
        assert_eq!(p.id(), "openai");
    }

    #[test]
    fn register_with_existing_id_replaces_doc_contract() {
        // Pinned doc comment: "Replaces any existing provider with the
        // same ID." Without this, a re-register would keep the stale
        // first provider — silent wiring bug.
        let mut r = ProviderRegistry::new();
        r.register(stub("p"));
        // Re-register with a different concrete instance but same id.
        let new_instance = stub("p");
        r.register(new_instance.clone());
        let got = r.get("p").expect("provider still exists after re-register");
        // Verify the stored Arc is the SAME pointer as the latest
        // registration — proves replacement, not duplicate-keep.
        assert!(
            Arc::ptr_eq(got, &new_instance),
            "second register must replace the first, not coexist"
        );
    }

    #[test]
    fn get_unknown_returns_none() {
        let mut r = ProviderRegistry::new();
        r.register(stub("openai"));
        assert!(r.get("anthropic").is_none());
    }

    #[test]
    fn provider_ids_lists_all_registered() {
        let mut r = ProviderRegistry::new();
        r.register(stub("openai"));
        r.register(stub("anthropic"));
        let mut ids: Vec<&str> = r.provider_ids();
        ids.sort();
        assert_eq!(ids, vec!["anthropic", "openai"]);
    }

    // -- Shorthand error formatting ---------------------------------------
    //
    // Each shorthand builds `NoSuchModel { model_id: "p:m", model_type: "..." }`
    // when the provider isn't registered. Pin the ":" separator and
    // the per-method model_type label.

    #[test]
    fn language_model_unknown_provider_uses_colon_format_and_language_type() {
        let r = ProviderRegistry::new();
        // Avoid `.unwrap_err()` — `Arc<dyn LanguageModel>` doesn't impl
        // Debug, so we destructure the Result manually.
        match r.language_model("oai", "gpt-4o") {
            Err(ProviderError::NoSuchModel { model_id, model_type }) => {
                assert_eq!(model_id, "oai:gpt-4o", "must use ':' separator");
                assert_eq!(model_type, "language");
            }
            Err(other) => panic!("expected NoSuchModel, got {other:?}"),
            Ok(_) => panic!("expected Err on unregistered provider"),
        }
    }

    #[test]
    fn embedding_model_unknown_provider_labels_embedding_type() {
        let r = ProviderRegistry::new();
        match r.embedding_model("oai", "ada") {
            Err(ProviderError::NoSuchModel { model_id, model_type }) => {
                assert_eq!(model_id, "oai:ada");
                assert_eq!(model_type, "embedding");
            }
            Err(other) => panic!("expected NoSuchModel, got {other:?}"),
            Ok(_) => panic!("expected Err"),
        }
    }

    #[test]
    fn image_model_unknown_provider_labels_image_type() {
        let r = ProviderRegistry::new();
        match r.image_model("oai", "dalle") {
            Err(ProviderError::NoSuchModel { model_type, .. }) => {
                assert_eq!(model_type, "image");
            }
            Err(other) => panic!("expected NoSuchModel, got {other:?}"),
            Ok(_) => panic!("expected Err"),
        }
    }

    // -- ProviderError Display --------------------------------------------

    #[test]
    fn no_such_model_display_includes_both_type_and_id() {
        // Pin user-facing error message format. Drop either field
        // and the user can't tell what went wrong.
        let e = ProviderError::NoSuchModel {
            model_id: "oai:gpt-4o".to_string(),
            model_type: "language".to_string(),
        };
        let s = format!("{e}");
        assert!(s.contains("language"));
        assert!(s.contains("oai:gpt-4o"));
    }
}
