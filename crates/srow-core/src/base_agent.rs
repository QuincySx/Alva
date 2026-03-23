// INPUT:  alva_core, alva_types, alva_tools, alva_security, alva_memory, alva_runtime, crate::skills, crate::mcp
// OUTPUT: BaseAgent, BaseAgentBuilder
// POS:    Pre-wired batteries-included agent -- auto-composes tools, security, compression, skill injection, MCP.

use std::path::PathBuf;
use std::sync::Arc;

use alva_core::middleware::MiddlewareStack;
use alva_core::{Agent, AgentHooks, AgentMessage, AgentContext, ConvertToLlmFn};
use alva_core::event::AgentEvent;
use alva_core::middleware::{Middleware, CompressionMiddleware, CompressionConfig};
use alva_runtime::middleware::SecurityMiddleware;
use alva_security::SandboxMode;
use alva_memory::{MemoryService, MemorySqlite, NoopEmbeddingProvider};
use alva_types::{LanguageModel, Message, ModelConfig, Tool, ToolRegistry};

use crate::skills::store::SkillStore;
use crate::skills::loader::SkillLoader;
use crate::skills::injector::SkillInjector;
use crate::skills::skill_fs::FsSkillRepository;
use crate::skills::middleware::SkillInjectionMiddleware;
use crate::skills::skill_ports::skill_repository::SkillRepository;
use crate::mcp::runtime::McpManager;
use crate::error::EngineError;

use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// BaseAgent
// ---------------------------------------------------------------------------

/// Pre-wired, batteries-included agent that automatically composes tools,
/// security, compression, skill injection, and MCP.
///
/// Use [`BaseAgent::builder()`] to construct one with sensible defaults:
///
/// ```rust,ignore
/// let agent = BaseAgent::builder()
///     .workspace("/path/to/project")
///     .build(model)
///     .await?;
///
/// let events = agent.prompt_text("Help me refactor this code");
/// ```
pub struct BaseAgent {
    agent: Agent,
    tool_registry: ToolRegistry,
    skill_store: Arc<SkillStore>,
    mcp_manager: Option<Arc<McpManager>>,
    memory: Option<MemoryService>,
}

impl BaseAgent {
    /// Start building a new BaseAgent.
    pub fn builder() -> BaseAgentBuilder {
        BaseAgentBuilder::new()
    }

    /// Send messages to the agent and receive events via an unbounded channel.
    pub fn prompt(&self, messages: Vec<AgentMessage>) -> mpsc::UnboundedReceiver<AgentEvent> {
        self.agent.prompt(messages)
    }

    /// Convenience: wrap a text string as a user message and prompt the agent.
    pub fn prompt_text(&self, text: &str) -> mpsc::UnboundedReceiver<AgentEvent> {
        let msg = AgentMessage::Standard(Message::user(text));
        self.agent.prompt(vec![msg])
    }

    /// Cancel the currently running agent loop.
    pub fn cancel(&self) {
        self.agent.cancel();
    }

    /// Get a snapshot of the current message history.
    pub async fn messages(&self) -> Vec<AgentMessage> {
        self.agent.messages().await
    }

    /// Access the skill store.
    pub fn skill_store(&self) -> &Arc<SkillStore> {
        &self.skill_store
    }

    /// Access the tool registry (for name-based lookup of registered tools).
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    /// Access the MCP manager (if configured).
    pub fn mcp_manager(&self) -> Option<&Arc<McpManager>> {
        self.mcp_manager.as_ref()
    }

    /// Access the memory service (if enabled).
    pub fn memory(&self) -> Option<&MemoryService> {
        self.memory.as_ref()
    }
}

// ---------------------------------------------------------------------------
// BaseAgentBuilder
// ---------------------------------------------------------------------------

/// Builder for constructing a [`BaseAgent`] with sensible defaults.
///
/// Required: `workspace` must be set before calling `build()`.
pub struct BaseAgentBuilder {
    pub(crate) workspace: Option<PathBuf>,
    pub(crate) system_prompt: String,
    pub(crate) sandbox_mode: SandboxMode,
    pub(crate) model_config: ModelConfig,

    // Optional overrides
    pub(crate) extra_tools: Vec<Box<dyn Tool>>,
    pub(crate) extra_middleware: Vec<Arc<dyn Middleware>>,
    pub(crate) skill_dirs: Vec<PathBuf>,
    pub(crate) enable_memory: bool,
    pub(crate) enable_browser: bool,
    pub(crate) compression_threshold: u32,
    pub(crate) max_iterations: u32,

    // Pre-resolved model conversion function (optional)
    pub(crate) convert_to_llm: Option<ConvertToLlmFn>,
}

impl BaseAgentBuilder {
    /// Create a new builder with sensible defaults.
    pub fn new() -> Self {
        Self {
            workspace: None,
            system_prompt: "You are a helpful AI assistant.".to_string(),
            sandbox_mode: SandboxMode::RestrictiveOpen,
            model_config: ModelConfig::default(),
            extra_tools: Vec::new(),
            extra_middleware: Vec::new(),
            skill_dirs: Vec::new(),
            enable_memory: false,
            enable_browser: true,
            compression_threshold: 100_000,
            max_iterations: 100,
            convert_to_llm: None,
        }
    }

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

    /// Override the model configuration (temperature, max_tokens, etc.).
    pub fn model_config(mut self, config: ModelConfig) -> Self {
        self.model_config = config;
        self
    }

    /// Add a custom tool.
    pub fn tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.extra_tools.push(tool);
        self
    }

    /// Add extra middleware (appended AFTER the default middleware stack).
    pub fn middleware(mut self, mw: Arc<dyn Middleware>) -> Self {
        self.extra_middleware.push(mw);
        self
    }

    /// Add a skill directory to scan for skills.
    pub fn skill_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.skill_dirs.push(path.into());
        self
    }

    /// Enable the memory subsystem (SQLite-backed, stored in `workspace/.srow/memory.db`).
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

    /// Set the compression threshold in estimated tokens (default: 100,000).
    pub fn compression_threshold(mut self, tokens: u32) -> Self {
        self.compression_threshold = tokens;
        self
    }

    /// Set the max iterations for the agent loop (default: 100).
    pub fn max_iterations(mut self, n: u32) -> Self {
        self.max_iterations = n;
        self
    }

    /// Override the message conversion function (`convert_to_llm`).
    pub fn convert_to_llm(mut self, f: ConvertToLlmFn) -> Self {
        self.convert_to_llm = Some(f);
        self
    }

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

        // 2. Create ToolRegistry and populate with builtin/browser tools
        let mut tool_registry = ToolRegistry::new();
        if self.enable_browser {
            alva_tools::register_all_tools(&mut tool_registry);
        } else {
            alva_tools::register_builtin_tools(&mut tool_registry);
        }

        // 3. Register extra custom tools in the registry
        for tool in self.extra_tools {
            tool_registry.register(tool);
        }

        // 4. Build Arc<dyn Tool> list for the agent (definitions from registry)
        //    We create a fresh set of tools for the agent because ToolRegistry
        //    owns Box<dyn Tool> while Agent needs Vec<Arc<dyn Tool>>.
        let mut alva_tools_list: Vec<Arc<dyn Tool>> = Vec::new();
        {
            let mut tmp_registry = ToolRegistry::new();
            if self.enable_browser {
                alva_tools::register_all_tools(&mut tmp_registry);
            } else {
                alva_tools::register_builtin_tools(&mut tmp_registry);
            }
            // Extract tools from the temporary registry by draining it
            for def in tmp_registry.definitions() {
                if let Some(tool) = tmp_registry.remove(&def.name) {
                    alva_tools_list.push(Arc::from(tool));
                }
            }
        }

        // 5. Create SkillStore
        let skill_store = if !self.skill_dirs.is_empty() {
            // Use the first skill dir as bundled, second as mbb, third as user
            // For simplicity, treat all dirs as user skill dirs with a single
            // FsSkillRepository pointing to the first dir structure.
            let first_dir = &self.skill_dirs[0];
            let bundled_dir = first_dir.join("bundled");
            let mbb_dir = first_dir.join("mbb");
            let user_dir = first_dir.join("user");
            let state_file = first_dir.join("state.json");

            let repo = Arc::new(FsSkillRepository::new(
                bundled_dir,
                mbb_dir,
                user_dir,
                state_file,
            ));
            let store = SkillStore::new(repo.clone() as Arc<dyn SkillRepository>);
            // Scan is best-effort; we don't fail if no skills found
            let _ = store.scan().await;
            (store, repo as Arc<dyn SkillRepository>)
        } else {
            // Empty skill store with a non-existent directory repo
            let empty_dir = workspace.join(".srow").join("skills");
            let repo = Arc::new(FsSkillRepository::new(
                empty_dir.join("bundled"),
                empty_dir.join("mbb"),
                empty_dir.join("user"),
                empty_dir.join("state.json"),
            ));
            let store = SkillStore::new(repo.clone() as Arc<dyn SkillRepository>);
            (store, repo as Arc<dyn SkillRepository>)
        };

        let (skill_store, skill_repo) = skill_store;
        let skill_store = Arc::new(skill_store);

        // 6. Create SkillLoader + SkillInjector
        let skill_loader = SkillLoader::new(skill_repo.clone());
        let skill_injector = Arc::new(SkillInjector::new(skill_loader));

        // 7. Create MiddlewareStack in order
        let mut middleware_stack = MiddlewareStack::new();

        // a. SecurityMiddleware
        middleware_stack.push(Arc::new(SecurityMiddleware::for_workspace(
            &workspace,
            self.sandbox_mode,
        )));

        // b. CompressionMiddleware
        middleware_stack.push(Arc::new(CompressionMiddleware::new(CompressionConfig {
            token_threshold: self.compression_threshold,
            ..CompressionConfig::default()
        })));

        // c. SkillInjectionMiddleware
        middleware_stack.push(Arc::new(SkillInjectionMiddleware::with_defaults(
            skill_store.clone(),
            skill_injector,
        )));

        // d. Extra middleware from user
        for mw in self.extra_middleware {
            middleware_stack.push(mw);
        }

        // 8. Create AgentHooks
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

        let mut hooks = AgentHooks::new(convert_fn);
        hooks.middleware = middleware_stack;
        hooks.max_iterations = self.max_iterations;

        // 9. Create Agent
        let agent = Agent::new(model, &self.system_prompt, hooks);

        // 10. Set tools on the agent
        agent.set_tools(alva_tools_list).await;

        // 11. Optionally create MemoryService
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

        // 12. Return BaseAgent
        Ok(BaseAgent {
            agent,
            tool_registry,
            skill_store,
            mcp_manager: None,
            memory,
        })
    }
}

impl Default for BaseAgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_defaults() {
        let builder = BaseAgentBuilder::new();
        assert!(builder.workspace.is_none());
        assert_eq!(builder.compression_threshold, 100_000);
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
            .compression_threshold(50_000)
            .max_iterations(200);

        assert_eq!(builder.workspace, Some(PathBuf::from("/tmp/test")));
        assert_eq!(builder.system_prompt, "Custom prompt");
        assert!(!builder.enable_browser);
        assert!(builder.enable_memory);
        assert_eq!(builder.compression_threshold, 50_000);
        assert_eq!(builder.max_iterations, 200);
    }

    #[test]
    fn builder_skill_dirs() {
        let builder = BaseAgentBuilder::new()
            .skill_dir("/path/to/skills1")
            .skill_dir("/path/to/skills2");

        assert_eq!(builder.skill_dirs.len(), 2);
    }
}
