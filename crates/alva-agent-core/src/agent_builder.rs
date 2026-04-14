//! AgentBuilder — SDK-level builder that assembles an `Agent` from
//! extensions, tools, middleware, model, and kernel config.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use alva_kernel_abi::session::{AgentSession, InMemorySession};
use alva_kernel_abi::{
    Bus, BusHandle, BusWriter, LanguageModel, ModelConfig, Tool, ToolRegistry,
};
use alva_kernel_core::middleware::{Middleware, MiddlewareStack};
use alva_kernel_core::shared::Extensions;
use alva_kernel_core::state::{AgentConfig, AgentState};
use tokio::sync::Mutex;

use crate::agent::Agent;
use crate::extension::{Extension, ExtensionBridgeMiddleware, ExtensionHost, HostAPI};

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
    pub async fn build(self) -> Result<Agent, alva_kernel_abi::AgentError> {
        // Body filled in Task 1.2.
        todo!("Task 1.2")
    }
}

impl Default for AgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}
