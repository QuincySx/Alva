use std::path::PathBuf;
use std::sync::Arc;

use alva_agent_core::middleware::{Middleware, MiddlewareStack};
use alva_agent_core::state::{AgentConfig, AgentState};
use alva_agent_core::shared::Extensions;
use alva_agent_memory::{MemoryService, MemorySqlite, NoopEmbeddingProvider};
use alva_agent_runtime::middleware::security::{ApprovalNotifier, ApprovalRequest};
use alva_agent_runtime::middleware::SecurityMiddleware;
use alva_agent_security::SandboxMode;
use alva_types::{
    Bus, BusPlugin, CancellationToken, LanguageModel,
    PluginRegistrar, Tool, ToolRegistry,
};
use alva_types::session::{AgentSession, InMemorySession};

use crate::error::EngineError;

use tokio::sync::{mpsc, Mutex};

use super::agent::BaseAgent;
use super::permission::PermissionMode;
use crate::extension::Extension;

/// Builder for constructing a [`BaseAgent`].
///
/// By default, `build()` only adds `SecurityMiddleware`. All other tools
/// and middleware come from registered Extensions via `.extension()`.
///
/// ```rust,ignore
/// BaseAgent::builder()
///     .workspace(path)
///     .extension(Box::new(CoreExtension))
///     .extension(Box::new(ShellExtension))
///     .extension(Box::new(LoopDetectionExtension))
///     .extension(Box::new(CompactionExtension))
///     .build(model).await?;
/// ```
pub struct BaseAgentBuilder {
    pub(crate) workspace: Option<PathBuf>,
    pub(crate) system_prompt: String,
    pub(crate) sandbox_mode: SandboxMode,

    // Extensions
    pub(crate) extensions: Vec<Box<dyn Extension>>,
    // Direct tool/middleware (for special cases beyond extensions)
    pub(crate) extra_tools: Vec<Box<dyn Tool>>,
    pub(crate) extra_middleware: Vec<Arc<dyn Middleware>>,
    pub(crate) enable_memory: bool,
    pub(crate) max_iterations: u32,
    pub(crate) context_window: usize,
    pub(crate) approval_notifier: Option<ApprovalNotifier>,
    pub(crate) bus_plugins: Vec<Box<dyn BusPlugin>>,
}

impl BaseAgentBuilder {
    /// Create a new builder with sensible defaults.
    pub fn new() -> Self {
        Self {
            workspace: None,
            system_prompt: "You are a helpful AI assistant.".to_string(),
            sandbox_mode: SandboxMode::RestrictiveOpen,
            extensions: Vec::new(),
            extra_tools: Vec::new(),
            extra_middleware: Vec::new(),
            enable_memory: false,
            max_iterations: 100,
            context_window: 0,
            approval_notifier: None,
            bus_plugins: Vec::new(),
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

    // -- Extensions -----------------------------------------------------------

    /// Register an extension. Extensions contribute tools and/or middleware.
    /// This is the primary way to add capabilities to an agent.
    pub fn extension(mut self, ext: Box<dyn Extension>) -> Self {
        self.extensions.push(ext);
        self
    }

    // -- Direct tool/middleware (internal / eval use) -------------------------

    /// Add tools directly. Prefer `.extension()` for public use.
    pub fn tools(mut self, tools: Vec<Box<dyn Tool>>) -> Self {
        self.extra_tools.extend(tools);
        self
    }

    /// Add a single tool directly. Prefer `.extension()` for public use.
    pub fn tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.extra_tools.push(tool);
        self
    }

    /// Add middleware directly. Prefer `.extension()` for public use.
    pub fn middlewares(mut self, mws: Vec<Arc<dyn Middleware>>) -> Self {
        self.extra_middleware.extend(mws);
        self
    }

    /// Add a single middleware directly. Prefer `.extension()` for public use.
    pub fn middleware(mut self, mw: Arc<dyn Middleware>) -> Self {
        self.extra_middleware.push(mw);
        self
    }

    // -- Features -------------------------------------------------------------

    /// Enable the memory subsystem (SQLite-backed).
    pub fn with_memory(mut self) -> Self {
        self.enable_memory = true;
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

        // 2. Collect tools + middleware from all extensions
        let mut tool_registry = ToolRegistry::new();
        let mut ext_middleware: Vec<Arc<dyn Middleware>> = Vec::new();

        for ext in &self.extensions {
            tracing::info!(extension = ext.name(), "loading extension");
            for tool in ext.tools().await {
                tool_registry.register(tool);
            }
            ext_middleware.extend(ext.middleware().await);
        }

        // 3. Add direct tools (for special cases)
        for tool in self.extra_tools {
            tool_registry.register(tool);
        }

        // 4. Build Arc<dyn Tool> list
        let mut alva_tools_list: Vec<Arc<dyn Tool>> = tool_registry.list_arc();

        // 5. SkillStore is now managed by SkillsExtension

        // 6. Build MiddlewareStack
        let mut middleware_stack = MiddlewareStack::new();

        // Security is always active (bound to workspace + sandbox_mode)
        let security_mw = SecurityMiddleware::for_workspace(&workspace, self.sandbox_mode.clone())
            .with_bus(bus_handle.clone());
        let security_guard = Some(security_mw.guard());
        middleware_stack.push_sorted(Arc::new(security_mw));

        // Extension middleware + direct middleware
        for mw in ext_middleware {
            middleware_stack.push_sorted(mw);
        }
        for mw in self.extra_middleware {
            middleware_stack.push_sorted(mw);
        }

        // Configure all middleware with shared infrastructure (bus, workspace).
        // Middleware that needs bus/workspace grabs it here via configure().
        middleware_stack.configure_all(&alva_agent_core::middleware::MiddlewareContext {
            bus: Some(bus_handle.clone()),
            workspace: Some(workspace.clone()),
        });

        // Configure extensions with full context (bus, workspace, tool names).
        let ext_ctx = crate::extension::ExtensionContext {
            bus: bus_handle.clone(),
            bus_writer: bus_writer.clone(),
            workspace: workspace.clone(),
            tool_names: tool_registry.definitions().iter().map(|d| d.name.clone()).collect(),
        };
        for ext in &self.extensions {
            ext.configure(&ext_ctx).await;
        }

        // 7. Finalize phase — extensions can add tools that depend on the final tool list
        let finalize_ctx = crate::extension::FinalizeContext {
            bus: bus_handle.clone(),
            bus_writer: bus_writer.clone(),
            workspace: workspace.clone(),
            model: model.clone(),
            tools: alva_tools_list.clone(),
            max_iterations: self.max_iterations,
        };
        for ext in &self.extensions {
            let extra = ext.finalize(&finalize_ctx).await;
            alva_tools_list.extend(extra);
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

// middleware_presets removed — use individual middleware extensions instead.

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
            .with_memory()
            .max_iterations(200);

        assert_eq!(builder.workspace, Some(PathBuf::from("/tmp/test")));
        assert_eq!(builder.system_prompt, "Custom prompt");
        assert!(builder.enable_memory);
        assert_eq!(builder.max_iterations, 200);
    }

    #[tokio::test]
    async fn test_build_without_workspace_fails() {
        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("unused")),
        );

        let result = BaseAgent::builder()
            .system_prompt("test")
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
            .extension(Box::new(crate::extension::LoopDetectionExtension))
            .extension(Box::new(crate::extension::DanglingToolCallExtension))
            .extension(Box::new(crate::extension::ToolTimeoutExtension))
            .build(model)
            .await;

        assert!(result.is_ok(), "build with extension should succeed");
    }
}
