// INPUT:  Alva/Claude/Codex engine adapters, alva_engine_runtime, alva_llm_provider
// OUTPUT: build_engine() for the user-selected Alva, Claude, or Codex runtime
// POS:    Hub Desktop engine assembly; adapters stay behind EngineRuntime.

use std::path::Path;
use std::sync::Arc;

use alva_engine_adapter_alva::{AlvaAdapter, CatalogBaseAgentFactory};
use alva_engine_adapter_claude::{ClaudePrintAdapter, ClaudePrintConfig};
use alva_engine_adapter_codex::{CodexAdapter, CodexAdapterConfig};
use alva_engine_runtime::EngineRuntime;

pub fn build_engine(
    engine_id: &str,
    workspace: &Path,
    history_dir: &Path,
) -> Result<Arc<dyn EngineRuntime>, String> {
    match engine_id {
        "alva" => build_alva_engine(workspace, history_dir),
        "claude" => Ok(Arc::new(ClaudePrintAdapter::new(ClaudePrintConfig {
            default_cwd: Some(workspace.to_path_buf()),
            ..ClaudePrintConfig::default()
        }))),
        "codex" => Ok(Arc::new(CodexAdapter::new(CodexAdapterConfig {
            default_cwd: Some(workspace.to_path_buf()),
            ..CodexAdapterConfig::default()
        }))),
        other => Err(format!(
            "unsupported Hub engine '{other}'; expected alva, claude, or codex"
        )),
    }
}

fn build_alva_engine(
    workspace: &Path,
    history_dir: &Path,
) -> Result<Arc<dyn EngineRuntime>, String> {
    let config = alva_llm_provider::ProviderConfig::load(workspace)?;
    if config.api_key.is_empty() {
        return Err("no Alva provider API key configured".into());
    }

    let provider_kind = config.kind.clone();
    let provider_registry = alva_llm_provider::build_provider_registry(&config);
    let model = alva_llm_provider::build_language_model(provider_kind.as_deref(), config);
    let factory =
        CatalogBaseAgentFactory::new(model, workspace).with_provider_registry(provider_registry);

    Ok(Arc::new(
        AlvaAdapter::new(Arc::new(factory)).with_history_dir(history_dir),
    ))
}
