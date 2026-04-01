// INPUT:  std::path, std::sync, alva_agent_core (V2), alva_types, alva_agent_tools
// OUTPUT: AgentRuntime, AgentRuntimeBuilder
// POS:    Builder pattern for constructing a fully-configured AgentRuntime with V2 state, config, tools, and middleware.
use std::path::PathBuf;
use std::sync::Arc;

use alva_agent_core::middleware::MiddlewareStack;
use alva_agent_core::state::{AgentConfig, AgentState};
use alva_agent_core::shared::Extensions;
use alva_types::{LanguageModel, ModelConfig, Tool, ToolRegistry};
use alva_types::session::{AgentSession, InMemorySession};

/// A fully-configured agent runtime combining V2 AgentState, AgentConfig, and ToolRegistry.
pub struct AgentRuntime {
    pub state: AgentState,
    pub config: AgentConfig,
    pub tool_registry: ToolRegistry,
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
        }
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
    pub fn middleware(mut self, mw: Arc<dyn alva_agent_core::middleware::Middleware>) -> Self {
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

    /// Register a single custom tool.
    pub fn tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.custom_tools.push(tool);
        self
    }

    /// Consume the builder and produce a ready-to-use [`AgentRuntime`].
    ///
    /// `model` is the language model to use for LLM calls.
    pub fn build(self, model: Arc<dyn LanguageModel>) -> AgentRuntime {
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
            middleware: self.middleware,
            system_prompt: self.system_prompt,
            max_iterations: self.max_iterations,
            model_config: self.model_config,
            context_window: self.context_window,
            loop_hook: None,
            workspace: self.workspace,
        };

        AgentRuntime {
            state,
            config,
            tool_registry: registry,
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
