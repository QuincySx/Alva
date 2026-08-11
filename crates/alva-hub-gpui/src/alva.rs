// INPUT:  Alva/Claude/Codex/Pi engine adapters, alva_engine_runtime, alva_llm_provider, permissions
// OUTPUT: build_engine() for the user-selected Alva, Claude, Codex, or Pi runtime
// POS:    Hub Desktop engine assembly; adapters stay behind EngineRuntime and
//         are always configured from the session permission profile.

use std::path::Path;
use std::sync::Arc;

use alva_engine_adapter_alva::{AlvaAdapter, CatalogBaseAgentFactory, SessionRepository};
use alva_engine_adapter_claude::{ClaudePrintAdapter, ClaudePrintConfig};
use alva_engine_adapter_codex::{CodexAdapter, CodexAdapterConfig};
use alva_engine_adapter_pi::{
    PiAdapter, PiAdapterConfig, PiApiProtocol, PiProviderConfig, PiToolPolicy,
};
use alva_engine_runtime::{EngineInspector, EngineRuntime};

use crate::models::{ApiProtocol, ProviderRuntimeConfig};
use crate::permissions::{enforcement_for, EngineEnforcement, PermissionProfile};
use crate::store::EngineTarget;

pub struct EngineBuild {
    pub runtime: Arc<dyn EngineRuntime>,
    pub inspector: Option<Arc<dyn EngineInspector>>,
}

/// Assemble an engine that enforces `profile` equivalently or stricter.
/// Fails closed when the engine can only weaken the session's permission
/// contract — continuity never overrides it.
pub fn build_engine(
    engine_id: &str,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
) -> Result<Arc<dyn EngineRuntime>, String> {
    build_engine_bundle(engine_id, profile, workspace, history_dir).map(|built| built.runtime)
}

pub fn build_engine_bundle(
    engine_id: &str,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
) -> Result<EngineBuild, String> {
    let target = EngineTarget::for_engine(engine_id)?;
    build_engine_bundle_for_target(&target, profile, workspace, history_dir)
}

pub fn build_engine_bundle_for_target(
    target: &EngineTarget,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
) -> Result<EngineBuild, String> {
    build_engine_bundle_for_target_with_provider(target, profile, workspace, history_dir, None)
}

pub fn build_engine_bundle_for_target_with_provider(
    target: &EngineTarget,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
    provider: Option<&ProviderRuntimeConfig>,
) -> Result<EngineBuild, String> {
    let engine_id = target.engine_id.as_str();
    let plan = enforcement_for(engine_id, profile)?;
    match (engine_id, plan.enforcement) {
        ("alva", EngineEnforcement::InteractiveCallbacks) => {
            build_alva_engine(target, workspace, history_dir, provider)
        }
        ("claude", EngineEnforcement::ClaudePrintMode(permission_mode)) => {
            let mut config = ClaudePrintConfig {
                default_cwd: Some(workspace.to_path_buf()),
                permission_mode,
                model: model_override(target),
                effort: target
                    .reasoning_effort
                    .as_deref()
                    .and_then(runtime_effort)
                    .map(ToString::to_string),
                ..ClaudePrintConfig::default()
            };
            if let Some(provider) = provider {
                if provider.protocol != ApiProtocol::AnthropicMessages {
                    return Err("Claude Code custom providers require Anthropic Messages".into());
                }
                config
                    .env
                    .insert("ANTHROPIC_BASE_URL".into(), provider.base_url.clone());
                config
                    .env
                    .insert("ANTHROPIC_API_KEY".into(), provider.api_key.clone());
            }
            Ok(EngineBuild {
                runtime: Arc::new(ClaudePrintAdapter::new(config)),
                inspector: None,
            })
        }
        ("codex", EngineEnforcement::CodexSandbox(sandbox)) => {
            let mut config = CodexAdapterConfig {
                default_cwd: Some(workspace.to_path_buf()),
                sandbox,
                model: model_override(target),
                ..CodexAdapterConfig::default()
            };
            if let Some(effort) = target.reasoning_effort.as_deref().and_then(runtime_effort) {
                config
                    .config_overrides
                    .push(("model_reasoning_effort".into(), format!("\"{effort}\"")));
            }
            if let Some(provider) = provider {
                if provider.protocol != ApiProtocol::OpenAiResponses {
                    return Err("Codex custom providers require OpenAI Responses".into());
                }
                config
                    .env
                    .insert("ALVA_HUB_PROVIDER_API_KEY".into(), provider.api_key.clone());
                config.config_overrides.extend([
                    ("model_provider".into(), "\"alva_hub\"".into()),
                    (
                        "model_providers.alva_hub.name".into(),
                        "\"ALVA Hub provider\"".into(),
                    ),
                    (
                        "model_providers.alva_hub.base_url".into(),
                        format!("\"{}\"", provider.base_url),
                    ),
                    (
                        "model_providers.alva_hub.env_key".into(),
                        "\"ALVA_HUB_PROVIDER_API_KEY\"".into(),
                    ),
                    (
                        "model_providers.alva_hub.wire_api".into(),
                        "\"responses\"".into(),
                    ),
                ]);
            }
            Ok(EngineBuild {
                runtime: Arc::new(CodexAdapter::new(config)),
                inspector: None,
            })
        }
        ("pi", EngineEnforcement::PiReadOnly) | ("pi", EngineEnforcement::PiUnrestricted) => {
            let custom_provider = provider
                .map(|provider| pi_provider_config(target, provider))
                .transpose()?;
            let config = PiAdapterConfig {
                default_cwd: Some(workspace.to_path_buf()),
                session_dir: history_dir.join("pi-sessions"),
                provider: Some(target.provider_id.clone()),
                model: model_override(target),
                thinking: target
                    .reasoning_effort
                    .as_deref()
                    .and_then(runtime_effort)
                    .map(ToString::to_string),
                tool_policy: if plan.enforcement == EngineEnforcement::PiUnrestricted {
                    PiToolPolicy::Unrestricted
                } else {
                    PiToolPolicy::ReadOnly
                },
                custom_provider,
                ..PiAdapterConfig::default()
            };
            Ok(EngineBuild {
                runtime: Arc::new(PiAdapter::new(config)),
                inspector: None,
            })
        }
        (other, enforcement) => Err(format!(
            "Hub permission mapping for engine '{other}' returned an enforcement \
             it cannot apply: {enforcement:?}"
        )),
    }
}

fn pi_provider_config(
    target: &EngineTarget,
    provider: &ProviderRuntimeConfig,
) -> Result<PiProviderConfig, String> {
    let protocol = match provider.protocol {
        ApiProtocol::AnthropicMessages => PiApiProtocol::AnthropicMessages,
        ApiProtocol::OpenAiChat => PiApiProtocol::OpenAiCompletions,
        ApiProtocol::OpenAiResponses => PiApiProtocol::OpenAiResponses,
        ApiProtocol::Gemini => PiApiProtocol::GoogleGenerativeAi,
        ApiProtocol::NativeSubscription => {
            return Err("Pi custom providers cannot use a native subscription protocol".into())
        }
    };
    Ok(PiProviderConfig {
        provider_id: target.provider_id.clone(),
        display_name: target.account_source.clone(),
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        protocol,
        model_id: target.model_id.clone(),
        model_name: target.model_id.clone(),
        context_window: 200_000,
        max_output_tokens: 32_000,
        images: target
            .required_capabilities
            .iter()
            .any(|capability| capability == "images"),
        reasoning: target.reasoning_effort.as_deref() != Some("off"),
    })
}

fn model_override(target: &EngineTarget) -> Option<String> {
    (target.model_id != "engine-default" && target.model_id != "workspace-config")
        .then(|| target.model_id.clone())
}

fn runtime_effort(effort: &str) -> Option<&str> {
    match effort {
        "off" => None,
        "extra_high" => Some("xhigh"),
        other => Some(other),
    }
}

fn build_alva_engine(
    target: &EngineTarget,
    workspace: &Path,
    history_dir: &Path,
    provider: Option<&ProviderRuntimeConfig>,
) -> Result<EngineBuild, String> {
    let mut config = if let Some(provider) = provider {
        alva_llm_provider::ProviderConfig {
            api_key: provider.api_key.clone(),
            model: target.model_id.clone(),
            base_url: provider.base_url.clone(),
            max_tokens: 32_000,
            custom_headers: Default::default(),
            kind: Some(
                match provider.protocol {
                    ApiProtocol::AnthropicMessages => "anthropic",
                    ApiProtocol::OpenAiChat => "openai-chat",
                    ApiProtocol::OpenAiResponses => "openai-responses",
                    ApiProtocol::Gemini => "gemini",
                    ApiProtocol::NativeSubscription => {
                        return Err("ALVA cannot use a native subscription provider".into());
                    }
                }
                .into(),
            ),
        }
    } else {
        alva_llm_provider::ProviderConfig::load(workspace)?
    };
    if config.api_key.is_empty() {
        return Err("no Alva provider API key configured".into());
    }
    if let Some(model) = model_override(target) {
        config.model = model;
    }

    let provider_kind = config.kind.clone();
    let provider_registry = alva_llm_provider::build_provider_registry(&config);
    let model = alva_llm_provider::build_language_model(provider_kind.as_deref(), config);
    let factory = CatalogBaseAgentFactory::new(model, workspace)
        .with_provider_registry(provider_registry)
        .with_reasoning_effort(
            target
                .reasoning_effort
                .as_deref()
                .and_then(runtime_effort)
                .and_then(alva_kernel_abi::ReasoningEffort::parse),
        );

    let repository = SessionRepository::open(history_dir.join("alva-agent-records.db"))?;
    let adapter = Arc::new(AlvaAdapter::new(Arc::new(factory)).with_session_repository(repository));
    Ok(EngineBuild {
        runtime: adapter.clone(),
        inspector: Some(adapter),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_engine_fails_closed_for_weaken_only_combinations() {
        let workspace = tempfile::tempdir().unwrap();
        let history = tempfile::tempdir().unwrap();
        for engine_id in ["claude", "codex", "pi"] {
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
        for engine_id in ["claude", "codex", "pi"] {
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

    #[test]
    fn target_model_and_effort_are_forwarded_to_cli_adapters() {
        let workspace = tempfile::tempdir().unwrap();
        let history = tempfile::tempdir().unwrap();
        for engine_id in ["claude", "codex", "pi"] {
            let mut target = EngineTarget::for_engine(engine_id).unwrap();
            target.model_id = "custom-model".into();
            target.reasoning_effort = Some("extra_high".into());
            build_engine_bundle_for_target(
                &target,
                PermissionProfile::WorkspaceWrite,
                workspace.path(),
                history.path(),
            )
            .unwrap();
        }
        assert_eq!(runtime_effort("extra_high"), Some("xhigh"));
        assert_eq!(runtime_effort("off"), None);
    }
}
