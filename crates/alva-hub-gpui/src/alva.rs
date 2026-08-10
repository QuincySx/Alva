// INPUT:  Alva/Claude/Codex engine adapters, alva_engine_runtime, alva_llm_provider, permissions
// OUTPUT: build_engine() for the user-selected Alva, Claude, or Codex runtime
// POS:    Hub Desktop engine assembly; adapters stay behind EngineRuntime and
//         are always configured from the session permission profile.

use std::path::Path;
use std::sync::Arc;

use alva_engine_adapter_alva::{AlvaAdapter, CatalogBaseAgentFactory};
use alva_engine_adapter_claude::{ClaudePrintAdapter, ClaudePrintConfig};
use alva_engine_adapter_codex::{CodexAdapter, CodexAdapterConfig};
use alva_engine_runtime::EngineRuntime;

use crate::permissions::{enforcement_for, EngineEnforcement, PermissionProfile};

/// Assemble an engine that enforces `profile` equivalently or stricter.
/// Fails closed when the engine can only weaken the session's permission
/// contract — continuity never overrides it.
pub fn build_engine(
    engine_id: &str,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
) -> Result<Arc<dyn EngineRuntime>, String> {
    let plan = enforcement_for(engine_id, profile)?;
    match (engine_id, plan.enforcement) {
        ("alva", EngineEnforcement::InteractiveCallbacks) => {
            build_alva_engine(workspace, history_dir)
        }
        ("claude", EngineEnforcement::ClaudePrintMode(permission_mode)) => {
            Ok(Arc::new(ClaudePrintAdapter::new(ClaudePrintConfig {
                default_cwd: Some(workspace.to_path_buf()),
                permission_mode,
                ..ClaudePrintConfig::default()
            })))
        }
        ("codex", EngineEnforcement::CodexSandbox(sandbox)) => {
            Ok(Arc::new(CodexAdapter::new(CodexAdapterConfig {
                default_cwd: Some(workspace.to_path_buf()),
                sandbox,
                ..CodexAdapterConfig::default()
            })))
        }
        (other, enforcement) => Err(format!(
            "Hub permission mapping for engine '{other}' returned an enforcement \
             it cannot apply: {enforcement:?}"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_engine_fails_closed_for_weaken_only_combinations() {
        let workspace = tempfile::tempdir().unwrap();
        let history = tempfile::tempdir().unwrap();
        for engine_id in ["claude", "codex"] {
            let error = match build_engine(
                engine_id,
                PermissionProfile::Interactive,
                workspace.path(),
                history.path(),
            ) {
                Err(error) => error,
                Ok(_) => panic!("weaken-only combination must not assemble an engine"),
            };
            assert!(
                error.contains("cannot preserve or tighten"),
                "{engine_id}: {error}"
            );
        }
    }

    #[test]
    fn eligible_cli_engines_assemble_with_profile_mapping() {
        let workspace = tempfile::tempdir().unwrap();
        let history = tempfile::tempdir().unwrap();
        for engine_id in ["claude", "codex"] {
            for profile in [
                PermissionProfile::WorkspaceWrite,
                PermissionProfile::Unrestricted,
            ] {
                build_engine(engine_id, profile, workspace.path(), history.path()).unwrap_or_else(
                    |error| panic!("{engine_id}/{} must assemble: {error}", profile.as_str()),
                );
            }
        }
    }
}
