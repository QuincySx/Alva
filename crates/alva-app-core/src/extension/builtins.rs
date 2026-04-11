//! Built-in extensions — each wraps a tool preset or middleware set.

use std::path::PathBuf;
use std::sync::Arc;

use alva_types::tool::Tool;
use alva_agent_core::middleware::Middleware;
use alva_agent_tools::tool_presets;
use async_trait::async_trait;

use super::{Extension, ExtensionContext, HostAPI};

// ===========================================================================
// Tool extensions
// ===========================================================================

/// Core file I/O tools: read, write, edit, search, list.
pub struct CoreExtension;
#[async_trait]
impl Extension for CoreExtension {
    fn name(&self) -> &str { "core" }
    fn description(&self) -> &str { "Core file I/O tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::file_io() }
}

/// Shell execution tool.
pub struct ShellExtension;
#[async_trait]
impl Extension for ShellExtension {
    fn name(&self) -> &str { "shell" }
    fn description(&self) -> &str { "Shell execution" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::shell() }
}

/// Human interaction tool (ask_human).
pub struct InteractionExtension;
#[async_trait]
impl Extension for InteractionExtension {
    fn name(&self) -> &str { "interaction" }
    fn description(&self) -> &str { "Human interaction" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::interaction() }
}

/// Task management tools: create, update, get, list, output, stop.
pub struct TaskExtension;
#[async_trait]
impl Extension for TaskExtension {
    fn name(&self) -> &str { "task" }
    fn description(&self) -> &str { "Task management" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::task_management() }
}

/// Team / multi-agent coordination tools.
pub struct TeamExtension;
#[async_trait]
impl Extension for TeamExtension {
    fn name(&self) -> &str { "team" }
    fn description(&self) -> &str { "Team / multi-agent coordination" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::team() }
}

/// Planning and worktree tools.
pub struct PlanningExtension;
#[async_trait]
impl Extension for PlanningExtension {
    fn name(&self) -> &str { "planning" }
    fn description(&self) -> &str { "Planning and worktree tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        let mut tools = tool_presets::planning();
        tools.extend(tool_presets::worktree());
        tools
    }
}

/// Utility tools: sleep, config, notebook, skill, tool_search, schedule, remote.
pub struct UtilityExtension;
#[async_trait]
impl Extension for UtilityExtension {
    fn name(&self) -> &str { "utility" }
    fn description(&self) -> &str { "Utility tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::utility() }
}

/// Web tools: internet search, URL fetching.
pub struct WebExtension;
#[async_trait]
impl Extension for WebExtension {
    fn name(&self) -> &str { "web" }
    fn description(&self) -> &str { "Web tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::web() }
}

/// Browser automation tools.
pub struct BrowserExtension;
#[async_trait]
impl Extension for BrowserExtension {
    fn name(&self) -> &str { "browser" }
    fn description(&self) -> &str { "Browser automation tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::browser_tools() }
}

// ===========================================================================
// Middleware extensions
// ===========================================================================

/// Loop detection middleware.
pub struct LoopDetectionExtension;
#[async_trait]
impl Extension for LoopDetectionExtension {
    fn name(&self) -> &str { "loop-detection" }
    fn description(&self) -> &str { "Detect repeated tool calls and break loops" }
    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(alva_agent_core::builtins::LoopDetectionMiddleware::new()));
    }
}

/// Dangling tool call validation middleware.
pub struct DanglingToolCallExtension;
#[async_trait]
impl Extension for DanglingToolCallExtension {
    fn name(&self) -> &str { "dangling-tool-call" }
    fn description(&self) -> &str { "Validate tool call format and existence" }
    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(alva_agent_core::builtins::DanglingToolCallMiddleware::new()));
    }
}

/// Tool timeout middleware (120s default).
pub struct ToolTimeoutExtension;
#[async_trait]
impl Extension for ToolTimeoutExtension {
    fn name(&self) -> &str { "tool-timeout" }
    fn description(&self) -> &str { "120s timeout per tool execution" }
    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(alva_agent_core::builtins::ToolTimeoutMiddleware::default()));
    }
}

/// Context compaction middleware.
pub struct CompactionExtension;
#[async_trait]
impl Extension for CompactionExtension {
    fn name(&self) -> &str { "compaction" }
    fn description(&self) -> &str { "Context compaction" }
    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(alva_agent_runtime::middleware::CompactionMiddleware::default()));
    }
}

/// Checkpoint middleware — file backups before tool execution.
pub struct CheckpointExtension;
#[async_trait]
impl Extension for CheckpointExtension {
    fn name(&self) -> &str { "checkpoint" }
    fn description(&self) -> &str { "File checkpoint before tool execution" }
    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(alva_agent_runtime::middleware::CheckpointMiddleware::new()));
    }
}

// ===========================================================================
// PlanMode extension
// ===========================================================================

use alva_agent_runtime::middleware::{PlanModeControl, PlanModeMiddleware};

/// Plan mode extension — blocks non-read-only tools when plan mode is active.
///
/// Runtime toggle is exposed via the bus as `dyn PlanModeControl`, allowing
/// `BaseAgent::set_permission_mode()` to toggle it without a typed reference.
pub struct PlanModeExtension {
    middleware: Arc<PlanModeMiddleware>,
}

impl PlanModeExtension {
    pub fn new() -> Self {
        Self {
            middleware: Arc::new(PlanModeMiddleware::new(false)),
        }
    }
}

#[async_trait]
impl Extension for PlanModeExtension {
    fn name(&self) -> &str { "plan-mode" }
    fn description(&self) -> &str { "Plan mode (read-only tool restriction, runtime toggle)" }

    fn activate(&self, api: &HostAPI) {
        api.middleware(self.middleware.clone());
    }

    async fn configure(&self, ctx: &ExtensionContext) {
        // Register PlanModeControl on bus for runtime toggle
        ctx.bus_writer.provide::<dyn PlanModeControl>(self.middleware.clone());
    }
}

// ===========================================================================
// SubAgent extension
// ===========================================================================

/// Sub-agent spawning via the `agent` tool.
///
/// Uses `finalize()` because it needs the final tool list and model to
/// construct the `SpawnScopeImpl` root scope.
pub struct SubAgentExtension {
    max_depth: u32,
}

impl SubAgentExtension {
    pub fn new(max_depth: u32) -> Self {
        Self { max_depth }
    }
}

#[async_trait]
impl Extension for SubAgentExtension {
    fn name(&self) -> &str { "sub-agents" }
    fn description(&self) -> &str { "Sub-agent spawning via the agent tool" }

    async fn finalize(&self, ctx: &super::FinalizeContext) -> Vec<Arc<dyn Tool>> {
        // Build a clean tool list without any placeholder agent tool
        let tools_without_agent: Vec<Arc<dyn Tool>> = ctx.tools.iter()
            .filter(|t| t.name() != "agent")
            .cloned()
            .collect();

        let root_scope = Arc::new(alva_agent_scope::SpawnScopeImpl::root(
            ctx.model.clone(),
            tools_without_agent,
            std::time::Duration::from_secs(300),
            ctx.max_iterations,
            self.max_depth,
        ));
        let spawn_tool = crate::plugins::agent_spawn::create_agent_spawn_tool(root_scope);
        vec![Arc::from(spawn_tool)]
    }
}

// ===========================================================================
// MCP extension
// ===========================================================================

use alva_protocol_mcp::transport::McpTransport;
use alva_protocol_mcp::error::McpError;
use alva_protocol_mcp::types::McpToolInfo;

use crate::mcp::config::McpConfig;
use crate::mcp::runtime::{McpManager, McpTransportFactory};
use crate::mcp::tool_adapter::build_mcp_tools;
use crate::mcp::tools::McpRuntimeTool;

/// Stub transport factory used when no real MCP transport implementation is
/// available.  Creates transports that immediately fail on connect, so the
/// extension degrades gracefully (tools from unreachable servers are simply
/// omitted).
struct StubTransportFactory;

impl McpTransportFactory for StubTransportFactory {
    fn create(
        &self,
        _config: &alva_protocol_mcp::types::McpServerConfig,
    ) -> Box<dyn McpTransport> {
        Box::new(StubTransport)
    }
}

/// A transport that always fails — used as a placeholder until real stdio/SSE
/// transports are wired in.
struct StubTransport;

#[async_trait]
impl McpTransport for StubTransport {
    async fn connect(&mut self) -> Result<(), McpError> {
        Err(McpError::Transport(
            "no real MCP transport implementation available yet".into(),
        ))
    }
    async fn disconnect(&mut self) -> Result<(), McpError> {
        Ok(())
    }
    fn is_connected(&self) -> bool {
        false
    }
    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError> {
        Ok(vec![])
    }
    async fn call_tool(
        &self,
        _tool_name: &str,
        _arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        Err(McpError::NotConnected("stub transport".into()))
    }
}

/// MCP server integration — discovers and exposes tools from MCP servers.
///
/// During `tools()`, the extension:
/// 1. Loads MCP config from the given paths (global + project `mcp.json`).
/// 2. Creates an [`McpManager`], registers servers, and auto-connects.
/// 3. Wraps discovered MCP tools as standard `Tool` trait objects via
///    [`McpToolAdapter`](crate::mcp::tool_adapter::McpToolAdapter).
/// 4. Provides an [`McpRuntimeTool`] for runtime server management.
///
/// All errors are caught and logged — MCP failures never prevent agent startup.
pub struct McpExtension {
    config_paths: Vec<PathBuf>,
}

impl McpExtension {
    /// Create a new MCP extension that will load config from the given paths.
    ///
    /// Typically called with `[paths.global_mcp_config(), paths.project_mcp_config()]`.
    pub fn new(config_paths: Vec<PathBuf>) -> Self {
        Self { config_paths }
    }
}

#[async_trait]
impl Extension for McpExtension {
    fn name(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        "MCP server integration"
    }

    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        match self.load_and_connect().await {
            Ok(tools) => tools,
            Err(e) => {
                tracing::warn!("MCP extension failed to initialise: {e}");
                vec![]
            }
        }
    }
}

impl McpExtension {
    /// Internal helper: load config, create manager, connect, discover tools.
    async fn load_and_connect(&self) -> Result<Vec<Box<dyn Tool>>, Box<dyn std::error::Error + Send + Sync>> {
        // 1. Load and merge configs from all paths (later paths override earlier).
        let mut merged = McpConfig::default();
        for path in &self.config_paths {
            let cfg = McpConfig::load(path).await?;
            for (id, entry) in cfg.servers {
                merged.servers.insert(id, entry);
            }
        }

        if merged.servers.is_empty() {
            tracing::debug!("MCP: no servers configured — skipping");
            return Ok(vec![]);
        }

        tracing::info!("MCP: {} server(s) configured", merged.servers.len());

        // 2. Create manager with stub factory (will be replaced with real
        //    transport implementations later).
        let factory: Arc<dyn McpTransportFactory> = Arc::new(StubTransportFactory);
        let manager = Arc::new(McpManager::new(factory));

        // 3. Register all servers.
        let server_configs = merged.to_server_configs();
        for cfg in &server_configs {
            manager.register(cfg.clone()).await;
        }

        // 4. Auto-connect servers that have auto_connect = true.
        let errors = manager.connect_auto().await;
        for (id, err) in &errors {
            tracing::warn!("MCP: server '{id}' auto-connect failed: {err}");
        }

        // 5. Discover tools from connected servers.
        let tool_infos = manager.list_all_tools().await;
        tracing::info!("MCP: discovered {} tool(s) from connected servers", tool_infos.len());

        // 6. Wrap MCP tools as standard Tool trait objects.
        let mut tools = build_mcp_tools(manager.clone(), tool_infos);

        // 7. Add the MCP runtime meta-tool for server management.
        tools.push(Box::new(McpRuntimeTool {
            manager: manager.clone(),
        }));

        Ok(tools)
    }
}

// ===========================================================================
// Hooks extension
// ===========================================================================

use std::sync::OnceLock;
use crate::hooks::{HookEvent, HookExecutor, HookInput};
use crate::settings::HooksSettings;
use alva_types::ToolCall;
use alva_types::tool::execution::ToolOutput;

/// Lifecycle hooks as middleware — runs shell scripts at PreToolUse, PostToolUse,
/// SessionStart, and SessionEnd events.
pub struct HooksExtension {
    settings: HooksSettings,
}

impl HooksExtension {
    pub fn new(settings: HooksSettings) -> Self {
        Self { settings }
    }
}

#[async_trait]
impl Extension for HooksExtension {
    fn name(&self) -> &str { "hooks" }
    fn description(&self) -> &str { "Lifecycle hooks (shell scripts at tool/session events)" }

    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(HooksMiddleware {
            settings: self.settings.clone(),
            workspace: OnceLock::new(),
        }));
    }
}

/// Internal middleware that delegates to HookExecutor.
struct HooksMiddleware {
    settings: HooksSettings,
    workspace: OnceLock<PathBuf>,
}

#[async_trait]
impl Middleware for HooksMiddleware {
    fn name(&self) -> &str { "hooks" }

    fn configure(&self, ctx: &alva_agent_core::middleware::MiddlewareContext) {
        if let Some(ref ws) = ctx.workspace {
            let _ = self.workspace.set(ws.clone());
        }
    }

    fn priority(&self) -> i32 {
        // Run after security but before guardrails and most other middleware
        alva_agent_core::shared::MiddlewarePriority::HOOKS
    }

    async fn on_agent_start(&self, _state: &mut alva_agent_core::state::AgentState) -> Result<(), alva_agent_core::shared::MiddlewareError> {
        if let Some(ws) = self.workspace.get() {
            let executor = HookExecutor::new(ws, "session"); // TODO: real session_id
            let input = HookInput::lifecycle(HookEvent::SessionStart, "session", ws);
            let result = executor.run(&self.settings, HookEvent::SessionStart, None, input).await;
            if result.is_blocked() {
                return Err(alva_agent_core::shared::MiddlewareError::Blocked {
                    reason: result.blocking_messages().join("; "),
                });
            }
        }
        Ok(())
    }

    async fn on_agent_end(
        &self,
        _state: &mut alva_agent_core::state::AgentState,
        _error: Option<&str>,
    ) -> Result<(), alva_agent_core::shared::MiddlewareError> {
        if let Some(ws) = self.workspace.get() {
            let executor = HookExecutor::new(ws, "session");
            let input = HookInput::lifecycle(HookEvent::SessionEnd, "session", ws);
            let _ = executor.run(&self.settings, HookEvent::SessionEnd, None, input).await;
        }
        Ok(())
    }

    async fn before_tool_call(
        &self,
        _state: &mut alva_agent_core::state::AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), alva_agent_core::shared::MiddlewareError> {
        if let Some(ws) = self.workspace.get() {
            let executor = HookExecutor::new(ws, "session");
            let input = HookInput::pre_tool_use(
                &tool_call.name,
                tool_call.arguments.clone(),
                "session",
                ws,
            );
            let result = executor.run(
                &self.settings,
                HookEvent::PreToolUse,
                Some(&tool_call.name),
                input,
            ).await;
            if result.is_blocked() {
                return Err(alva_agent_core::shared::MiddlewareError::Blocked {
                    reason: result.blocking_messages().join("; "),
                });
            }
        }
        Ok(())
    }

    async fn after_tool_call(
        &self,
        _state: &mut alva_agent_core::state::AgentState,
        tool_call: &ToolCall,
        tool_output: &mut ToolOutput,
    ) -> Result<(), alva_agent_core::shared::MiddlewareError> {
        if let Some(ws) = self.workspace.get() {
            let executor = HookExecutor::new(ws, "session");
            let response_text = tool_output.model_text();
            let input = HookInput::post_tool_use(
                &tool_call.name,
                tool_call.arguments.clone(),
                &response_text,
                "session",
                ws,
            );
            let _ = executor.run(
                &self.settings,
                HookEvent::PostToolUse,
                Some(&tool_call.name),
                input,
            ).await;
        }
        Ok(())
    }
}

// ===========================================================================
// Skill system extension
// ===========================================================================

use crate::skills::store::SkillStore;
use crate::skills::loader::SkillLoader;
use crate::skills::injector::SkillInjector;
use crate::skills::tools::{SearchSkillsTool, UseSkillTool};
use crate::skills::middleware::SkillInjectionMiddleware;
use crate::skills::skill_fs::FsSkillRepository;
use crate::skills::skill_ports::skill_repository::SkillRepository;

// ===========================================================================
// Analytics, Auth, LSP, Evaluation extensions
// ===========================================================================

/// Telemetry and event tracking.
pub struct AnalyticsExtension {
    log_path: Option<PathBuf>,
}

impl AnalyticsExtension {
    pub fn new(log_path: Option<PathBuf>) -> Self {
        Self { log_path }
    }
}

#[async_trait]
impl Extension for AnalyticsExtension {
    fn name(&self) -> &str { "analytics" }
    fn description(&self) -> &str { "Telemetry and event tracking" }

    async fn configure(&self, ctx: &ExtensionContext) {
        let path = self.log_path.clone()
            .unwrap_or_else(|| ctx.workspace.join(".alva/analytics.jsonl"));
        tracing::debug!(path = %path.display(), "analytics sink configured");
    }
}

/// OAuth authentication and token persistence.
pub struct AuthExtension;

#[async_trait]
impl Extension for AuthExtension {
    fn name(&self) -> &str { "auth" }
    fn description(&self) -> &str { "OAuth authentication and token persistence" }
}

/// Language Server Protocol management.
pub struct LspExtension;

#[async_trait]
impl Extension for LspExtension {
    fn name(&self) -> &str { "lsp" }
    fn description(&self) -> &str { "Language server management and diagnostics" }
}

/// QA evaluation with sprint contract enforcement.
pub struct EvaluationExtension {
    contract: Option<crate::plugins::evaluation::SprintContract>,
}

impl EvaluationExtension {
    pub fn new() -> Self {
        Self { contract: None }
    }

    pub fn with_contract(mut self, contract: crate::plugins::evaluation::SprintContract) -> Self {
        self.contract = Some(contract);
        self
    }
}

#[async_trait]
impl Extension for EvaluationExtension {
    fn name(&self) -> &str { "evaluation" }
    fn description(&self) -> &str { "QA evaluation and sprint contract enforcement" }

    fn activate(&self, api: &HostAPI) {
        if let Some(contract) = &self.contract {
            api.middleware(Arc::new(
                crate::plugins::evaluation::SprintContractMiddleware::new(contract.clone())
            ));
        }
    }
}

/// Skill system: discovery, loading, and context injection.
/// Provides SearchSkillsTool + UseSkillTool and SkillInjectionMiddleware.
pub struct SkillsExtension {
    store: Arc<SkillStore>,
    loader: Arc<SkillLoader>,
    injector: Arc<SkillInjector>,
}

impl SkillsExtension {
    /// Create a new SkillsExtension with the given skill directories.
    /// The first directory is used as primary (bundled/mbb/user subdirs).
    pub fn new(skill_dirs: Vec<PathBuf>) -> Self {
        let primary_dir = skill_dirs.first().cloned()
            .unwrap_or_else(|| PathBuf::from(".alva/skills"));

        let repo = Arc::new(FsSkillRepository::new(
            primary_dir.join("bundled"),
            primary_dir.join("mbb"),
            primary_dir.join("user"),
            primary_dir.join("state.json"),
        ));
        let store = Arc::new(SkillStore::new(repo.clone() as Arc<dyn SkillRepository>));
        let loader = Arc::new(SkillLoader::new(repo.clone() as Arc<dyn SkillRepository>));
        let injector = Arc::new(SkillInjector::new(SkillLoader::new(repo as Arc<dyn SkillRepository>)));

        Self { store, loader, injector }
    }
}

#[async_trait]
impl Extension for SkillsExtension {
    fn name(&self) -> &str { "skills" }
    fn description(&self) -> &str { "Skill discovery, loading, and context injection" }

    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(SearchSkillsTool { store: self.store.clone() }),
            Box::new(UseSkillTool { store: self.store.clone(), loader: self.loader.clone() }),
        ]
    }

    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(SkillInjectionMiddleware::with_defaults(
            self.store.clone(),
            self.injector.clone(),
        )));
    }

    async fn configure(&self, _ctx: &ExtensionContext) {
        let _ = self.store.scan().await;
    }
}
