use std::path::PathBuf;
use std::sync::Arc;

use alva_kernel_core::middleware::Middleware;
use alva_agent_memory::{MemoryService, NoopEmbeddingProvider};
use alva_host_native::middleware::{ApprovalNotifier, ApprovalRequest};
use alva_host_native::middleware::SecurityMiddleware;
use alva_agent_security::SandboxMode;
use alva_kernel_abi::{Bus, BusPlugin, CancellationToken, LanguageModel, PluginRegistrar, Tool};

use crate::error::EngineError;

use tokio::sync::mpsc;

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
    pub(crate) memory_service_override: Option<MemoryService>,
    pub(crate) security_middleware_override: Option<Arc<dyn Middleware>>,
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
            memory_service_override: None,
            security_middleware_override: None,
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

    /// Enable the memory subsystem with the default pure in-memory backend.
    /// Use `.memory_service(...)` to swap in a persistent backend.
    pub fn with_memory(mut self) -> Self {
        self.enable_memory = true;
        self
    }

    /// Inject a pre-constructed `MemoryService` (overrides the default
    /// `InMemoryBackend`-backed construction). Implies `enable_memory = true`.
    pub fn memory_service(mut self, service: MemoryService) -> Self {
        self.memory_service_override = Some(service);
        self.enable_memory = true;
        self
    }

    /// Inject a custom middleware in place of the default
    /// `SecurityMiddleware::for_workspace(workspace, sandbox_mode)`. Use this
    /// when you want fine-grained control over sandboxing or are running in
    /// an environment where the built-in sandbox doesn't apply (tests,
    /// in-process harness, etc).
    pub fn security_middleware(mut self, mw: Arc<dyn Middleware>) -> Self {
        self.security_middleware_override = Some(mw);
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
        // 1. Validate workspace.
        let workspace = self
            .workspace
            .ok_or_else(|| EngineError::ToolExecution("workspace is required".into()))?;

        // 2. Create the coordination bus. We keep our own writer alive for
        //    the lifetime of BaseAgent so the harness can register
        //    capabilities post-build (e.g. checkpoint callbacks).
        let bus = Bus::new();
        let bus_writer = bus.writer();
        let bus_handle = bus.handle();

        // 3. Pre-build wiring on the bus (BEFORE delegating to AgentBuilder
        //    so that any extension `configure()` running inside the builder
        //    can already see these capabilities).

        // 3a. Default token counter.
        bus_writer.provide::<dyn alva_kernel_abi::TokenCounter>(Arc::new(
            alva_kernel_abi::model::HeuristicTokenCounter::new(200_000),
        ));

        // 3b. Approval notifier (only if the caller wired one).
        if let Some(notifier) = self.approval_notifier {
            bus_writer.provide(Arc::new(notifier));
        }

        // 3c. PendingMessageQueue + AgentLoopHook. We need to keep
        //     `pending_messages` outside the builder because BaseAgent's
        //     `steer()` / `follow_up()` push into it directly.
        let pending_messages = Arc::new(alva_kernel_core::pending_queue::PendingMessageQueue::new());
        bus_writer.provide::<dyn alva_kernel_core::pending_queue::AgentLoopHook>(
            pending_messages.clone() as Arc<dyn alva_kernel_core::pending_queue::AgentLoopHook>,
        );

        // 4. Build the security middleware (harness preset, overridable).
        //    It needs to see the bus_handle so its guard can publish events.
        let mut security_guard = None;
        let security_mw: Arc<dyn Middleware> = match self.security_middleware_override {
            Some(mw) => mw,
            None => {
                let default = SecurityMiddleware::for_workspace(&workspace, self.sandbox_mode.clone())
                    .with_bus(bus_handle.clone());
                security_guard = Some(default.guard());
                Arc::new(default)
            }
        };

        // 5. Compose the inner alva_agent_core::AgentBuilder. The generic
        //    extension lifecycle (tools/activate/configure/finalize),
        //    middleware stack assembly, and AgentState/AgentConfig wiring
        //    all live inside its `build()`.
        let mut agent_builder = alva_agent_core::AgentBuilder::new()
            .model(model)
            .system_prompt(self.system_prompt)
            .workspace(workspace.clone())
            .max_iterations(self.max_iterations)
            .context_window(self.context_window)
            .with_bus_writer(bus_writer.clone());

        // 5a. Trace each extension we hand off.
        for ext in self.extensions {
            tracing::info!(extension = ext.name(), "loading extension");
            agent_builder = agent_builder.extension(ext);
        }

        // 5b. Direct tools (e.g. mock tools in tests).
        for tool in self.extra_tools {
            agent_builder = agent_builder.tool(tool);
        }

        // 5c. Security middleware first, then any caller-supplied extras.
        //     The inner builder uses `push_sorted`, so global ordering still
        //     respects each middleware's `priority()`.
        agent_builder = agent_builder.middleware(security_mw);
        for mw in self.extra_middleware {
            agent_builder = agent_builder.middleware(mw);
        }

        // 6. Delegate the generic build.
        let inner = agent_builder
            .build()
            .await
            .map_err(|e| EngineError::ToolExecution(format!("agent build failed: {e}")))?;

        // 7. Post-build harness wiring. The extension host now exists and
        //    extensions have already been activated/configured. We need to
        //    bind the cancellation token + pending messages so the host can
        //    cancel the loop and inject steering messages.
        let current_cancel = Arc::new(std::sync::Mutex::new(CancellationToken::new()));
        {
            let mut host = inner.host().write().unwrap();
            host.bind_agent(pending_messages.clone(), current_cancel.clone());
        }

        // 8. Register bus plugins (harness-specific, not exposed by SDK).
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

        // 9. Build a tool registry snapshot for `BaseAgent::tool_registry()`.
        //    The inner Agent already cached the tools list, but downstream
        //    callers expect the `ToolRegistry` shape (definitions/list).
        let mut tool_registry = alva_kernel_abi::ToolRegistry::new();
        for tool in inner.tools() {
            tool_registry.register_arc(tool.clone());
        }

        // 10. Optionally create MemoryService (harness concern).
        let memory = if let Some(service) = self.memory_service_override {
            Some(service)
        } else if self.enable_memory {
            // Default: pure in-memory backend. Zero external deps, no filesystem.
            // Users who want SQLite (or any other backend) should call
            // `.memory_service(...)` with their own MemoryService.
            let store = alva_agent_memory::InMemoryBackend::new();
            let embedder = Box::new(NoopEmbeddingProvider::new());
            Some(MemoryService::with_backend(std::sync::Arc::new(store), embedder))
        } else {
            None
        };

        // 11. Return BaseAgent wrapping the inner Agent.
        Ok(BaseAgent {
            inner: Arc::new(inner),
            current_cancel,
            permission_mode: std::sync::Mutex::new(PermissionMode::Ask),
            tool_registry,
            memory,
            security_guard,
            pending_messages,
            bus_writer,
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
