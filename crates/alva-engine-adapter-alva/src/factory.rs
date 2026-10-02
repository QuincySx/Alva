// INPUT:  app-core BaseAgent/catalog/paths/Approval/MCP plugins, RuntimeRequest, LanguageModel
// OUTPUT: BaseAgentFactory, SessionPluginFactory, BuiltAgent, DefaultBaseAgentFactory, CatalogBaseAgentFactory
// POS:    Per-session harness assembly with Hub capability/MCP snapshots and explicitly root-bound product Plugins.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use alva_app_core::extension::{ApprovalPlugin, Plugin};
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

/// Product-owned Plugin recipe bound to the actual native root session.
/// Unlike tools cloned for child scopes, its implementation must validate the
/// execution session before exposing product history or artifacts.
pub trait SessionPluginFactory: Send + Sync {
    fn create(&self, native_session_id: &str) -> Box<dyn Plugin>;
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
    managed_mcp: Option<Vec<alva_app_core::extension::skills::skill_domain::mcp::McpServerConfig>>,
    context_repository: Option<crate::SessionRepository>,
    session_plugins: Option<Arc<dyn SessionPluginFactory>>,
}

impl CatalogBaseAgentFactory {
    pub fn new(model: Arc<dyn LanguageModel>, default_workspace: impl Into<PathBuf>) -> Self {
        Self {
            model,
            default_workspace: default_workspace.into(),
            default_system_prompt: "You are a helpful coding assistant. Use only the tools \
                 available in the current tool schema; capabilities may be disabled. \
                 Use available tools when needed to accomplish the user's task. \
                 Be concise in your responses."
                .into(),
            max_iterations: 100,
            provider_registry: None,
            toggles: ComponentToggles::default(),
            reasoning_effort: None,
            managed_mcp: None,
            context_repository: None,
            session_plugins: None,
        }
    }

    pub fn with_provider_registry(mut self, reg: Arc<alva_agent_abi::ProviderRegistry>) -> Self {
        self.provider_registry = Some(reg);
        self
    }

    /// The SAME authority used by the adapter and Inspector; enables bounded
    /// reference-first tool projection and session history retrieval.
    pub fn with_context_repository(mut self, repository: crate::SessionRepository) -> Self {
        self.context_repository = Some(repository);
        self
    }

    /// Explicit product recipe; does not discover ambient plugins or grant
    /// its root-only data access to recursive children.
    pub fn with_session_plugins(mut self, plugins: Arc<dyn SessionPluginFactory>) -> Self {
        self.session_plugins = Some(plugins);
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

    /// Explicit Hub configuration; excludes ambient MCP and AEP discovery.
    /// Empty is intentional and does not fall back to global files.
    pub fn with_managed_mcp(
        mut self,
        servers: Vec<alva_app_core::extension::skills::skill_domain::mcp::McpServerConfig>,
    ) -> Self {
        self.managed_mcp = Some(servers);
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

        let mut toggles = self.toggles.clone();
        if self.managed_mcp.is_some() {
            toggles.insert("mcp".into(), false);
            toggles.insert("subprocess-loader".into(), false);
        }
        let mut builder = components::apply_components(
            BaseAgentBuilder::new()
                .workspace(&workspace)
                .system_prompt(&system_prompt)
                .max_iterations(max_iterations)
                .session(session.clone())
                .plugin(Box::new(approval_ext)),
            &toggles,
            &ctx,
        );
        if let Some(servers) = &self.managed_mcp {
            if !servers.is_empty() {
                builder =
                    builder.plugin(Box::new(alva_app_core::extension::McpPlugin::from_servers(
                        servers.clone(),
                        workspace.clone(),
                    ).with_progressive_discovery()));
            }
        }
        if let Some(repository) = &self.context_repository {
            builder = builder.plugin(Box::new(crate::context_retrieval::ContextRetrievalPlugin::new(
                repository.clone(), session.session_id().to_string(),
            )));
        }
        if let Some(plugins) = &self.session_plugins {
            builder = builder.plugin(plugins.create(session.session_id()));
        }
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

    #[cfg(unix)]
    #[tokio::test]
    async fn managed_capabilities_remove_tools_and_gate_real_mcp_calls() {
        use alva_app_core::alva_protocol_mcp::types::{McpServerConfig, McpTransportConfig};
        use alva_app_core::{SecurityDecision, SecurityGuard};
        use alva_kernel_abi::{EffectDescription, MinimalExecutionContext};
        let workspace = tempfile::tempdir().unwrap();
        let server = McpServerConfig { id: "fixture".into(), display_name: "Fixture".into(), auto_connect: true, connect_timeout_secs: 5, transport: McpTransportConfig::Stdio { command: "/usr/bin/python3".into(), args: vec!["-u".into(), "-c".into(), r#"import json,sys
for line in sys.stdin:
 r=json.loads(line)
 if 'id' not in r: continue
 result={'protocolVersion':r['params']['protocolVersion'],'capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'}} if r['method']=='initialize' else {'tools':[{'name':'echo','inputSchema':{'type':'object'}}]} if r['method']=='tools/list' else {'content':[{'type':'text','text':'x'*600000+'native-result-tail'}]}
 print(json.dumps({'jsonrpc':'2.0','id':r['id'],'result':result}),flush=True)
"#.into()], env: Default::default() } };
        let toggles = components_for_test()
            .into_iter()
            .map(|id| (id.to_string(), false))
            .collect();
        let mut call = alva_kernel_abi::Message::user("");
        call.role = alva_kernel_abi::MessageRole::Assistant;
        call.content = vec![alva_kernel_abi::ContentBlock::ToolUse { id:"mcp-call".into(), name:"mcp_runtime".into(), input:serde_json::json!({"action":"call_tool","server_id":"fixture","tool_name":"echo"}) }];
        let mut search = alva_kernel_abi::Message::user("");
        search.role = alva_kernel_abi::MessageRole::Assistant;
        search.content = vec![alva_kernel_abi::ContentBlock::ToolUse { id:"history-call".into(), name:"context_retrieval".into(), input:serde_json::json!({"action":"search_history","query":"native-result-tail","kind":"tool"}) }];
        let mut done = alva_kernel_abi::Message::user("done"); done.role = alva_kernel_abi::MessageRole::Assistant;
        let model = MockLanguageModel::new().with_response(call).with_response(search).with_response(done);
        let repository = crate::SessionRepository::open(workspace.path().join("records.db")).unwrap();
        let session = repository.open_session("fixture-session", None).await.unwrap();
        let factory =
            CatalogBaseAgentFactory::new(Arc::new(model.clone()), workspace.path())
                .with_toggles(toggles)
                .with_context_repository(repository.clone())
                .with_managed_mcp(vec![server]);
        let request = RuntimeRequest {
            prompt: "fixture".into(),
            resume_session: None,
            system_prompt: None,
            working_directory: None,
            options: Default::default(),
        };
        let mut built = factory
            .build(
                &request,
                session,
            )
            .await
            .unwrap();
        let names = built.agent.tool_names();
        assert!(!names
            .iter()
            .any(|n| n == "execute_shell" || n == "read_file"));
        let alias = alva_app_core::alva_protocol_mcp::tool_adapter::McpToolAdapter::tool_name(
            "fixture", "echo",
        );
        assert!(!names.iter().any(|n| n == &alias));
        assert!(names.iter().any(|n| n == "mcp_runtime"));
        assert!(names.iter().any(|n| n == "context_retrieval"));
        assert_eq!(names.iter().filter(|name| name.starts_with("mcp_")).count(), 1);
        let guard = built
            .agent
            .bus()
            .get::<tokio::sync::Mutex<SecurityGuard>>()
            .unwrap();
        for name in ["mcp_runtime"] {
            assert!(matches!(
                guard.lock().await.check_tool_call(
                    name,
                    &serde_json::json!({}),
                    &MinimalExecutionContext::new(),
                    &EffectDescription::of("External MCP")
                ),
                SecurityDecision::NeedHumanApproval { .. }
            ));
        }
        assert!(built
            .agent
            .assembly_snapshot()
            .middleware_names
            .iter()
            .any(|n| n.contains("security")));
        let mut approvals = built.approval_rx.take().unwrap();
        let mut events = built.agent.prompt_text("Read fixture data, then search its result").await;
        let mut raw_result = false;
        let mut search_result = false;
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                tokio::select! {
                    Some(approval) = approvals.recv() => {
                        assert_eq!(approval.tool_name, "mcp_runtime");
                        assert!(built.agent.resolve_permission(&approval.request_id, &approval.tool_name, alva_app_core::PermissionDecision::AllowOnce).await);
                    }
                    event = events.recv() => match event {
                        Some(alva_kernel_core::AgentEvent::ToolExecutionEnd { tool_call, result, .. }) => {
                            if tool_call.name == "mcp_runtime" { raw_result = result.model_text().len() > 600000; }
                            if tool_call.name == "context_retrieval" { search_result = result.model_text().contains("native-result-tail"); }
                        }
                        Some(_) => {},
                        None => break,
                    }
                }
            }
        }).await.expect("native MCP/retrieval run must terminate");
        assert!(raw_result && search_result);
        let calls = model.calls();
        assert_eq!(calls.len(), 3, "reference projection must precede automatic compaction");
        assert!(serde_json::to_string(&calls[1]).unwrap().contains("context_reference"));
        assert!(calls.iter().all(|messages| serde_json::to_string(messages).unwrap().len() < 20000));
        assert!(repository.records("fixture-session").unwrap().iter().any(|record| record.type_name == "tool_result"));
    }

    #[cfg(unix)]
    fn components_for_test() -> Vec<&'static str> {
        // Disable path-based discovery and optional tools, but not safety.
        vec![
            "core",
            "shell",
            "web",
            "skills",
            "task",
            "team",
            "sub-agents",
            "hooks",
            "checkpoint",
            "interaction",
        ]
    }
}
