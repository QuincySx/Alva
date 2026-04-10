use std::path::PathBuf;
use std::sync::Arc;

use alva_agent_core::middleware::{Middleware, MiddlewareStack};
use alva_agent_core::state::{AgentConfig, AgentState};
use alva_agent_core::shared::Extensions;
use alva_agent_memory::{MemoryService, MemorySqlite, NoopEmbeddingProvider};
use alva_agent_runtime::middleware::security::{ApprovalNotifier, ApprovalRequest};
use alva_agent_runtime::middleware::{CheckpointMiddleware, PlanModeMiddleware, SecurityMiddleware};
use alva_agent_security::SandboxMode;
use alva_types::{
    Bus, BusPlugin, CancellationToken, LanguageModel,
    PluginRegistrar, Tool, ToolRegistry,
};
use alva_types::session::{AgentSession, InMemorySession};

use crate::skills::store::SkillStore;
use crate::skills::skill_fs::FsSkillRepository;
use crate::skills::skill_ports::skill_repository::SkillRepository;
use crate::error::EngineError;

use tokio::sync::{mpsc, Mutex};

use super::agent::BaseAgent;
use super::permission::PermissionMode;

/// Builder for constructing a [`BaseAgent`] with sensible defaults.
///
/// # Middleware
///
/// By default, `build()` adds the full production middleware stack
/// (Security, LoopDetection, DanglingToolCall, ToolTimeout, Compaction,
/// PlanMode, Checkpoint). Use `.bare()` to skip all defaults and
/// register only what you need via `.middleware()` / `.middlewares()`.
///
/// ```rust,ignore
/// // Full production stack (default):
/// BaseAgent::builder().workspace(path).build(model).await?;
///
/// // Bare — only what you register:
/// BaseAgent::builder()
///     .workspace(path)
///     .bare()
///     .middleware(Arc::new(LoopDetectionMiddleware::new()))
///     .build(model).await?;
/// ```
pub struct BaseAgentBuilder {
    pub(crate) workspace: Option<PathBuf>,
    pub(crate) system_prompt: String,
    pub(crate) sandbox_mode: SandboxMode,

    // Optional overrides
    pub(crate) extra_tools: Vec<Box<dyn Tool>>,
    pub(crate) extra_middleware: Vec<Arc<dyn Middleware>>,
    pub(crate) skill_dirs: Vec<PathBuf>,
    pub(crate) enable_memory: bool,
    pub(crate) enable_browser: bool,
    pub(crate) enable_sub_agents: bool,
    pub(crate) sub_agent_max_depth: u32,
    pub(crate) max_iterations: u32,
    pub(crate) context_window: usize,
    pub(crate) approval_notifier: Option<ApprovalNotifier>,
    pub(crate) bus_plugins: Vec<Box<dyn BusPlugin>>,

    // Typed reference: Security needs guard for resolve_permission()
    pub(crate) security_guard: Option<Arc<Mutex<alva_agent_security::SecurityGuard>>>,

}

impl BaseAgentBuilder {
    /// Create a new builder with sensible defaults.
    pub fn new() -> Self {
        Self {
            workspace: None,
            system_prompt: "You are a helpful AI assistant.".to_string(),
            sandbox_mode: SandboxMode::RestrictiveOpen,
            extra_tools: Vec::new(),
            extra_middleware: Vec::new(),
            skill_dirs: Vec::new(),
            enable_memory: false,
            enable_browser: true,
            enable_sub_agents: false,
            sub_agent_max_depth: 3,
            max_iterations: 100,
            context_window: 0,
            approval_notifier: None,
            bus_plugins: Vec::new(),
            security_guard: None,
        }
    }

    // -- Core configuration ---------------------------------------------------

    /// Set the workspace root directory (required).
    pub fn workspace(mut self, path: impl Into<PathBuf>) -> Self {
        self.workspace = Some(path.into());
        self
    }

    /// Override the default system prompt.
    pub fn system_prompt(mut self, text: impl Into<String>) -> Self {
        self.system_prompt = text.into();
        self
    }

    /// Override the sandbox mode (default: `RestrictiveOpen`).
    pub fn sandbox_mode(mut self, mode: SandboxMode) -> Self {
        self.sandbox_mode = mode;
        self
    }

    // -- Tools ----------------------------------------------------------------

    /// Add a custom tool.
    pub fn tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.extra_tools.push(tool);
        self
    }

    // -- Middleware ------------------------------------------------------------

    /// Add a single middleware.
    pub fn middleware(mut self, mw: Arc<dyn Middleware>) -> Self {
        self.extra_middleware.push(mw);
        self
    }

    /// Add multiple middleware at once.
    pub fn middlewares(mut self, mws: Vec<Arc<dyn Middleware>>) -> Self {
        self.extra_middleware.extend(mws);
        self
    }


    // -- Features -------------------------------------------------------------

    /// Add a skill directory to scan for skills.
    pub fn skill_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.skill_dirs.push(path.into());
        self
    }

    /// Enable the memory subsystem (SQLite-backed).
    pub fn with_memory(mut self) -> Self {
        self.enable_memory = true;
        self
    }

    /// Enable browser tools (default: enabled).
    pub fn with_browser(mut self) -> Self {
        self.enable_browser = true;
        self
    }

    /// Disable browser tools.
    pub fn without_browser(mut self) -> Self {
        self.enable_browser = false;
        self
    }

    /// Enable the `agent` tool (sub-agent spawning). Default: off.
    pub fn with_sub_agents(mut self) -> Self {
        self.enable_sub_agents = true;
        self
    }

    /// Set the maximum sub-agent nesting depth (default: 3).
    pub fn sub_agent_max_depth(mut self, depth: u32) -> Self {
        self.sub_agent_max_depth = depth;
        self
    }

    /// Set the max iterations for the agent loop (default: 100).
    pub fn max_iterations(mut self, n: u32) -> Self {
        self.max_iterations = n;
        self
    }

    /// Set the context window size (default: 0 = no limit).
    pub fn context_window(mut self, n: usize) -> Self {
        self.context_window = n;
        self
    }

    /// Set up an approval channel for interactive permission prompts.
    pub fn with_approval_channel(&mut self) -> mpsc::UnboundedReceiver<ApprovalRequest> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.approval_notifier = Some(ApprovalNotifier { tx });
        rx
    }

    /// Add a bus plugin that will register capabilities during build.
    pub fn bus_plugin(mut self, plugin: Box<dyn BusPlugin>) -> Self {
        self.bus_plugins.push(plugin);
        self
    }

    // -- Build ----------------------------------------------------------------

    /// Consume the builder and produce a ready-to-use [`BaseAgent`].
    ///
    /// # Errors
    ///
    /// Returns `EngineError` if workspace is not set or if memory initialization fails.
    pub async fn build(self, model: Arc<dyn LanguageModel>) -> Result<BaseAgent, EngineError> {
        // 1. Validate workspace
        let workspace = self
            .workspace
            .ok_or_else(|| EngineError::ToolExecution("workspace is required".into()))?;

        // 1b. Create the coordination bus
        let bus = Bus::new();
        let bus_writer = bus.writer();
        let bus_handle = bus.handle();

        bus_writer.provide::<dyn alva_types::TokenCounter>(
            Arc::new(alva_types::model::HeuristicTokenCounter::new(200_000))
        );

        // 2. Create ToolRegistry and populate with builtin/browser tools
        let mut tool_registry = ToolRegistry::new();
        if self.enable_browser {
            alva_agent_tools::register_all_tools(&mut tool_registry);
        } else {
            alva_agent_tools::register_builtin_tools(&mut tool_registry);
        }

        // 3. Register extra custom tools in the registry
        for tool in self.extra_tools {
            tool_registry.register(tool);
        }

        // 4. Build Arc<dyn Tool> list
        let mut alva_tools_list: Vec<Arc<dyn Tool>> = tool_registry.list_arc();

        // 5. Create SkillStore
        let skill_store = {
            let primary_dir = if !self.skill_dirs.is_empty() {
                self.skill_dirs[0].clone()
            } else {
                workspace.join(".alva").join("skills")
            };

            let repo = Arc::new(FsSkillRepository::new(
                primary_dir.join("bundled"),
                primary_dir.join("mbb"),
                primary_dir.join("user"),
                primary_dir.join("state.json"),
            ));
            let store = SkillStore::new(repo.clone() as Arc<dyn SkillRepository>);
            let _ = store.scan().await;
            store
        };

        let skill_store = Arc::new(skill_store);

        // 6. Build MiddlewareStack
        let mut middleware_stack = MiddlewareStack::new();

        // Security is always active (bound to workspace + sandbox_mode)
        let security_mw = SecurityMiddleware::for_workspace(&workspace, self.sandbox_mode.clone())
            .with_bus(bus_handle.clone());
        let security_guard = Some(security_mw.guard());
        middleware_stack.push_sorted(Arc::new(security_mw));

        // Caller-registered middleware
        for mw in self.extra_middleware {
            middleware_stack.push_sorted(mw);
        }

        // Configure all middleware with shared infrastructure (bus, workspace).
        // Middleware that needs bus/workspace grabs it here via configure().
        middleware_stack.configure_all(&alva_agent_core::middleware::MiddlewareContext {
            bus: Some(bus_handle.clone()),
            workspace: Some(workspace.clone()),
        });

        // 7. Optionally add the agent spawn tool (replaces the placeholder from builtins)
        if self.enable_sub_agents {
            // Remove the placeholder AgentTool registered by register_builtin_tools()
            alva_tools_list.retain(|t| t.name() != "agent");

            let root_scope = Arc::new(alva_agent_scope::SpawnScopeImpl::root(
                model.clone(),
                alva_tools_list.clone(),
                std::time::Duration::from_secs(300),
                self.max_iterations,
                self.sub_agent_max_depth,
            ));
            let spawn_tool = crate::plugins::agent_spawn::create_agent_spawn_tool(root_scope);
            alva_tools_list.push(Arc::from(spawn_tool));
        }

        // 8. Create V2 AgentState
        let session: Arc<dyn AgentSession> = Arc::new(InMemorySession::new());
        if let Some(notifier) = self.approval_notifier {
            bus_writer.provide(Arc::new(notifier));
        }
        let extensions = Extensions::new();
        let state = AgentState {
            model,
            tools: alva_tools_list,
            session,
            extensions,
        };

        // 9. Create PendingMessageQueue + V2 AgentConfig
        let pending_messages = Arc::new(alva_agent_core::pending_queue::PendingMessageQueue::new());
        bus_writer.provide::<dyn alva_agent_core::pending_queue::AgentLoopHook>(
            pending_messages.clone() as Arc<dyn alva_agent_core::pending_queue::AgentLoopHook>,
        );

        // Register bus plugins
        for plugin in &self.bus_plugins {
            let mut registrar = PluginRegistrar::new(&bus_writer, plugin.name());
            plugin.register(&mut registrar);
            tracing::info!(
                plugin = plugin.name(),
                capabilities = ?registrar.registered_capabilities(),
                "bus plugin registered"
            );
        }

        for plugin in &self.bus_plugins {
            plugin.start(&bus_handle);
        }

        let config = AgentConfig {
            middleware: middleware_stack,
            system_prompt: self.system_prompt,
            max_iterations: self.max_iterations,
            model_config: alva_types::ModelConfig::default(),
            context_window: self.context_window,
            workspace: Some(workspace.clone()),
            bus: Some(bus_handle.clone()),
        };

        // 10. Optionally create MemoryService
        let memory = if self.enable_memory {
            let db_dir = workspace.join(".srow");
            tokio::fs::create_dir_all(&db_dir).await?;
            let db_path = db_dir.join("memory.db");
            let store = MemorySqlite::open(&db_path).await?;
            let embedder = Box::new(NoopEmbeddingProvider::new());
            Some(MemoryService::new(store, embedder))
        } else {
            None
        };

        // 11. Return BaseAgent
        Ok(BaseAgent {
            state: Arc::new(Mutex::new(state)),
            config: Arc::new(config),
            current_cancel: std::sync::Mutex::new(CancellationToken::new()),
            permission_mode: std::sync::Mutex::new(PermissionMode::Ask),
            tool_registry,
            skill_store,
            memory,
            security_guard,
            pending_messages,
            bus_writer,
            bus: bus_handle,
        })
    }
}

impl Default for BaseAgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Middleware presets — common middleware combinations
// ---------------------------------------------------------------------------

/// Pre-built middleware sets for common use cases.
///
/// Security middleware is always added by `build()` and is NOT included in presets.
pub mod middleware_presets {
    use super::*;

    /// Minimal guardrails: loop detection + dangling tool call validation.
    pub fn guardrails() -> Vec<Arc<dyn Middleware>> {
        vec![
            Arc::new(alva_agent_core::builtins::LoopDetectionMiddleware::new()),
            Arc::new(alva_agent_core::builtins::DanglingToolCallMiddleware::new()),
        ]
    }

    /// Guardrails + tool timeout (120s default).
    pub fn guardrails_with_timeout() -> Vec<Arc<dyn Middleware>> {
        let mut mws = guardrails();
        mws.push(Arc::new(alva_agent_core::builtins::ToolTimeoutMiddleware::default()));
        mws
    }

    /// Full production stack: guardrails + timeout + compaction + checkpoint + plan mode.
    /// Middleware that needs bus receives it via `configure()` at build time.
    pub fn production() -> Vec<Arc<dyn Middleware>> {
        vec![
            Arc::new(alva_agent_core::builtins::LoopDetectionMiddleware::new()),
            Arc::new(alva_agent_core::builtins::DanglingToolCallMiddleware::new()),
            Arc::new(alva_agent_core::builtins::ToolTimeoutMiddleware::default()),
            Arc::new(alva_agent_runtime::middleware::CompactionMiddleware::default()),
            Arc::new(CheckpointMiddleware::new()),
            Arc::new(alva_agent_runtime::middleware::PlanModeMiddleware::new(false)),
        ]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use alva_test::fixtures::make_assistant_message;
    use alva_test::mock_provider::MockLanguageModel;

    #[test]
    fn builder_defaults() {
        let builder = BaseAgentBuilder::new();
        assert!(builder.workspace.is_none());
        assert!(builder.enable_browser);
        assert!(!builder.enable_memory);
        assert_eq!(builder.max_iterations, 100);
        assert_eq!(builder.system_prompt, "You are a helpful AI assistant.");
    }

    #[test]
    fn builder_fluent_api() {
        let builder = BaseAgentBuilder::new()
            .workspace("/tmp/test")
            .system_prompt("Custom prompt")
            .sandbox_mode(SandboxMode::RestrictiveOpen)
            .without_browser()
            .with_memory()
            .max_iterations(200);

        assert_eq!(builder.workspace, Some(PathBuf::from("/tmp/test")));
        assert_eq!(builder.system_prompt, "Custom prompt");
        assert!(!builder.enable_browser);
        assert!(builder.enable_memory);
        assert_eq!(builder.max_iterations, 200);
    }

    #[test]
    fn builder_skill_dirs() {
        let builder = BaseAgentBuilder::new()
            .skill_dir("/path/to/skills1")
            .skill_dir("/path/to/skills2");

        assert_eq!(builder.skill_dirs.len(), 2);
    }

    #[tokio::test]
    async fn test_build_without_workspace_fails() {
        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("unused")),
        );

        let result = BaseAgent::builder()
            .system_prompt("test")
            .without_browser()
            .build(model)
            .await;

        assert!(result.is_err(), "build without workspace should fail");
    }

    #[tokio::test]
    async fn test_build_with_workspace_succeeds() {
        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("unused")),
        );

        let tmp = tempfile::tempdir().expect("tempdir");
        let result = BaseAgent::builder()
            .workspace(tmp.path())
            .without_browser()
            .build(model)
            .await;

        assert!(result.is_ok(), "build with workspace should succeed");
    }

    #[tokio::test]
    async fn test_build_with_preset_succeeds() {
        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("unused")),
        );

        let tmp = tempfile::tempdir().expect("tempdir");
        let result = BaseAgent::builder()
            .workspace(tmp.path())
            .without_browser()
            .middlewares(middleware_presets::guardrails())
            .build(model)
            .await;

        assert!(result.is_ok(), "build with preset should succeed");
    }
}
