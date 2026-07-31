// INPUT:  alva_engine_adapter_alva, alva_engine_runtime, alva_llm_provider
// OUTPUT: build_alva_engine()
// POS:    Builds the ticket-01 Alva EngineRuntime for a selected workspace.

use std::path::Path;
use std::sync::Arc;

use alva_engine_adapter_alva::{AlvaAdapter, CatalogBaseAgentFactory};
use alva_engine_runtime::EngineRuntime;

pub fn build_alva_engine(workspace: &Path) -> Result<Arc<dyn EngineRuntime>, String> {
    let config = alva_llm_provider::ProviderConfig::load(workspace)?;
    if config.api_key.is_empty() {
        return Err("no Alva provider API key configured".into());
    }

    let provider_kind = config.kind.clone();
    let provider_registry = alva_llm_provider::build_provider_registry(&config);
    let model = alva_llm_provider::build_language_model(provider_kind.as_deref(), config);
    let factory =
        CatalogBaseAgentFactory::new(model, workspace).with_provider_registry(provider_registry);

    Ok(Arc::new(AlvaAdapter::new(Arc::new(factory))))
}
