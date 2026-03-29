// INPUT:  alva_agent_core (V2: AgentState, AgentConfig, run_agent, Middleware, MiddlewareStack, Extensions),
//         alva_types, alva_agent_tools, alva_agent_security, alva_agent_memory, alva_agent_runtime
// OUTPUT: BaseAgent, BaseAgentBuilder
// POS:    Pre-wired batteries-included agent using V2 engine — auto-composes tools, security, skill injection.

use std::path::PathBuf;
use std::sync::Arc;

use alva_agent_core::middleware::{Middleware, MiddlewareStack};
use alva_agent_core::state::{AgentConfig, AgentState};
use alva_agent_core::run::run_agent;
use alva_agent_core::event::AgentEvent;
use alva_agent_core::shared::Extensions;
use alva_agent_memory::{MemoryService, MemorySqlite, NoopEmbeddingProvider};
use alva_agent_security::SandboxMode;
use alva_types::{
    AgentMessage, CancellationToken, LanguageModel, Message, Tool, ToolRegistry,
};
use alva_types::session::{AgentSession, InMemorySession};

use crate::skills::store::SkillStore;
use crate::skills::skill_fs::FsSkillRepository;
use crate::skills::skill_ports::skill_repository::SkillRepository;
use crate::error::EngineError;

use tokio::sync::{mpsc, Mutex};

// ---------------------------------------------------------------------------
// BaseAgent
// ---------------------------------------------------------------------------

/// Pre-wired, batteries-included agent (V2 engine) that automatically composes
/// tools, security, and skill injection.
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
    state: Arc<Mutex<AgentState>>,
    config: Arc<AgentConfig>,
    cancel: CancellationToken,
    tool_registry: ToolRegistry,
    skill_store: Arc<SkillStore>,
    memory: Option<MemoryService>,
}

impl BaseAgent {
    /// Start building a new BaseAgent.
    pub fn builder() -> BaseAgentBuilder {
        BaseAgentBuilder::new()
    }

    /// Convenience: wrap a text string as a user message and prompt the agent.
    pub fn prompt_text(&self, text: &str) -> mpsc::UnboundedReceiver<AgentEvent> {
        let msg = AgentMessage::Standard(Message::user(text));
        self.prompt(vec![msg])
    }

    /// Send messages to the agent and receive events via an unbounded channel.
    pub fn prompt(&self, messages: Vec<AgentMessage>) -> mpsc::UnboundedReceiver<AgentEvent> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let state = self.state.clone();
        let config = self.config.clone();
        let cancel = self.cancel.clone();

        tokio::spawn(async move {
            let mut st = state.lock().await;
            if let Err(e) = run_agent(
                &mut st,
                &config,
                cancel,
                messages,
                event_tx.clone(),
            )
            .await
            {
                tracing::error!(error = %e, "agent loop failed");
            }
        });

        event_rx
    }

    /// Cancel the currently running agent loop.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Get a snapshot of the current message history.
    pub async fn messages(&self) -> Vec<AgentMessage> {
        let st = self.state.lock().await;
        st.session.messages()
    }

    /// Restore message history (e.g., when resuming a session).
    pub async fn restore_messages(&self, messages: Vec<AgentMessage>) {
        let st = self.state.lock().await;
        // Clear existing messages by creating a fresh session is complex,
        // so we append to the session. For full restore, we clear + re-add.
        // InMemorySession doesn't have a clear method, so we work with what we have.
        for msg in messages {
            st.session.append(msg);
        }
    }

    /// Access the skill store.
    pub fn skill_store(&self) -> &Arc<SkillStore> {
        &self.skill_store
    }

    /// Access the tool registry (for name-based lookup of registered tools).
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
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

    // Optional overrides
    pub(crate) extra_tools: Vec<Box<dyn Tool>>,
    pub(crate) extra_middleware: Vec<Arc<dyn Middleware>>,
    pub(crate) skill_dirs: Vec<PathBuf>,
    pub(crate) enable_memory: bool,
    pub(crate) enable_browser: bool,
    pub(crate) enable_sub_agents: bool,
    pub(crate) sub_agent_max_depth: u32,
    pub(crate) max_iterations: u32,
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

    /// Enable the `agent` tool (sub-agent spawning). Default: off.
    ///
    /// Sub-agents can spawn further sub-agents up to `max_depth` levels.
    /// Default max depth: 3.
    pub fn with_sub_agents(mut self) -> Self {
        self.enable_sub_agents = true;
        self
    }

    /// Set the maximum sub-agent nesting depth (default: 3).
    /// Only meaningful when sub-agents are enabled.
    pub fn sub_agent_max_depth(mut self, depth: u32) -> Self {
        self.sub_agent_max_depth = depth;
        self
    }

    /// Set the max iterations for the agent loop (default: 100).
    pub fn max_iterations(mut self, n: u32) -> Self {
        self.max_iterations = n;
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

        // 2. Create ToolRegistry (for name-based lookup) and populate with builtin/browser tools
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

        // 4. Build Arc<dyn Tool> list for the agent by draining the registry.
        let mut alva_tools_list: Vec<Arc<dyn Tool>> = Vec::new();
        {
            let defs: Vec<String> = tool_registry.definitions().iter().map(|d| d.name.clone()).collect();
            for name in &defs {
                if let Some(tool) = tool_registry.remove(name) {
                    alva_tools_list.push(Arc::from(tool));
                }
            }
        }
        // Rebuild the registry so BaseAgent.tool_registry remains usable for
        // name-based lookup (definitions only — these are separate instances).
        {
            let mut fresh_registry = ToolRegistry::new();
            if self.enable_browser {
                alva_agent_tools::register_all_tools(&mut fresh_registry);
            } else {
                alva_agent_tools::register_builtin_tools(&mut fresh_registry);
            }
            tool_registry = fresh_registry;
        }

        // 5. Create SkillStore
        let skill_store = if !self.skill_dirs.is_empty() {
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
            let _ = store.scan().await;
            store
        } else {
            let empty_dir = workspace.join(".srow").join("skills");
            let repo = Arc::new(FsSkillRepository::new(
                empty_dir.join("bundled"),
                empty_dir.join("mbb"),
                empty_dir.join("user"),
                empty_dir.join("state.json"),
            ));
            SkillStore::new(repo.clone() as Arc<dyn SkillRepository>)
        };

        let skill_store = Arc::new(skill_store);

        // 6. Build V2 MiddlewareStack
        let mut middleware_stack = MiddlewareStack::new();

        // a. Builtin: DanglingToolCallMiddleware
        middleware_stack.push(Arc::new(
            alva_agent_core::builtins::DanglingToolCallMiddleware::new(),
        ));

        // b. Builtin: LoopDetectionMiddleware
        middleware_stack.push(Arc::new(
            alva_agent_core::builtins::LoopDetectionMiddleware::new(),
        ));

        // c. Extra middleware from user
        for mw in self.extra_middleware {
            middleware_stack.push(mw);
        }

        // 7. Optionally add the agent spawn tool
        if self.enable_sub_agents {
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
        let state = AgentState {
            model,
            tools: alva_tools_list,
            session,
            extensions: Extensions::new(),
        };

        // 9. Create V2 AgentConfig
        let config = AgentConfig {
            middleware: middleware_stack,
            system_prompt: self.system_prompt,
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
            cancel: CancellationToken::new(),
            tool_registry,
            skill_store,
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

    // -----------------------------------------------------------------------
    // Integration tests using alva-test mocks
    // -----------------------------------------------------------------------

    use alva_test::fixtures::make_assistant_message;
    use alva_test::mock_provider::MockLanguageModel;
    use alva_agent_core::event::AgentEvent;

    /// Helper: build a BaseAgent with minimal config using a mock model.
    async fn build_test_agent(model: Arc<dyn alva_types::LanguageModel>) -> BaseAgent {
        let tmp = tempfile::tempdir().expect("failed to create tempdir");
        BaseAgent::builder()
            .workspace(tmp.path())
            .system_prompt("You are a test agent.")
            .without_browser()
            .build(model)
            .await
            .expect("build should succeed")
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
    async fn test_base_agent_prompt_produces_events() {
        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("Hello from mock!")),
        );

        let agent = build_test_agent(model).await;
        let mut rx = agent.prompt_text("hi");

        let mut got_agent_start = false;
        let mut got_agent_end = false;
        let mut got_message_start = false;
        let mut got_message_end = false;

        while let Some(event) = rx.recv().await {
            match &event {
                AgentEvent::AgentStart => got_agent_start = true,
                AgentEvent::AgentEnd { .. } => {
                    got_agent_end = true;
                    break;
                }
                AgentEvent::MessageStart { .. } => got_message_start = true,
                AgentEvent::MessageEnd { .. } => got_message_end = true,
                _ => {}
            }
        }

        assert!(got_agent_start, "should receive AgentStart event");
        assert!(got_message_start, "should receive MessageStart event");
        assert!(got_message_end, "should receive MessageEnd event");
        assert!(got_agent_end, "should receive AgentEnd event");
    }

    #[tokio::test]
    async fn test_base_agent_prompt_text_ends_without_error() {
        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("All good!")),
        );

        let agent = build_test_agent(model).await;
        let mut rx = agent.prompt_text("Tell me something.");

        let mut end_error: Option<Option<String>> = None;
        while let Some(event) = rx.recv().await {
            if let AgentEvent::AgentEnd { error } = event {
                end_error = Some(error);
                break;
            }
        }

        let error = end_error.expect("should receive AgentEnd");
        assert!(error.is_none(), "AgentEnd should have no error, got: {:?}", error);
    }

    #[tokio::test]
    async fn test_base_agent_messages_after_prompt() {
        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("Response text")),
        );

        let agent = build_test_agent(model).await;
        let mut rx = agent.prompt_text("hello");

        // Drain all events until AgentEnd
        while let Some(event) = rx.recv().await {
            if matches!(event, AgentEvent::AgentEnd { .. }) {
                break;
            }
        }

        let messages = agent.messages().await;
        // Should contain at least the user message and assistant message
        assert!(
            messages.len() >= 2,
            "expected at least 2 messages (user + assistant), got {}",
            messages.len()
        );
    }

    #[tokio::test]
    async fn test_base_agent_with_custom_tool() {
        use alva_test::mock_tool::MockTool;
        use alva_test::fixtures::make_tool_call_message;
        use alva_types::tool::ToolResult;

        // The model will first return a tool call, then a final text response.
        let tool_call_resp = make_tool_call_message(
            "my_test_tool",
            serde_json::json!({"key": "value"}),
        );
        let final_resp = make_assistant_message("Done using the tool.");

        let mock_model = MockLanguageModel::new()
            .with_response(tool_call_resp)
            .with_response(final_resp);
        let model = Arc::new(mock_model);

        let mock_tool = MockTool::new("my_test_tool")
            .with_result(ToolResult {
                content: "tool executed".into(),
                is_error: false,
                details: None,
            });
        let mock_tool_clone = mock_tool.clone();

        let tmp = tempfile::tempdir().expect("tempdir");
        let agent = BaseAgent::builder()
            .workspace(tmp.path())
            .system_prompt("You are a test agent.")
            .without_browser()
            .tool(Box::new(mock_tool))
            .build(model)
            .await
            .expect("build should succeed");

        let mut rx = agent.prompt_text("Use the tool please.");

        let mut got_tool_exec_start = false;
        let mut got_tool_exec_end = false;
        let mut got_agent_end = false;

        while let Some(event) = rx.recv().await {
            match &event {
                AgentEvent::ToolExecutionStart { tool_call } => {
                    assert_eq!(tool_call.name, "my_test_tool");
                    got_tool_exec_start = true;
                }
                AgentEvent::ToolExecutionEnd { tool_call, result } => {
                    assert_eq!(tool_call.name, "my_test_tool");
                    assert_eq!(result.content, "tool executed");
                    assert!(!result.is_error);
                    got_tool_exec_end = true;
                }
                AgentEvent::AgentEnd { error } => {
                    assert!(error.is_none(), "AgentEnd should have no error");
                    got_agent_end = true;
                    break;
                }
                _ => {}
            }
        }

        assert!(got_tool_exec_start, "should receive ToolExecutionStart");
        assert!(got_tool_exec_end, "should receive ToolExecutionEnd");
        assert!(got_agent_end, "should receive AgentEnd");

        // Verify the mock tool actually received the call
        let calls = mock_tool_clone.calls();
        assert_eq!(calls.len(), 1, "tool should have been called exactly once");
        assert_eq!(calls[0], serde_json::json!({"key": "value"}));
    }

    #[tokio::test]
    async fn test_base_agent_tool_registry_has_builtin_tools() {
        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("unused")),
        );

        let tmp = tempfile::tempdir().expect("tempdir");
        let agent = BaseAgent::builder()
            .workspace(tmp.path())
            .without_browser()
            .build(model)
            .await
            .expect("build should succeed");

        // Without browser, we should still have builtin tools registered.
        let defs = agent.tool_registry().definitions();
        assert!(
            !defs.is_empty(),
            "tool registry should contain builtin tools"
        );

        // Verify some expected builtins exist
        let names: Vec<String> = defs.iter().map(|d| d.name.clone()).collect();
        assert!(names.iter().any(|n| n == "execute_shell" || n == "shell" || n.contains("shell")),
            "should have a shell tool in builtins, got: {:?}", names);
    }
}
