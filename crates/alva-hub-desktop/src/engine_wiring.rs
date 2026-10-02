// INPUT:  four engine adapters, alva_llm_provider, alva_engine_runtime descriptors,
//         crate::{engine_registry, permissions, readiness}
// OUTPUT: builtin_blueprints() — the ONE place builtin engine names appear
// POS:    Builtin engine wiring (ticket 17). Permission, readiness, and
//         assembly consult the registry; registering these four blueprints is
//         the only place that names them. Adding a fifth engine = adding a
//         blueprint here (or registering one at runtime), touching nothing
//         else.
//         ALVA's factory, adapter and Inspector share one native repository,
//         including reference-first projection and session history retrieval.

use std::path::Path;
use std::sync::Arc;

use alva_engine_adapter_alva::{AlvaAdapter, CatalogBaseAgentFactory, SessionRepository};
use alva_engine_adapter_claude::{
    ClaudeAdapter, ClaudeAdapterConfig, PermissionMode as ClaudeSdkPermissionMode,
};
use alva_engine_adapter_codex::{CodexAdapter, CodexAdapterConfig, CodexSandboxMode};
use alva_engine_adapter_pi::{
    PiAdapter, PiAdapterConfig, PiApiProtocol, PiProviderConfig, PiToolPolicy,
};
use alva_engine_runtime::{EngineDescriptor, EngineRuntime, RuntimeCapabilities};
use alva_llm_provider::ProviderConfig;

use crate::engine_registry::{EngineBlueprint, EngineBuildArgs};
use crate::permissions::{
    EnforcementPlan, EnforcementRelation, EngineEnforcement, PermissionProfile,
};
use crate::readiness::{
    capability_labels, AuthKind, EngineProbeFacts, ReadinessSpec, StreamHealth, SystemProbe,
};
use crate::store::EngineTarget;

fn caps(c: RuntimeCapabilities) -> Vec<String> {
    capability_labels(&c)
}

fn all_caps() -> Vec<String> {
    caps(RuntimeCapabilities {
        streaming: true,
        tool_control: true,
        permission_callback: true,
        resume: true,
        cancel: true,
    })
}

// ---------------------------------------------------------------------------
// Shared config helpers (moved verbatim from the assembly module)
// ---------------------------------------------------------------------------

fn explicit_model(target: &EngineTarget) -> Option<String> {
    (target.model_id != "engine-default").then(|| target.model_id.clone())
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

fn claude_config(
    target: &EngineTarget,
    provider: Option<ProviderConfig>,
    permission_mode: ClaudeSdkPermissionMode,
    _workspace: &Path,
) -> Result<ClaudeAdapterConfig, String> {
    let mut config = ClaudeAdapterConfig {
        model: explicit_model(target),
        effort: target.reasoning_effort.clone(),
        permission_mode,
        persist_session: Some(true),
        ..ClaudeAdapterConfig::default()
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

fn claude_sdk_entry(home: Option<&Path>, workspace: &Path) -> Result<std::path::PathBuf, String> {
    crate::sdk_installation::managed_sdk_entry(
        home.ok_or("User home unavailable for the managed SDK installation")?, workspace,
        Path::new(".local/share/alva-hub/runtimes/claude/node_modules/@anthropic-ai/claude-agent-sdk"),
        "@anthropic-ai/claude-agent-sdk",
    )
}

fn pin_claude_sdk(mut config: ClaudeAdapterConfig, home: Option<&Path>, workspace: &Path) -> Result<ClaudeAdapterConfig, String> {
    config.sdk_package_path = Some(claude_sdk_entry(home, workspace)?.to_string_lossy().into_owned());
    Ok(config)
}

fn codex_config(
    target: &EngineTarget,
    provider: Option<ProviderConfig>,
    sandbox: CodexSandboxMode,
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
    read_only: bool,
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
        tool_policy: if read_only {
            PiToolPolicy::ReadOnly
        } else {
            PiToolPolicy::Unrestricted
        },
        custom_provider: Some(custom_provider),
        ..PiAdapterConfig::default()
    })
}

fn build_alva_engine(
    target: &EngineTarget,
    workspace: &Path,
    history_dir: &Path,
    config: ProviderConfig,
    repository: Option<SessionRepository>,
    tool_settings: Option<&crate::HubToolSettings>,
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
    let mut factory = CatalogBaseAgentFactory::new(model, workspace)
        .with_provider_registry(provider_registry)
        .with_reasoning_effort(effort);
    if let Some(settings) = tool_settings {
        factory = factory.with_toggles(settings.toggles()).with_managed_mcp(settings.enabled_mcp_servers());
    }

    let repository = match repository {
        Some(repository) => repository,
        None => SessionRepository::open(history_dir.join("alva-agent-records.db"))?,
    };
    let factory = factory.with_context_repository(repository.clone());
    Ok(Arc::new(
        AlvaAdapter::new(Arc::new(factory)).with_session_repository(repository),
    ))
}

// ---------------------------------------------------------------------------
// The four builtin blueprints
// ---------------------------------------------------------------------------

/// Built-in engines Hub can assemble. This function is the single wiring
/// point: adding an entry here (or registering a blueprint at runtime) makes
/// a new engine appear in readiness, the permission gate, and assembly —
/// none of which name it.
pub fn builtin_blueprints() -> Vec<EngineBlueprint> {
    vec![
        EngineBlueprint {
            engine_id: "alva".into(),
            descriptor: None,
            readiness: ReadinessSpec {
                display_name: "Alva".into(),
                minimum_version: "0.1.0".into(),
                default_auth_remedy: "Configure a provider in Settings > Models and engines."
                    .into(),
                auth_kind: AuthKind::ProviderKey,
            },
            probe: Arc::new(|probe: &SystemProbe| {
                // In-process engine: auth = a usable provider key / route.
                let authenticated = probe.provider_authenticated_for("alva").unwrap_or_else(|| {
                    // Compatibility path for non-Hub callers and isolated
                    // readiness tests. Product commands always provide the
                    // catalog-backed override above.
                    let workspace = probe.workspace_path();
                    alva_llm_provider::ProviderConfig::load(&workspace)
                        .map(|config| {
                            !config.api_key.is_empty() || !config.custom_headers.is_empty()
                        })
                        .unwrap_or(false)
                });
                let (stream_health, last_stream_failure) = probe.stream_evidence("alva");
                EngineProbeFacts {
                    binary_path: Some("built-in".to_string()),
                    dependency_error: None,
                    searched_locations: Vec::new(),
                    version: Some(Ok(env!("CARGO_PKG_VERSION").to_string())),
                    // Mirrors AlvaAdapter::capabilities(); building the real
                    // adapter requires a configured provider, which an
                    // unauthenticated dashboard must still report on.
                    capabilities: all_caps(),
                    authenticated: Some(authenticated),
                    auth_remedy: None,
                    stream_health,
                    last_stream_failure,
                }
            }),
            quick_veto: Arc::new(|probe, uses_provider_api| {
                if uses_provider_api {
                    return None;
                }
                let authenticated = probe.provider_authenticated_for("alva").unwrap_or_else(|| {
                    let workspace = probe.workspace_path();
                    alva_llm_provider::ProviderConfig::load(&workspace)
                        .map(|config| {
                            !config.api_key.is_empty() || !config.custom_headers.is_empty()
                        })
                        .unwrap_or(false)
                });
                (!authenticated).then(|| {
                    "provider key missing — configure a provider in Settings > Models and engines."
                        .to_string()
                })
            }),
            enforcement: Some(Arc::new(|profile| match profile {
                PermissionProfile::Interactive => Ok(EnforcementPlan {
                    relation: EnforcementRelation::Equivalent,
                    enforcement: EngineEnforcement::InteractiveCallbacks,
                    summary: "risk-gated permission requests are routed to you for approval".into(),
                }),
                PermissionProfile::WorkspaceWrite | PermissionProfile::Unrestricted => {
                    Ok(EnforcementPlan {
                        relation: EnforcementRelation::Stricter,
                        enforcement: EngineEnforcement::InteractiveCallbacks,
                        summary:
                            "alva still routes risk-gated requests to you instead of auto-allowing"
                                .into(),
                    })
                }
            })),
            build: Arc::new(|args: &EngineBuildArgs| {
                let provider_config = args
                    .provider_config
                    .clone()
                    .ok_or("the selected Alva route has no provider configuration")?;
                build_alva_engine(
                    args.target,
                    args.workspace,
                    args.history_dir,
                    provider_config,
                    args.repository.clone(),
                    args.tool_settings,
                )
            }),
        },
        EngineBlueprint {
            engine_id: "claude".into(),
            descriptor: Some(ClaudeAdapter::descriptor()),
            readiness: ReadinessSpec {
                display_name: "Claude Code SDK".into(),
                minimum_version: "20.0.0".into(),
                default_auth_remedy: "Install @anthropic-ai/claude-agent-sdk and sign in to Claude Code, or configure an API provider.".into(),
                auth_kind: AuthKind::CliLogin,
            },
            probe: Arc::new(|probe: &SystemProbe| {
                let native = if probe.provider_authenticated_for("claude").is_some() { None } else { probe.file_marker_auth(
                    ".claude/.credentials.json",
                    ".claude.json",
                    "oauthAccount",
                ) };
                let mut facts = probe.cli_facts(
                    "claude",
                    "node",
                    caps(ClaudeAdapter::new(ClaudeAdapterConfig::default()).capabilities()),
                    native,
                );
                facts.dependency_error = claude_sdk_entry(
                    probe.home_dir().map(|path| path.as_path()), &probe.workspace_path(),
                ).err();
                facts
            }),
            quick_veto: Arc::new(|probe: &SystemProbe, uses_provider_api| {
                if probe.locate("node").is_none() {
                    return Some("Claude Agent SDK requires Node.js".to_string());
                }
                if let Err(error) = claude_sdk_entry(
                    probe.home_dir().map(|path| path.as_path()), &probe.workspace_path(),
                ) { return Some(error); }
                if uses_provider_api {
                    return None;
                }
                let authenticated = probe.file_marker_auth(
                    ".claude/.credentials.json",
                    ".claude.json",
                    "oauthAccount",
                );
                (authenticated == Some(false))
                    .then(|| format!("not signed in — run `login` in a terminal to sign in."))
            }),
            enforcement: Some(Arc::new(|profile| match profile {
                PermissionProfile::Interactive => Ok(EnforcementPlan {
                    relation: EnforcementRelation::Equivalent,
                    enforcement: EngineEnforcement::ClaudeSdkMode(ClaudeSdkPermissionMode::Default),
                    summary: "Claude Agent SDK routes tool permission requests to the Hub user".into(),
                }),
                PermissionProfile::WorkspaceWrite => Ok(EnforcementPlan {
                    relation: EnforcementRelation::Stricter,
                    enforcement: EngineEnforcement::ClaudeSdkMode(
                        ClaudeSdkPermissionMode::Default,
                    ),
                    summary:
                        "Claude Agent SDK uses approval callbacks, including for file edits"
                            .into(),
                }),
                PermissionProfile::Unrestricted => Ok(EnforcementPlan {
                    relation: EnforcementRelation::Equivalent,
                    enforcement: EngineEnforcement::ClaudeSdkMode(
                        ClaudeSdkPermissionMode::BypassPermissions,
                    ),
                    summary: "bypassPermissions auto-allows everything".into(),
                }),
            })),
            build: Arc::new(|args: &EngineBuildArgs| {
                let EngineEnforcement::ClaudeSdkMode(permission_mode) = args.plan.enforcement
                else {
                    return Err("Claude route resolved to a non-Claude enforcement".into());
                };
                let config = claude_config(
                    args.target,
                    args.provider_config.clone(),
                    permission_mode,
                    args.workspace,
                )?;
                let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
                let config = pin_claude_sdk(config, home.as_deref(), args.workspace)?;
                Ok(Arc::new(ClaudeAdapter::new(config)))
            }),
        },
        EngineBlueprint {
            engine_id: "codex".into(),
            descriptor: Some(alva_engine_adapter_codex::CodexAdapter::descriptor()),
            readiness: ReadinessSpec {
                display_name: "Codex CLI".into(),
                minimum_version: "0.20.0".into(),
                default_auth_remedy: "Run `codex login` in a terminal to sign in.".into(),
                auth_kind: AuthKind::CliLogin,
            },
            probe: Arc::new(|probe: &SystemProbe| {
                let native = probe.file_auth(".codex/auth.json");
                probe.cli_facts(
                    "codex",
                    "codex",
                    caps(CodexAdapter::new(CodexAdapterConfig::default()).capabilities()),
                    native,
                )
            }),
            quick_veto: Arc::new(|probe: &SystemProbe, uses_provider_api| {
                if probe.locate("codex").is_none() {
                    return Some("engine binary not found".to_string());
                }
                if uses_provider_api {
                    return None;
                }
                let authenticated = probe.file_auth(".codex/auth.json");
                (authenticated == Some(false))
                    .then(|| format!("not signed in — run `login` in a terminal to sign in."))
            }),
            enforcement: Some(Arc::new(|profile| match profile {
                PermissionProfile::Interactive => Err(format!(
                    "codex cannot preserve or tighten the session permission profile \
                     '{}': the codex CLI runs with approval_policy=never and cannot route \
                     permission approvals to you; its sandbox either auto-allows \
                     workspace writes or silently denies them, which weakens or \
                     silently rewrites the profile",
                    profile.as_str()
                )),
                PermissionProfile::WorkspaceWrite => Ok(EnforcementPlan {
                    relation: EnforcementRelation::Equivalent,
                    enforcement: EngineEnforcement::CodexSandbox(CodexSandboxMode::WorkspaceWrite),
                    summary: "the workspace-write sandbox bounds side effects to the workspace"
                        .into(),
                }),
                PermissionProfile::Unrestricted => Ok(EnforcementPlan {
                    relation: EnforcementRelation::Equivalent,
                    enforcement: EngineEnforcement::CodexSandbox(
                        CodexSandboxMode::DangerFullAccess,
                    ),
                    summary: "danger-full-access auto-allows everything".into(),
                }),
            })),
            build: Arc::new(|args: &EngineBuildArgs| {
                let EngineEnforcement::CodexSandbox(sandbox) = args.plan.enforcement else {
                    return Err("Codex route resolved to a non-Codex enforcement".into());
                };
                let config = codex_config(
                    args.target,
                    args.provider_config.clone(),
                    sandbox,
                    args.workspace,
                )?;
                Ok(Arc::new(CodexAdapter::new(config)))
            }),
        },
        EngineBlueprint {
            engine_id: "pi".into(),
            descriptor: Some(alva_engine_adapter_pi::PiAdapter::descriptor()),
            readiness: ReadinessSpec {
                display_name: "Pi".into(),
                minimum_version: "0.84.0".into(),
                default_auth_remedy:
                    "Configure a compatible provider in Settings > Models and engines.".into(),
                auth_kind: AuthKind::ProviderKey,
            },
            probe: Arc::new(|probe: &SystemProbe| {
                probe.cli_facts(
                    "pi",
                    "pi",
                    caps(PiAdapter::new(PiAdapterConfig::default()).capabilities()),
                    Some(false),
                )
            }),
            quick_veto: Arc::new(|probe: &SystemProbe, uses_provider_api| {
                if probe.locate("pi").is_none() {
                    return Some("engine binary not found".to_string());
                }
                if uses_provider_api {
                    return None;
                }
                Some("provider key missing — configure a compatible provider in Settings > Models and engines."
                    .to_string())
            }),
            enforcement: Some(Arc::new(|profile| match profile {
                PermissionProfile::Interactive => Err(format!(
                    "pi cannot preserve or tighten the session permission profile \
                     '{}': Pi print mode does not expose interactive permission callbacks",
                    profile.as_str()
                )),
                PermissionProfile::WorkspaceWrite => Ok(EnforcementPlan {
                    relation: EnforcementRelation::Stricter,
                    enforcement: EngineEnforcement::PiReadOnly,
                    summary: "Pi is restricted to read, grep, find, and ls tools".into(),
                }),
                PermissionProfile::Unrestricted => Ok(EnforcementPlan {
                    relation: EnforcementRelation::Equivalent,
                    enforcement: EngineEnforcement::PiUnrestricted,
                    summary: "Pi runs its complete configured tool set".into(),
                }),
            })),
            build: Arc::new(|args: &EngineBuildArgs| {
                let read_only = args.plan.enforcement == EngineEnforcement::PiReadOnly;
                let provider_config = args
                    .provider_config
                    .clone()
                    .ok_or("the selected Pi route has no provider configuration")?;
                let config = pi_config(
                    args.target,
                    provider_config,
                    read_only,
                    args.workspace,
                    args.history_dir,
                )?;
                Ok(Arc::new(PiAdapter::new(config)))
            }),
        },
    ]
}

#[cfg(test)]
mod assembly_tests {
    use super::*;

    use super::*;
    use crate::alva::build_engine;
    use alva_llm_provider::ProviderConfig;

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
        for engine_id in ["codex", "pi"] {
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
    fn claude_product_assembly_uses_sdk_permission_callbacks() {
        let workspace = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let config = claude_config(
            &EngineTarget::for_engine("claude").unwrap(), None,
            ClaudeSdkPermissionMode::Default, workspace.path(),
        ).unwrap();
        assert_eq!(config.permission_mode, ClaudeSdkPermissionMode::Default);
        assert_eq!(config.persist_session, Some(true));
        assert!(config.setting_sources.is_empty());
        assert!(pin_claude_sdk(config.clone(), Some(home.path()), workspace.path()).is_err());
        let package = home.path().join(".local/share/alva-hub/runtimes/claude/node_modules/@anthropic-ai/claude-agent-sdk");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("package.json"), r#"{"name":"@anthropic-ai/claude-agent-sdk","version":"1.0.0","main":"sdk.mjs"}"#).unwrap();
        std::fs::write(package.join("sdk.mjs"), "throw new Error('probe must not import');").unwrap();
        let config = pin_claude_sdk(config, Some(home.path()), workspace.path()).unwrap();
        assert_eq!(config.sdk_package_path.as_deref(), package.canonicalize().unwrap().join("sdk.mjs").to_str());
        assert!(ClaudeAdapter::new(config).capabilities().permission_callback);
    }

    #[test]
    fn sdk_missing_is_reported_and_vetoed_even_with_node_and_provider_auth() {
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let node = bin.path().join(if cfg!(windows) { "node.exe" } else { "node" });
        std::fs::write(&node, "").unwrap();
        #[cfg(unix)] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let probe = SystemProbe::new(None, Some(workspace.path().into()))
            .with_home_dir(home.path().into()).with_search_paths(vec![bin.path().into()])
            .with_provider_authenticated("claude", true);
        let error = probe.quick_veto_for_target("claude", true).expect("missing SDK must veto API route");
        assert!(error.contains("Managed SDK"));
        let facts = (crate::engine_registry::blueprint("claude").unwrap().probe)(&probe);
        assert_eq!(facts.dependency_error.as_deref(), Some(error.as_str()));
        let report = crate::readiness::classify_engine("claude", EngineProbeFacts {
            binary_path: Some(node.to_string_lossy().into_owned()),
            version: Some(Ok("22.0.0".into())), authenticated: Some(true),
            dependency_error: Some(error), ..Default::default()
        });
        assert_eq!(report.status, crate::readiness::ReadinessStatus::Missing);
        assert!(matches!(&report.reasons[0], crate::readiness::ReadinessReason::DependencyUnavailable { .. }));
    }

    #[tokio::test]
    async fn pinned_product_sdk_ignores_a_hostile_workspace_package() {
        use tokio_stream::StreamExt;
        let home = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let trusted = home.path().join(".local/share/alva-hub/runtimes/claude/node_modules/@anthropic-ai/claude-agent-sdk");
        let hostile = workspace.path().join("node_modules/@anthropic-ai/claude-agent-sdk");
        for package in [&trusted, &hostile] {
            std::fs::create_dir_all(package).unwrap();
            std::fs::write(package.join("package.json"), r#"{"name":"@anthropic-ai/claude-agent-sdk","version":"1.0.0","main":"sdk.mjs"}"#).unwrap();
        }
        std::fs::write(hostile.join("sdk.mjs"), "throw new Error('project SDK must never load');").unwrap();
        std::fs::write(trusted.join("sdk.mjs"), r#"
export function query() { return (async function* () {
  yield { type: 'system', subtype: 'init', session_id: 'trusted-sdk', model: 'fake', tools: [] };
  yield { type: 'result', subtype: 'success', result: 'trusted-sdk-ok', duration_ms: 1, num_turns: 1, usage: {} };
})(); }
"#).unwrap();
        let config = claude_config(&EngineTarget::for_engine("claude").unwrap(),
            Some(direct_provider("anthropic")), ClaudeSdkPermissionMode::Default, workspace.path()).unwrap();
        let config = pin_claude_sdk(config, Some(home.path()), workspace.path()).unwrap();
        let adapter = ClaudeAdapter::new(config);
        let events: Vec<_> = adapter.execute(alva_engine_runtime::RuntimeRequest::new("synthetic test").with_cwd(workspace.path()))
            .unwrap().collect().await;
        assert!(events.iter().any(|event| matches!(event,
            alva_engine_runtime::RuntimeEvent::Completed { result: Some(text), .. } if text == "trusted-sdk-ok")), "{events:?}");
        assert!(!events.iter().any(|event| matches!(event, alva_engine_runtime::RuntimeEvent::Error { .. })), "{events:?}");
    }

    #[test]
    fn eligible_cli_engines_assemble_with_profile_mapping() {
        let workspace = tempfile::tempdir().unwrap();
        let history = tempfile::tempdir().unwrap();
        for engine_id in ["codex"] {
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
            ClaudeSdkPermissionMode::Default,
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
            true, // PiReadOnly tool policy
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
            ClaudeSdkPermissionMode::Default,
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
