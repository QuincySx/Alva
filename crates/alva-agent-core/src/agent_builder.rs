//! AgentBuilder — SDK-level builder that assembles an `Agent` from
//! extensions, tools, middleware, model, and kernel config.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use alva_kernel_abi::agent_session::{AgentSession, InMemoryAgentSession};
use alva_kernel_abi::{
    AgentError, Bus, BusHandle, BusWriter, LanguageModel, ModelConfig, Tool, ToolRegistry,
};
use alva_kernel_core::middleware::{Middleware, MiddlewareContext, MiddlewareStack};
use alva_kernel_core::shared::Extensions;
use alva_kernel_core::state::{AgentConfig, AgentState};
use tokio::sync::Mutex;

use crate::agent::Agent;
use crate::extension::{
    Extension, ExtensionBridgeMiddleware, ExtensionContext, ExtensionHost, FinalizeContext,
    HostAPI,
};

/// SDK-level builder for assembling an `Agent`.
///
/// This is the layer at which `alva-agent-core` assembles an agent without
/// any harness-level opinions. Callers (third-party harnesses or tests)
/// compose their own model, extensions, and middleware here. Opinionated
/// wrappers like `alva_app_core::BaseAgentBuilder` delegate to this.
pub struct AgentBuilder {
    model: Option<Arc<dyn LanguageModel>>,
    system_prompt: String,
    workspace: Option<PathBuf>,
    model_config: ModelConfig,
    max_iterations: u32,
    context_window: usize,

    extensions: Vec<Box<dyn Extension>>,
    extra_tools: Vec<Box<dyn Tool>>,
    extra_middleware: Vec<Arc<dyn Middleware>>,

    bus: Option<BusHandle>,
    bus_writer: Option<BusWriter>,
    session: Option<Arc<dyn AgentSession>>,

    context_system: Option<Arc<alva_kernel_abi::scope::context::ContextSystem>>,
    context_token_budget: Option<usize>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model: None,
            system_prompt: String::new(),
            workspace: None,
            model_config: ModelConfig::default(),
            max_iterations: 100,
            context_window: 0,
            extensions: Vec::new(),
            extra_tools: Vec::new(),
            extra_middleware: Vec::new(),
            bus: None,
            bus_writer: None,
            session: None,
            context_system: None,
            context_token_budget: None,
        }
    }

    pub fn model(mut self, m: Arc<dyn LanguageModel>) -> Self {
        self.model = Some(m);
        self
    }
    pub fn system_prompt(mut self, s: impl Into<String>) -> Self {
        self.system_prompt = s.into();
        self
    }
    pub fn workspace(mut self, p: impl Into<PathBuf>) -> Self {
        self.workspace = Some(p.into());
        self
    }
    pub fn model_config(mut self, cfg: ModelConfig) -> Self {
        self.model_config = cfg;
        self
    }
    pub fn max_iterations(mut self, n: u32) -> Self {
        self.max_iterations = n;
        self
    }
    pub fn context_window(mut self, n: usize) -> Self {
        self.context_window = n;
        self
    }
    pub fn extension(mut self, e: Box<dyn Extension>) -> Self {
        self.extensions.push(e);
        self
    }
    pub fn tool(mut self, t: Box<dyn Tool>) -> Self {
        self.extra_tools.push(t);
        self
    }
    pub fn middleware(mut self, mw: Arc<dyn Middleware>) -> Self {
        self.extra_middleware.push(mw);
        self
    }
    pub fn with_bus(mut self, bus: BusHandle) -> Self {
        self.bus = Some(bus);
        self
    }
    pub fn with_bus_writer(mut self, bw: BusWriter) -> Self {
        self.bus = Some(bw.handle());
        self.bus_writer = Some(bw);
        self
    }
    pub fn session(mut self, s: Arc<dyn AgentSession>) -> Self {
        self.session = Some(s);
        self
    }
    pub fn with_context_system(
        mut self,
        cs: Arc<alva_kernel_abi::scope::context::ContextSystem>,
    ) -> Self {
        self.context_system = Some(cs);
        self
    }
    pub fn with_context_token_budget(mut self, budget: usize) -> Self {
        self.context_token_budget = Some(budget);
        self
    }

    /// Build the Agent. Runs the extension lifecycle
    /// (`tools` → `activate` → `configure` → `finalize`), wires middleware,
    /// and produces a ready-to-run `Agent`.
    pub async fn build(self) -> Result<Agent, AgentError> {
        // 1. Validate required inputs.
        let model = self
            .model
            .ok_or_else(|| AgentError::Other("AgentBuilder requires a model".into()))?;

        // 2. Set up the bus. Prefer a caller-supplied writer (so the caller
        //    can register capabilities on it). If only a `BusHandle` was
        //    provided we use that as the routing handle but still spin a
        //    fresh writer for the contexts — capability `provide()` calls
        //    made on that writer will not be visible on the caller's bus,
        //    which is a documented caveat of the handle-only path.
        //    Otherwise create a fresh in-process Bus.
        let (bus, bus_writer): (BusHandle, BusWriter) = if let Some(writer) = self.bus_writer {
            (writer.handle(), writer)
        } else if let Some(handle) = self.bus {
            let fresh = Bus::new();
            (handle, fresh.writer())
        } else {
            let fresh = Bus::new();
            (fresh.handle(), fresh.writer())
        };

        // 3. Create the ExtensionHost.
        let host = Arc::new(RwLock::new(ExtensionHost::new()));

        // 4. Collect tools from every extension (extensions own the
        //    primary contribution path) and append the builder's direct
        //    tools afterwards.
        let mut all_tools: Vec<Box<dyn Tool>> = Vec::new();
        for ext in &self.extensions {
            let tools = ext.tools().await;
            all_tools.extend(tools);
        }
        all_tools.extend(self.extra_tools);

        // 5. Activate extensions. They register middleware / event
        //    handlers / commands via HostAPI here.
        for ext in &self.extensions {
            let api = HostAPI::new(host.clone(), ext.name().to_string());
            ext.activate(&api);
        }

        // 6. Build the middleware stack: middlewares the extensions
        //    registered during activate, then user-supplied extras, then the
        //    bridge that routes lifecycle events into the ExtensionHost.
        let mut middleware_stack = MiddlewareStack::new();
        {
            let mut host_mut = host.write().unwrap();
            for mw in host_mut.take_middlewares() {
                middleware_stack.push_sorted(mw);
            }
        }
        for mw in self.extra_middleware {
            middleware_stack.push_sorted(mw);
        }
        middleware_stack.push_sorted(Arc::new(ExtensionBridgeMiddleware::new(host.clone())));

        // 7. Register the (so-far) collected tools into a ToolRegistry so
        //    the configure phase can show extensions the registered names.
        let mut registry = ToolRegistry::new();
        for tool in all_tools {
            registry.register(tool);
        }

        // 8. Configure phase: extensions inspect bus + workspace + the set
        //    of registered tool names.
        //
        //    `ExtensionContext.workspace` is non-Option, so when the caller
        //    didn't set one we default to `PathBuf::new()` (i.e. the empty
        //    path). Extensions that need a real workspace must check.
        let workspace_for_ctx = self.workspace.clone().unwrap_or_default();
        let ext_ctx = ExtensionContext {
            bus: bus.clone(),
            bus_writer: bus_writer.clone(),
            workspace: workspace_for_ctx.clone(),
            tool_names: registry
                .definitions()
                .iter()
                .map(|d| d.name.clone())
                .collect(),
        };
        for ext in &self.extensions {
            ext.configure(&ext_ctx).await;
        }

        // 9. Configure middleware that needs shared infrastructure.
        middleware_stack.configure_all(&MiddlewareContext {
            bus: Some(bus.clone()),
            workspace: self.workspace.clone(),
            session: None, // TODO(phase-2): per-middleware scoped session
        });

        // 10. Finalize phase: extensions can return additional tools that
        //     depend on seeing the final tool list. We hand them an Arc
        //     snapshot of the registry; any returned tools are folded back
        //     through the registry so name collisions dedupe the same way
        //     as the `tools()` path (last write wins).
        let finalize_ctx = FinalizeContext {
            bus: bus.clone(),
            bus_writer: bus_writer.clone(),
            workspace: workspace_for_ctx,
            model: model.clone(),
            tools: registry.list_arc(),
            max_iterations: self.max_iterations,
        };
        for ext in &self.extensions {
            for tool in ext.finalize(&finalize_ctx).await {
                registry.register_arc(tool);
            }
        }
        let tools_arc: Vec<Arc<dyn Tool>> = registry.list_arc();

        // 11. Session — default to in-memory if not provided.
        let session: Arc<dyn AgentSession> = self
            .session
            .unwrap_or_else(|| Arc::new(InMemoryAgentSession::new()));

        // 12. AgentState
        let state = AgentState {
            model,
            tools: tools_arc,
            session,
            extensions: Extensions::new(),
        };

        // 13. AgentConfig
        let config = AgentConfig {
            middleware: middleware_stack,
            system_prompt: self.system_prompt,
            max_iterations: self.max_iterations,
            model_config: self.model_config,
            context_window: self.context_window,
            workspace: self.workspace,
            bus: Some(bus.clone()),
            context_system: self.context_system,
            context_token_budget: self.context_token_budget,
        };

        // 14. Wrap up. The bus_writer is intentionally dropped here — any
        //     capabilities the caller / extensions wanted to register on it
        //     have already been published, and the run loop only needs the
        //     read-side `BusHandle`.
        drop(bus_writer);

        // Snapshot tools for cheap external inspection (BaseAgent, tests).
        let tools_snapshot = {
            let st = state.tools.clone();
            st
        };

        Ok(Agent {
            state: Mutex::new(state),
            config,
            bus,
            host,
            tools: tools_snapshot,
        })
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
