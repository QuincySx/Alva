// INPUT:  Alva/Claude/Codex/Pi engine adapters, alva_engine_runtime, alva_llm_provider, permissions
// OUTPUT: build_engine(), route-aware assembly, and shared-repository Inspector assembly
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
use alva_engine_runtime::EngineRuntime;
use alva_llm_provider::ProviderConfig;

use crate::inspection::HubInspectorService;
use crate::permissions::{enforcement_for, EngineEnforcement, PermissionProfile};
use crate::store::EngineTarget;

/// Assemble an engine that enforces `profile` equivalently or stricter.
/// Fails closed when the engine can only weaken the session's permission
/// contract — continuity never overrides it.
pub fn build_engine(
    engine_id: &str,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
) -> Result<Arc<dyn EngineRuntime>, String> {
    let target = EngineTarget::for_engine(engine_id)?;
    let provider_config = if engine_id == "alva" {
        Some(ProviderConfig::load(workspace)?)
    } else {
        None
    };
    build_engine_for_target(&target, provider_config, profile, workspace, history_dir)
}

/// Assemble the exact provider/model route selected in the composer.
pub fn build_engine_for_target(
    target: &EngineTarget,
    provider_config: Option<ProviderConfig>,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
) -> Result<Arc<dyn EngineRuntime>, String> {
    build_engine_for_target_with_repository(
        target,
        provider_config,
        profile,
        workspace,
        history_dir,
        None,
    )
}

/// Assemble an engine using the same Alva native repository as Hub Inspector.
/// Non-Alva adapters ignore the service because they truthfully expose no full
/// native inspection capability yet.
pub fn build_engine_for_target_with_inspector(
    target: &EngineTarget,
    provider_config: Option<ProviderConfig>,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
    inspector: &HubInspectorService,
) -> Result<Arc<dyn EngineRuntime>, String> {
    build_engine_for_target_with_repository(
        target,
        provider_config,
        profile,
        workspace,
        history_dir,
        Some(inspector.repository()),
    )
}

fn build_engine_for_target_with_repository(
    target: &EngineTarget,
    provider_config: Option<ProviderConfig>,
    profile: PermissionProfile,
    workspace: &Path,
    history_dir: &Path,
    repository: Option<SessionRepository>,
) -> Result<Arc<dyn EngineRuntime>, String> {
    let engine_id = target.engine_id.as_str();
    let plan = enforcement_for(engine_id, profile)?;
    match (engine_id, plan.enforcement) {
        ("alva", EngineEnforcement::InteractiveCallbacks) => build_alva_engine(
            target,
            workspace,
            history_dir,
            provider_config.ok_or("the selected Alva route has no provider configuration")?,
            repository,
        ),
        ("claude", EngineEnforcement::ClaudePrintMode(permission_mode)) => {
            let config = claude_config(target, provider_config, permission_mode, workspace)?;
            Ok(Arc::new(ClaudePrintAdapter::new(config)))
        }
        ("codex", EngineEnforcement::CodexSandbox(sandbox)) => {
            let config = codex_config(target, provider_config, sandbox, workspace)?;
            Ok(Arc::new(CodexAdapter::new(config)))
        }
        ("pi", EngineEnforcement::PiReadOnly) | ("pi", EngineEnforcement::PiUnrestricted) => {
            let config = pi_config(
                target,
                provider_config.ok_or("the selected Pi route has no provider configuration")?,
                plan.enforcement,
                workspace,
                history_dir,
            )?;
            Ok(Arc::new(PiAdapter::new(config)))
        }
        (other, enforcement) => Err(format!(
            "Hub permission mapping for engine '{other}' returned an enforcement \
             it cannot apply: {enforcement:?}"
        )),
    }
}

fn claude_config(
    target: &EngineTarget,
    provider: Option<ProviderConfig>,
    permission_mode: alva_engine_adapter_claude::ClaudePrintPermissionMode,
    workspace: &Path,
) -> Result<ClaudePrintConfig, String> {
    let mut config = ClaudePrintConfig {
        default_cwd: Some(workspace.to_path_buf()),
        model: explicit_model(target),
        effort: target.reasoning_effort.clone(),
        permission_mode,
        ..ClaudePrintConfig::default()
    };
    if let Some(provider) = provider {
        require_direct_provider(&provider, "anthropic", "Claude Code")?;
        config
            .env
            .insert("ANTHROPIC_BASE_URL".into(), provider.base_url);
        config
            .env
            .insert("ANTHROPIC_API_KEY".into(), provider.api_key);
    }
    Ok(config)
}

fn codex_config(
    target: &EngineTarget,
    provider: Option<ProviderConfig>,
    sandbox: alva_engine_adapter_codex::CodexSandboxMode,
    workspace: &Path,
) -> Result<CodexAdapterConfig, String> {
    let mut config = CodexAdapterConfig {
        default_cwd: Some(workspace.to_path_buf()),
        model: explicit_model(target),
        sandbox,
        ..CodexAdapterConfig::default()
    };
    if let Some(effort) = &target.reasoning_effort {
        config.config_overrides.push((
            "model_reasoning_effort".into(),
            serde_json::to_string(effort)
                .map_err(|error| format!("encode Codex reasoning effort: {error}"))?,
        ));
    }
    config.config_overrides.push((
        "service_tier".into(),
        if target.fast_mode {
            "\"fast\"".into()
        } else {
            "\"default\"".into()
        },
    ));
    if let Some(provider) = provider {
        require_direct_provider(&provider, "openai-responses", "Codex")?;
        config
            .env
            .insert("ALVA_HUB_PROVIDER_API_KEY".into(), provider.api_key);
        let base_url = serde_json::to_string(&provider.base_url)
            .map_err(|error| format!("encode Codex provider endpoint: {error}"))?;
        config.config_overrides.extend([
            ("model_provider".into(), "\"alva_hub\"".into()),
            (
                "model_providers.alva_hub.name".into(),
                "\"ALVA Hub provider\"".into(),
            ),
            ("model_providers.alva_hub.base_url".into(), base_url),
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
    Ok(config)
}

fn pi_config(
    target: &EngineTarget,
    provider: ProviderConfig,
    enforcement: EngineEnforcement,
    workspace: &Path,
    history_dir: &Path,
) -> Result<PiAdapterConfig, String> {
    require_no_custom_headers(&provider, "Pi")?;
    if provider.api_key.trim().is_empty() {
        return Err("Pi custom providers require an API key".into());
    }
    let protocol = match provider.kind.as_deref() {
        Some("anthropic") => PiApiProtocol::AnthropicMessages,
        Some("openai-chat") => PiApiProtocol::OpenAiCompletions,
        Some("openai-responses") => PiApiProtocol::OpenAiResponses,
        Some("gemini") => PiApiProtocol::GoogleGenerativeAi,
        other => {
            return Err(format!(
                "Pi does not support provider protocol '{}'",
                other.unwrap_or("unknown")
            ))
        }
    };
    let custom_provider = PiProviderConfig {
        provider_id: target.provider_id.clone(),
        display_name: target.account_source.clone(),
        base_url: provider.base_url,
        api_key: provider.api_key,
        protocol,
        model_id: target.model_id.clone(),
        model_name: target.model_id.clone(),
        context_window: 200_000,
        max_output_tokens: provider.max_tokens,
        images: target
            .required_capabilities
            .iter()
            .any(|capability| matches!(capability.as_str(), "images" | "vision")),
        reasoning: true,
    };
    Ok(PiAdapterConfig {
        default_cwd: Some(workspace.to_path_buf()),
        session_dir: history_dir.join("pi-sessions"),
        provider: Some(target.provider_id.clone()),
        model: explicit_model(target),
        thinking: target.reasoning_effort.clone(),
        tool_policy: if enforcement == EngineEnforcement::PiUnrestricted {
            PiToolPolicy::Unrestricted
        } else {
            PiToolPolicy::ReadOnly
        },
        custom_provider: Some(custom_provider),
        ..PiAdapterConfig::default()
    })
}

fn require_direct_provider(
    provider: &ProviderConfig,
    expected_kind: &str,
    engine_name: &str,
) -> Result<(), String> {
    if provider.kind.as_deref() != Some(expected_kind) {
        return Err(format!(
            "{engine_name} custom providers require the {expected_kind} protocol"
        ));
    }
    require_no_custom_headers(provider, engine_name)?;
    if provider.api_key.trim().is_empty() {
        return Err(format!("{engine_name} custom providers require an API key"));
    }
    Ok(())
}

fn require_no_custom_headers(provider: &ProviderConfig, engine_name: &str) -> Result<(), String> {
    if provider.custom_headers.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{engine_name} cannot safely forward custom provider authentication headers yet"
        ))
    }
}

fn build_alva_engine(
    target: &EngineTarget,
    workspace: &Path,
    history_dir: &Path,
    config: ProviderConfig,
    repository: Option<SessionRepository>,
) -> Result<Arc<dyn EngineRuntime>, String> {
    if config.api_key.is_empty() {
        return Err("no Alva provider API key configured".into());
    }

    let provider_kind = config.kind.clone();
    let provider_registry = alva_llm_provider::build_provider_registry(&config);
    let model = alva_llm_provider::build_language_model(provider_kind.as_deref(), config);
    let effort = target
        .reasoning_effort
        .as_deref()
        .and_then(alva_kernel_abi::ReasoningEffort::parse);
    let factory = CatalogBaseAgentFactory::new(model, workspace)
        .with_provider_registry(provider_registry)
        .with_reasoning_effort(effort);

    let repository = match repository {
        Some(repository) => repository,
        None => SessionRepository::open(history_dir.join("alva-agent-records.db"))?,
    };
    Ok(Arc::new(
        AlvaAdapter::new(Arc::new(factory)).with_session_repository(repository),
    ))
}

fn explicit_model(target: &EngineTarget) -> Option<String> {
    (target.model_id != "engine-default").then(|| target.model_id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct_provider(kind: &str) -> ProviderConfig {
        ProviderConfig {
            api_key: "synthetic-key".into(),
            model: "synthetic-model".into(),
            base_url: "https://provider.example.test/v1".into(),
            max_tokens: 4096,
            custom_headers: Default::default(),
            kind: Some(kind.into()),
        }
    }

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

    #[test]
    fn third_party_provider_configs_are_projected_into_each_cli_adapter() {
        let workspace = tempfile::tempdir().unwrap();
        let history = tempfile::tempdir().unwrap();

        let claude_target = EngineTarget {
            model_id: "claude-custom".into(),
            provider_id: "anthropic".into(),
            reasoning_effort: Some("max".into()),
            ..EngineTarget::for_engine("claude").unwrap()
        };
        let claude = claude_config(
            &claude_target,
            Some(direct_provider("anthropic")),
            alva_engine_adapter_claude::ClaudePrintPermissionMode::AcceptEdits,
            workspace.path(),
        )
        .unwrap();
        assert_eq!(claude.model.as_deref(), Some("claude-custom"));
        assert_eq!(claude.effort.as_deref(), Some("max"));
        assert_eq!(
            claude.env.get("ANTHROPIC_BASE_URL").map(String::as_str),
            Some("https://provider.example.test/v1")
        );
        assert_eq!(
            claude.env.get("ANTHROPIC_API_KEY").map(String::as_str),
            Some("synthetic-key")
        );

        let codex_target = EngineTarget {
            model_id: "gpt-custom".into(),
            provider_id: "responses".into(),
            reasoning_effort: Some("xhigh".into()),
            ..EngineTarget::for_engine("codex").unwrap()
        };
        let codex = codex_config(
            &codex_target,
            Some(direct_provider("openai-responses")),
            alva_engine_adapter_codex::CodexSandboxMode::WorkspaceWrite,
            workspace.path(),
        )
        .unwrap();
        assert_eq!(codex.model.as_deref(), Some("gpt-custom"));
        assert!(codex
            .config_overrides
            .iter()
            .any(|(key, value)| key == "model_provider" && value == "\"alva_hub\""));
        assert!(codex
            .config_overrides
            .iter()
            .any(|(key, value)| key == "model_reasoning_effort" && value == "\"xhigh\""));
        assert!(codex
            .config_overrides
            .iter()
            .any(|(key, value)| key == "service_tier" && value == "\"default\""));
        assert_eq!(
            codex
                .env
                .get("ALVA_HUB_PROVIDER_API_KEY")
                .map(String::as_str),
            Some("synthetic-key")
        );

        let pi_target = EngineTarget {
            model_id: "pi-custom".into(),
            provider_id: "anthropic".into(),
            reasoning_effort: Some("max".into()),
            ..EngineTarget::for_engine("pi").unwrap()
        };
        let pi = pi_config(
            &pi_target,
            direct_provider("anthropic"),
            EngineEnforcement::PiReadOnly,
            workspace.path(),
            history.path(),
        )
        .unwrap();
        assert_eq!(pi.model.as_deref(), Some("pi-custom"));
        assert_eq!(pi.thinking.as_deref(), Some("max"));
        assert_eq!(pi.tool_policy, PiToolPolicy::ReadOnly);
        assert_eq!(
            pi.custom_provider.unwrap().protocol,
            PiApiProtocol::AnthropicMessages
        );

        let native_fast = codex_config(
            &EngineTarget {
                fast_mode: true,
                ..EngineTarget::for_engine("codex").unwrap()
            },
            None,
            alva_engine_adapter_codex::CodexSandboxMode::WorkspaceWrite,
            workspace.path(),
        )
        .unwrap();
        assert!(native_fast
            .config_overrides
            .iter()
            .any(|(key, value)| key == "service_tier" && value == "\"fast\""));
    }

    #[test]
    fn cli_adapters_reject_incompatible_provider_protocols() {
        let workspace = tempfile::tempdir().unwrap();
        let claude_target = EngineTarget::for_engine("claude").unwrap();
        assert!(claude_config(
            &claude_target,
            Some(direct_provider("openai-responses")),
            alva_engine_adapter_claude::ClaudePrintPermissionMode::AcceptEdits,
            workspace.path(),
        )
        .unwrap_err()
        .contains("anthropic"));

        let codex_target = EngineTarget::for_engine("codex").unwrap();
        assert!(codex_config(
            &codex_target,
            Some(direct_provider("openai-chat")),
            alva_engine_adapter_codex::CodexSandboxMode::WorkspaceWrite,
            workspace.path(),
        )
        .unwrap_err()
        .contains("openai-responses"));
    }
}
