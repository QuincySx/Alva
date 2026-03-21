// INPUT:  thiserror
// OUTPUT: ProviderError
// POS:    Comprehensive error enum for all provider operations, aligned with AI SDK error types.

/// Errors that can occur during provider operations.
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
