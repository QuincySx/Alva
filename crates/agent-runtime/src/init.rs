use std::sync::Arc;
use agent_types::{LanguageModel, ProviderRegistry, ProviderError};

/// Parse a `provider/model_id` string and resolve from registry.
///
/// # Examples
/// ```rust,no_run
/// # use agent_types::ProviderRegistry;
/// # let registry = ProviderRegistry::new();
/// let llm = agent_runtime::model("anthropic/claude-sonnet-4-20250514", &registry);
/// ```
pub fn model(
    spec: &str,
    registry: &ProviderRegistry,
) -> Result<Arc<dyn LanguageModel>, ProviderError> {
    let (provider_id, model_id) = spec.split_once('/').ok_or_else(|| {
        ProviderError::InvalidArgument {
            argument: "spec".to_string(),
            message: format!("expected 'provider/model_id' format, got '{}'", spec),
        }
    })?;
    registry.language_model(provider_id, model_id)
}
