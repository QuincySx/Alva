// INPUT:  std::sync, alva_types
// OUTPUT: model
// POS:    Resolves a "provider/model_id" spec string into a LanguageModel via ProviderRegistry.
use std::sync::Arc;
use alva_types::{LanguageModel, ProviderRegistry, ProviderError};

/// Parse a `provider/model_id` string and resolve from registry.
///
/// # Examples
/// ```rust,no_run
/// # use alva_types::ProviderRegistry;
/// # let registry = ProviderRegistry::new();
/// let llm = alva_runtime::model("anthropic/claude-sonnet-4-20250514", &registry);
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
