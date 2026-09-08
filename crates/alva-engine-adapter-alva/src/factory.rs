// INPUT:  alva_app_core::{BaseAgent, components, paths, ApprovalPlugin}, RuntimeRequest, LanguageModel
// OUTPUT: BaseAgentFactory, BuiltAgent, DefaultBaseAgentFactory, CatalogBaseAgentFactory
// POS:    Per-engine-session BaseAgent construction; catalog factory matches CLI/Tauri harness depth.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use alva_app_core::extension::ApprovalPlugin;
use alva_app_core::{
    components::{self, ComponentContext, ComponentToggles, SkillsDiscovery},
    AlvaPaths, ApprovalRequest, BaseAgent, BaseAgentBuilder, EngineError,
};
use alva_engine_runtime::RuntimeRequest;
use alva_kernel_abi::{AgentSession, LanguageModel};
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Result of assembling one Hub Alva session agent.
pub struct BuiltAgent {
    pub agent: BaseAgent,
    /// When present, the product must drain this and call
    /// [`BaseAgent::resolve_permission`] (via the engine adapter) or tools that
    /// need HITL will stall. Hub CLI auto-denies.
    pub approval_rx: Option<mpsc::UnboundedReceiver<ApprovalRequest>>,
}

/// Builds a fresh complete `BaseAgent` for each new engine session.
#[async_trait]
pub trait BaseAgentFactory: Send + Sync {
    async fn build(
        &self,
        request: &RuntimeRequest,
        session: Arc<dyn AgentSession>,
    ) -> Result<BuiltAgent, EngineError>;
}

/// Minimal factory input (chat + default plugins only — no tool catalog).
#[derive(Clone)]
pub struct DefaultBaseAgentFactoryConfig {
    pub model: Arc<dyn LanguageModel>,
    pub workspace: PathBuf,
    pub system_prompt: String,
    pub max_iterations: u32,
}

impl DefaultBaseAgentFactoryConfig {
    pub fn new(model: Arc<dyn LanguageModel>, workspace: impl Into<PathBuf>) -> Self {
        Self {
            model,
            workspace: workspace.into(),
            system_prompt: "You are a helpful AI assistant.".into(),
            max_iterations: 100,
        }
    }
}

/// Baseline factory: default memory/security/system-context only.
///
/// Prefer [`CatalogBaseAgentFactory`] for product Hub runs that need tools,
/// skills, and MCP parity with `alva-app-cli`.
pub struct DefaultBaseAgentFactory {
    config: DefaultBaseAgentFactoryConfig,
}

impl DefaultBaseAgentFactory {
    pub fn new(config: DefaultBaseAgentFactoryConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl BaseAgentFactory for DefaultBaseAgentFactory {
    async fn build(
        &self,
        request: &RuntimeRequest,
        session: Arc<dyn AgentSession>,
    ) -> Result<BuiltAgent, EngineError> {
        let workspace = request
            .working_directory
            .as_ref()
            .unwrap_or(&self.config.workspace);
        let system_prompt = request
            .system_prompt
            .as_deref()
            .unwrap_or(&self.config.system_prompt);
        let max_iterations = request
            .options
            .max_turns
            .unwrap_or(self.config.max_iterations);

        let agent = BaseAgent::builder()
            .workspace(workspace)
            .system_prompt(system_prompt)
            .max_iterations(max_iterations)
            .session(session)
            .build(self.config.model.clone())
            .await?;
        Ok(BuiltAgent {
            agent,
            approval_rx: None,
        })
    }
}

/// Catalog factory: same component table as CLI/Tauri (`apply_components`).
///
/// Includes core/shell/web/skills/MCP/sub-agents/… plus `ApprovalPlugin` so
/// SecurityMiddleware can surface HITL as engine `PermissionRequest`s.
pub struct CatalogBaseAgentFactory {
    model: Arc<dyn LanguageModel>,
    /// Fallback workspace when the request has no `working_directory`.
    default_workspace: PathBuf,
    default_system_prompt: String,
    max_iterations: u32,
    /// Optional provider registry for sub-agent model routing.
    provider_registry: Option<Arc<alva_agent_abi::ProviderRegistry>>,
    /// Component on/off map (empty → catalog defaults).
    toggles: ComponentToggles,
    reasoning_effort: Option<alva_kernel_abi::ReasoningEffort>,
}

impl CatalogBaseAgentFactory {
    pub fn new(model: Arc<dyn LanguageModel>, default_workspace: impl Into<PathBuf>) -> Self {
        Self {
            model,
            default_workspace: default_workspace.into(),
            default_system_prompt: "You are a helpful coding assistant. You have access to tools \
                 for running shell commands, reading/writing files, and searching code. \
                 Use tools when needed to accomplish the user's task. \
                 Be concise in your responses."
                .into(),
            max_iterations: 100,
            provider_registry: None,
            toggles: ComponentToggles::default(),
            reasoning_effort: None,
        }
    }

    pub fn with_provider_registry(mut self, reg: Arc<alva_agent_abi::ProviderRegistry>) -> Self {
        self.provider_registry = Some(reg);
        self
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.default_system_prompt = prompt.into();
        self
    }

    pub fn with_max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = max;
        self
    }

    pub fn with_toggles(mut self, toggles: ComponentToggles) -> Self {
        self.toggles = toggles;
        self
    }

    pub fn with_reasoning_effort(
        mut self,
        effort: Option<alva_kernel_abi::ReasoningEffort>,
    ) -> Self {
        self.reasoning_effort = effort;
        self
    }
}

#[async_trait]
impl BaseAgentFactory for CatalogBaseAgentFactory {
    async fn build(
        &self,
        request: &RuntimeRequest,
        session: Arc<dyn AgentSession>,
    ) -> Result<BuiltAgent, EngineError> {
        let workspace = request
            .working_directory
            .clone()
            .unwrap_or_else(|| self.default_workspace.clone());
        let paths = AlvaPaths::new(&workspace);

        let base_prompt = request
            .system_prompt
            .as_deref()
            .unwrap_or(&self.default_system_prompt);
        let system_prompt = format!("{base_prompt}{}", load_project_context(&workspace));
        let max_iterations = request.options.max_turns.unwrap_or(self.max_iterations);

        let (approval_ext, approval_rx) = ApprovalPlugin::with_channel();
        let ctx = ComponentContext {
            workspace: workspace.clone(),
            provider_registry: self.provider_registry.clone(),
            skills: Some(SkillsDiscovery::from_alva_paths(&paths, None)),
            mcp_config_paths: paths.mcp_discovery_paths(),
            subagent_depth: components::DEFAULT_SUBAGENT_DEPTH,
            subagent_timeout: components::DEFAULT_SUBAGENT_TIMEOUT,
            subagent_tool_timeout: components::DEFAULT_SUBAGENT_TOOL_TIMEOUT,
            agent_templates: alva_app_core::extension::agent_templates::resolve_agent_templates(&[
                paths.global_agents_config(),
                paths.project_agents_config(),
            ]),
            hooks_settings: alva_app_core::settings::HooksSettings::default(),
            subprocess_ext_dirs: vec![
                paths.project_extensions_dir(),
                paths.global_extensions_dir(),
            ],
        };

        let builder = components::apply_components(
            BaseAgentBuilder::new()
                .workspace(&workspace)
                .system_prompt(&system_prompt)
                .max_iterations(max_iterations)
                .session(session)
                .plugin(Box::new(approval_ext)),
            &self.toggles,
            &ctx,
        );
        let agent = builder.build(self.model.clone()).await?;
        agent.set_reasoning_effort(self.reasoning_effort).await;
        Ok(BuiltAgent {
            agent,
            approval_rx: Some(approval_rx),
        })
    }
}

/// Workspace AGENTS.md / CLAUDE.md / .alva/context.md (bounded, path-safe).
fn load_project_context(workspace: &Path) -> String {
    const MAX_BYTES: u64 = 1024 * 1024;
    let Ok(canonical_workspace) = workspace.canonicalize() else {
        return String::new();
    };
    let mut context = String::new();
    let mut seen = std::collections::HashSet::new();

    let agents_start = std::env::current_dir()
        .ok()
        .and_then(|cwd| cwd.canonicalize().ok())
        .filter(|cwd| cwd.starts_with(&canonical_workspace))
        .unwrap_or_else(|| canonical_workspace.clone());

    for path in alva_app_core::paths::collect_agents_md_chain(&agents_start, &canonical_workspace) {
        append_context_file(
            &mut context,
            &mut seen,
            &canonical_workspace,
            &path,
            "AGENTS.md",
            MAX_BYTES,
        );
    }
    for name in ["CLAUDE.md", ".alva/context.md"] {
        let path = workspace.join(name);
        let Ok(canonical_path) = path.canonicalize() else {
            continue;
        };
        append_context_file(
            &mut context,
            &mut seen,
            &canonical_workspace,
            &canonical_path,
            name,
            MAX_BYTES,
        );
    }
    context
}

fn append_context_file(
    context: &mut String,
    seen: &mut std::collections::HashSet<PathBuf>,
    canonical_workspace: &Path,
    canonical_path: &Path,
    label: &str,
    max_bytes: u64,
) {
    use std::io::Read as _;

    if !canonical_path.starts_with(canonical_workspace) || !canonical_path.is_file() {
        return;
    }
    if !seen.insert(canonical_path.to_path_buf()) {
        return;
    }
    let Ok(file) = std::fs::File::open(canonical_path) else {
        return;
    };
    let mut bytes = Vec::new();
    if file.take(max_bytes + 1).read_to_end(&mut bytes).is_err() {
        return;
    }
    let truncated = bytes.len() as u64 > max_bytes;
    bytes.truncate(max_bytes as usize);
    let content = String::from_utf8_lossy(&bytes);
    let display = canonical_path
        .strip_prefix(canonical_workspace)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| label.to_string());
    context.push_str(&format!(
        "\n\n# Project Context (from {})\n\n{}{}",
        display,
        content,
        if truncated {
            "\n\n[project context truncated at 1 MiB]"
        } else {
            ""
        }
    ));
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use alva_engine_runtime::RuntimeOptions;
    use alva_test::mock_provider::MockLanguageModel;

    /// The Hub Alva engine session must carry the same long-session
    /// middleware as the CLI/Tauri components table: context compaction
    /// (long conversations stay inside the window) and auto-checkpoint
    /// (edited files are archived). Regression guard for the Catalog
    /// factory silently drifting from `apply_components`.
    #[tokio::test]
    async fn catalog_factory_wires_compaction_and_checkpoint_middleware() {
        let factory =
            CatalogBaseAgentFactory::new(Arc::new(MockLanguageModel::new()), std::env::temp_dir());
        let request = RuntimeRequest {
            prompt: "do the thing".to_string(),
            resume_session: None,
            system_prompt: None,
            working_directory: None,
            options: RuntimeOptions {
                streaming: false,
                max_turns: None,
                extra: HashMap::new(),
            },
        };
        let built = factory
            .build(
                &request,
                Arc::new(alva_agent_core::agent_session::InMemoryAgentSession::new()),
            )
            .await
            .expect("catalog agent builds");
        let snapshot = built.agent.assembly_snapshot();
        let middleware = snapshot.middleware_names.clone();
        assert!(
            middleware.iter().any(|name| name.contains("compaction")),
            "long-session context compaction missing: {middleware:?}"
        );
        assert!(
            middleware.iter().any(|name| name.contains("checkpoint")),
            "auto-checkpoint missing: {middleware:?}"
        );
        // The tool-timeout middleware must use the real TokioSleeper so the
        // 120s per-tool cap is actually enforced (a NoopSleeper is dead).
        assert!(
            middleware.iter().any(|name| name.contains("tool_timeout")),
            "enforced tool timeout missing: {middleware:?}"
        );
    }
}
