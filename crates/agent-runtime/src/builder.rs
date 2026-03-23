use std::path::PathBuf;
use std::sync::Arc;

use agent_core::middleware::MiddlewareStack;
use agent_core::{Agent, AgentHooks, AgentMessage};
use agent_core::types::AgentContext;
use agent_types::{LanguageModel, Message, ModelConfig, Tool, ToolRegistry};

/// A fully-configured agent runtime combining the Agent and its ToolRegistry.
pub struct AgentRuntime {
    pub agent: Agent,
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
    convert_to_llm: Option<Arc<dyn Fn(&AgentContext<'_>) -> Vec<Message> + Send + Sync>>,
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
            convert_to_llm: None,
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
    pub fn middleware(mut self, mw: Arc<dyn agent_core::middleware::Middleware>) -> Self {
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

    /// Override the default `convert_to_llm` function used by the agent core.
    pub fn convert_to_llm(
        mut self,
        f: Arc<dyn Fn(&AgentContext<'_>) -> Vec<Message> + Send + Sync>,
    ) -> Self {
        self.convert_to_llm = Some(f);
        self
    }

    /// Consume the builder and produce a ready-to-use [`AgentRuntime`].
    ///
    /// `model` is the language model to use for LLM calls.
    pub fn build(self, model: Arc<dyn LanguageModel>) -> AgentRuntime {
        let mut registry = ToolRegistry::new();

        if self.register_builtin || self.register_browser {
            if self.register_browser {
                agent_tools::register_all_tools(&mut registry);
            } else {
                agent_tools::register_builtin_tools(&mut registry);
            }
        }
        for tool in self.custom_tools {
            registry.register(tool);
        }

        let convert_fn = self.convert_to_llm.unwrap_or_else(|| {
            Arc::new(|ctx: &AgentContext<'_>| {
                let mut result = vec![Message::system(ctx.system_prompt)];
                for m in ctx.messages {
                    if let AgentMessage::Standard(msg) = m {
                        result.push(msg.clone());
                    }
                }
                result
            })
        });

        let mut config = AgentHooks::new(convert_fn);
        config.middleware = self.middleware;

        let agent = Agent::new(model, self.system_prompt, config);

        AgentRuntime {
            agent,
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
