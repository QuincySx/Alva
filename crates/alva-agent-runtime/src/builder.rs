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

        // Build Arc<dyn Tool> list from registry
        let tools: Vec<Arc<dyn Tool>> = {
            let defs: Vec<String> = registry.definitions().iter().map(|d| d.name.clone()).collect();
            let mut tools_list = Vec::new();
            for name in &defs {
                if let Some(tool) = registry.remove(name) {
                    tools_list.push(Arc::from(tool));
                }
            }
            tools_list
        };

        // Rebuild registry for lookup
        let mut fresh_registry = ToolRegistry::new();
        if self.register_builtin || self.register_browser {
            if self.register_browser {
                alva_agent_tools::register_all_tools(&mut fresh_registry);
            } else {
                alva_agent_tools::register_builtin_tools(&mut fresh_registry);
            }
        }

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
            max_iterations: 100,
        };

        AgentRuntime {
            state,
            config,
            tool_registry: fresh_registry,
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
