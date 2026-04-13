// INPUT:  std::path, std::sync, alva_kernel_core, alva_kernel_abi, alva_agent_tools
// OUTPUT: AgentRuntime, AgentRuntimeBuilder
// POS:    Builder pattern for constructing a fully-configured AgentRuntime with state, config, tools, and middleware.
use std::path::PathBuf;
use std::sync::Arc;

use alva_kernel_core::builtins::{DanglingToolCallMiddleware, LoopDetectionMiddleware, ToolTimeoutMiddleware};
use alva_kernel_core::middleware::MiddlewareStack;
use alva_kernel_core::pending_queue::{AgentLoopHook, PendingMessageQueue};
use alva_kernel_core::state::{AgentConfig, AgentState};
use alva_kernel_core::shared::Extensions;
use alva_agent_security::{SandboxMode, SecurityGuard};
use alva_kernel_abi::{
    model::HeuristicTokenCounter, Bus, BusHandle, BusPlugin, BusWriter, LanguageModel, ModelConfig,
    PluginRegistrar, Tool, ToolRegistry, TokenCounter,
};
use alva_kernel_abi::session::{AgentSession, InMemorySession};
use tokio::sync::Mutex;

use crate::middleware::{
    ApprovalNotifier, CheckpointMiddleware, CompactionMiddleware, PlanModeMiddleware,
    SecurityMiddleware,
};

/// A fully-configured agent runtime combining AgentState, AgentConfig, and ToolRegistry.
pub struct AgentRuntime {
    pub state: AgentState,
    pub config: AgentConfig,
    pub tool_registry: ToolRegistry,
    pub bus: BusHandle,
    pub bus_writer: Option<BusWriter>,
    pub pending_messages: Option<Arc<PendingMessageQueue>>,
    pub plan_mode_middleware: Option<Arc<PlanModeMiddleware>>,
    pub security_guard: Option<Arc<Mutex<SecurityGuard>>>,
}

/// Builder for constructing an [`AgentRuntime`] step by step.
pub struct AgentRuntimeBuilder {
    system_prompt: String,
    workspace: Option<PathBuf>,
    model_config: ModelConfig,
    middleware: MiddlewareStack,
    register_builtin: bool,
    register_browser: bool,
    custom_tools: Vec<Box<dyn Tool>>,
    max_iterations: u32,
    context_window: usize,
    bus: Option<BusHandle>,
    bus_writer: Option<BusWriter>,
    approval_notifier: Option<ApprovalNotifier>,
    bus_plugins: Vec<Box<dyn BusPlugin>>,
    standard_agent_stack: Option<SandboxMode>,
    context_system: Option<Arc<alva_kernel_abi::scope::context::ContextSystem>>,
}

impl AgentRuntimeBuilder {
    pub fn new() -> Self {
        Self {
            system_prompt: String::new(),
            workspace: None,
            model_config: ModelConfig::default(),
            middleware: MiddlewareStack::new(),
            register_builtin: false,
            register_browser: false,
            custom_tools: Vec::new(),
            max_iterations: 100,
            context_window: 0,
            bus: None,
            bus_writer: None,
            approval_notifier: None,
            bus_plugins: Vec::new(),
            standard_agent_stack: None,
            context_system: None,
        }
    }

    /// Attach a `ContextSystem` to the runtime. When set, the kernel's
    /// `run_agent` loop fires `ContextHooks::{bootstrap, on_message,
    /// after_turn, dispose}` at the matching lifecycle points. None means
    /// no context plugins are wired (default).
    pub fn with_context_system(
        mut self,
        cs: Arc<alva_kernel_abi::scope::context::ContextSystem>,
    ) -> Self {
        self.context_system = Some(cs);
        self
    }

    /// Set the system prompt for the agent.
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    /// Set the workspace root directory.
    pub fn workspace(mut self, path: impl Into<PathBuf>) -> Self {
        self.workspace = Some(path.into());
        self
    }

    /// Set the model configuration (temperature, max_tokens, etc.).
    pub fn model_config(mut self, config: ModelConfig) -> Self {
        self.model_config = config;
        self
    }

    /// Add a middleware layer to the stack.
    pub fn middleware(mut self, mw: Arc<dyn alva_kernel_core::middleware::Middleware>) -> Self {
        self.middleware.push(mw);
        self
    }

    /// Register all standard built-in tools (shell, file edit, grep, etc.).
    pub fn with_builtin_tools(mut self) -> Self {
        self.register_builtin = true;
        self
    }

    /// Register browser tools in addition to the standard built-in tools.
    pub fn with_browser_tools(mut self) -> Self {
        self.register_browser = true;
        self
    }

    /// Set the max iterations for the agent loop (default: 100).
    pub fn max_iterations(mut self, n: u32) -> Self {
        self.max_iterations = n;
        self
    }

    /// Set the context window size (default: 0 = no limit).
    /// When > 0, only the most recent N messages are included in LLM context.
    pub fn context_window(mut self, n: usize) -> Self {
        self.context_window = n;
        self
    }

    /// Reuse an externally created bus handle instead of creating a fresh bus.
    pub fn with_bus(mut self, bus: BusHandle) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Reuse an externally created bus writer so runtime assembly can register capabilities.
    pub fn with_bus_writer(mut self, bus_writer: BusWriter) -> Self {
        self.bus = Some(bus_writer.handle());
        self.bus_writer = Some(bus_writer);
        self
    }

    /// Register a notifier used by SecurityMiddleware when human approval is required.
    pub fn with_approval_notifier(mut self, notifier: ApprovalNotifier) -> Self {
        self.approval_notifier = Some(notifier);
        self
    }

    /// Register a bus plugin to run during standard agent stack initialization.
    pub fn bus_plugin(mut self, plugin: Box<dyn BusPlugin>) -> Self {
        self.bus_plugins.push(plugin);
        self
    }

    /// Enable the standard batteries-included agent stack on top of core runtime pieces.
    ///
    /// This wires:
    /// - a default heuristic token counter
    /// - PendingMessageQueue as AgentLoopHook
    /// - Security / loop detection / timeout / compaction / plan / checkpoint middleware
    /// - optional approval notifier and bus plugins
    pub fn with_standard_agent_stack(mut self, sandbox_mode: SandboxMode) -> Self {
        self.standard_agent_stack = Some(sandbox_mode);
        self
    }

    /// Register a single custom tool.
    pub fn tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.custom_tools.push(tool);
        self
    }

    /// Consume the builder and produce a ready-to-use [`AgentRuntime`].
    ///
    /// `model` is the language model to use for LLM calls.
    pub fn build(self, model: Arc<dyn LanguageModel>) -> AgentRuntime {
        let (bus, bus_writer) = if let Some(writer) = self.bus_writer.clone() {
            (writer.handle(), Some(writer))
        } else if let Some(bus) = self.bus.clone() {
            (bus, None)
        } else {
            let bus = Bus::new();
            (bus.handle(), Some(bus.writer()))
        };

        let mut middleware = self.middleware;
        let mut pending_messages: Option<Arc<PendingMessageQueue>> = None;
        let mut plan_mode_middleware: Option<Arc<PlanModeMiddleware>> = None;
        let mut security_guard: Option<Arc<Mutex<SecurityGuard>>> = None;

        if let Some(sandbox_mode) = self.standard_agent_stack.clone() {
            let workspace = self
                .workspace
                .clone()
                .expect("standard agent stack requires workspace() to be set");
            let writer = bus_writer
                .clone()
                .expect("standard agent stack requires BusWriter; use default bus or with_bus_writer()");

            if !bus.has::<dyn TokenCounter>() {
                writer.provide::<dyn TokenCounter>(Arc::new(HeuristicTokenCounter::new(200_000)));
            }

            if let Some(notifier) = self.approval_notifier.clone() {
                writer.provide(Arc::new(notifier));
            }

            let queue = Arc::new(PendingMessageQueue::new());
            writer.provide::<dyn AgentLoopHook>(queue.clone() as Arc<dyn AgentLoopHook>);
            pending_messages = Some(queue);

            for plugin in &self.bus_plugins {
                let mut registrar = PluginRegistrar::new(&writer, plugin.name());
                plugin.register(&mut registrar);
                tracing::info!(
                    plugin = plugin.name(),
                    capabilities = ?registrar.registered_capabilities(),
                    "bus plugin registered"
                );
            }

            for plugin in &self.bus_plugins {
                plugin.start(&bus);
            }

            let security_mw = SecurityMiddleware::for_workspace(&workspace, sandbox_mode)
                .with_bus(bus.clone());
            security_guard = Some(security_mw.guard());
            middleware.push_sorted(Arc::new(security_mw));
            middleware.push_sorted(Arc::new(DanglingToolCallMiddleware::new()));
            middleware.push_sorted(Arc::new(LoopDetectionMiddleware::new()));
            middleware.push_sorted(Arc::new(ToolTimeoutMiddleware::default()));
            middleware.push_sorted(Arc::new(
                CompactionMiddleware::default().with_bus(bus.clone()),
            ));

            let plan_mw = Arc::new(PlanModeMiddleware::new(false));
            middleware.push_sorted(plan_mw.clone());
            middleware.push_sorted(Arc::new(CheckpointMiddleware::new().with_bus(bus.clone())));
            plan_mode_middleware = Some(plan_mw);
        }

        let mut registry = ToolRegistry::new();

        if self.register_builtin || self.register_browser {
            if self.register_browser {
                alva_agent_tools::register_all_tools(&mut registry);
            } else {
                alva_agent_tools::register_builtin_tools(&mut registry);
            }
        }
        for tool in self.custom_tools {
            registry.register(tool);
        }

        // Extract tools as Arc references — registry and state share the same instances.
        let tools: Vec<Arc<dyn Tool>> = registry.list_arc();

        let session: Arc<dyn AgentSession> = Arc::new(InMemorySession::new());

        let state = AgentState {
            model,
            tools,
            session,
            extensions: Extensions::new(),
        };

        let config = AgentConfig {
            middleware,
            system_prompt: self.system_prompt,
            max_iterations: self.max_iterations,
            model_config: self.model_config,
            context_window: self.context_window,
            workspace: self.workspace,
            bus: Some(bus.clone()),
            context_system: self.context_system,
        };

        AgentRuntime {
            state,
            config,
            tool_registry: registry,
            bus,
            bus_writer,
            pending_messages,
            plan_mode_middleware,
            security_guard,
        }
    }
}

impl AgentRuntime {
    /// Start building a new runtime via the builder API.
    pub fn builder() -> AgentRuntimeBuilder {
        AgentRuntimeBuilder::new()
    }
}

impl Default for AgentRuntimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::{AgentError, BusPlugin, CompletionResponse, Message, ModelConfig, StreamEvent};
    use async_trait::async_trait;
    use futures_core::Stream;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio_stream::empty;

    struct DummyModel;

    #[async_trait]
    impl LanguageModel for DummyModel {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Result<CompletionResponse, AgentError> {
            Err(AgentError::Other("not used in builder tests".into()))
        }

        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
            Box::pin(empty())
        }

        fn model_id(&self) -> &str {
            "dummy-model"
        }
    }

    #[test]
    fn build_wires_a_default_bus_handle() {
        let runtime = AgentRuntime::builder().build(Arc::new(DummyModel));
        assert!(runtime.config.bus.is_some());
    }

    #[test]
    fn builder_accepts_an_external_bus_handle() {
        let bus = Bus::new();
        let bus_handle = bus.handle();

        let runtime = AgentRuntime::builder()
            .with_bus(bus_handle.clone())
            .build(Arc::new(DummyModel));

        assert!(runtime.config.bus.is_some());
        let configured = runtime.config.bus.expect("bus should be configured");
        assert!(!configured.has::<u32>());

        bus.writer().provide(Arc::new(7_u32));
        assert_eq!(*configured.require::<u32>(), 7);
    }

    #[test]
    fn standard_stack_wires_core_agent_capabilities() {
        let runtime = AgentRuntime::builder()
            .workspace("/tmp/runtime-standard")
            .with_standard_agent_stack(SandboxMode::RestrictiveOpen)
            .build(Arc::new(DummyModel));

        assert!(runtime.bus_writer.is_some());
        assert!(runtime.pending_messages.is_some());
        assert!(runtime.plan_mode_middleware.is_some());
        assert!(runtime.security_guard.is_some());
        assert!(runtime.bus.has::<dyn TokenCounter>());
        assert!(runtime.bus.has::<dyn AgentLoopHook>());
        assert!(runtime.config.middleware.len() >= 7);
    }

    #[test]
    fn standard_stack_registers_approval_notifier() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let runtime = AgentRuntime::builder()
            .workspace("/tmp/runtime-approval")
            .with_approval_notifier(ApprovalNotifier { tx })
            .with_standard_agent_stack(SandboxMode::RestrictiveOpen)
            .build(Arc::new(DummyModel));

        assert!(runtime.bus.has::<ApprovalNotifier>());
    }

    #[test]
    fn standard_stack_starts_bus_plugins() {
        static STARTED: AtomicBool = AtomicBool::new(false);

        struct TestPlugin;
        impl BusPlugin for TestPlugin {
            fn name(&self) -> &str {
                "test-plugin"
            }

            fn register(&self, _registrar: &mut PluginRegistrar) {}

            fn start(&self, _bus: &BusHandle) {
                STARTED.store(true, Ordering::SeqCst);
            }
        }

        STARTED.store(false, Ordering::SeqCst);
        let _runtime = AgentRuntime::builder()
            .workspace("/tmp/runtime-plugin")
            .bus_plugin(Box::new(TestPlugin))
            .with_standard_agent_stack(SandboxMode::RestrictiveOpen)
            .build(Arc::new(DummyModel));

        assert!(STARTED.load(Ordering::SeqCst));
    }
}
