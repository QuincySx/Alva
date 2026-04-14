use std::sync::Arc;

use alva_agent_core::Agent;
use alva_agent_memory::MemoryService;
use alva_host_native::middleware::PlanModeControl;
use alva_kernel_abi::{
    AgentMessage, BusHandle, BusWriter, CancellationToken, Message, ToolRegistry,
};
use alva_kernel_core::event::AgentEvent;
use alva_kernel_core::run::run_agent;

use tokio::sync::{mpsc, Mutex};

use super::builder::BaseAgentBuilder;
use super::permission::PermissionMode;

/// Pre-wired, batteries-included agent (engine) that automatically composes
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
    /// Inner SDK-level agent. Holds the assembled state, config, bus, and
    /// extension host. The harness fields below live alongside it.
    pub(super) inner: Arc<Agent>,
    /// Holds the CancellationToken for the currently running prompt() call.
    /// Uses std::sync::Mutex (not tokio) because it is only held briefly.
    /// Wrapped in Arc so the ExtensionHost can also hold a reference for shutdown().
    pub(super) current_cancel: Arc<std::sync::Mutex<CancellationToken>>,
    pub(super) permission_mode: std::sync::Mutex<PermissionMode>,
    /// Snapshot of the registered tools, exposed via `tool_registry()`.
    pub(super) tool_registry: ToolRegistry,
    pub(super) memory: Option<MemoryService>,
    pub(super) security_guard: Option<Arc<Mutex<alva_agent_security::SecurityGuard>>>,
    /// Pending messages queue — bridges external steer/follow_up calls
    /// to the agent loop via AgentLoopHook.
    pub(super) pending_messages: Arc<alva_kernel_core::pending_queue::PendingMessageQueue>,
    /// Init-phase bus writer — retained for post-init registration (e.g., checkpoint callback).
    pub(super) bus_writer: BusWriter,
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
    ///
    /// A fresh `CancellationToken` is created for each call so that a
    /// previously-cancelled token does not block future prompts.
    pub fn prompt(&self, messages: Vec<AgentMessage>) -> mpsc::UnboundedReceiver<AgentEvent> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let cancel = CancellationToken::new();
        {
            let mut current = self.current_cancel.lock().unwrap_or_else(|e| e.into_inner());
            *current = cancel.clone();
        }

        let inner = self.inner.clone();

        tokio::spawn(async move {
            let mut st = inner.state().lock().await;
            if let Err(e) = run_agent(
                &mut st,
                inner.config(),
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
        let current = self.current_cancel.lock().unwrap_or_else(|e| e.into_inner());
        current.cancel();
    }

    /// Inject a steering message mid-turn.
    ///
    /// The message is delivered after the current tool execution completes,
    /// before the next LLM call. Replaces any previously queued steering message.
    pub fn steer(&self, text: &str) {
        self.pending_messages
            .steer(AgentMessage::Steering(Message::user(text)));
    }

    /// Queue a follow-up message.
    ///
    /// Delivered after the agent finishes its current turn naturally (no more tool calls).
    /// Multiple follow-ups accumulate and are processed in order.
    pub fn follow_up(&self, text: &str) {
        self.pending_messages
            .follow_up(AgentMessage::FollowUp(Message::user(text)));
    }

    /// Get a snapshot of the current message history.
    pub async fn messages(&self) -> Vec<AgentMessage> {
        let st = self.inner.state().lock().await;
        st.session.messages()
    }

    /// Clear the current session's message history, starting fresh.
    pub async fn new_session(&self) {
        let st = self.inner.state().lock().await;
        st.session.clear();
    }

    /// Restore message history (e.g., when resuming a session).
    ///
    /// Clears any existing messages first, then appends the restored history.
    pub async fn restore_messages(&self, messages: Vec<AgentMessage>) {
        let st = self.inner.state().lock().await;
        st.session.clear();
        for msg in messages {
            st.session.append(msg);
        }
    }

    /// Access the tool registry (for name-based lookup of registered tools).
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    /// Get the names of all registered tools.
    pub fn tool_names(&self) -> Vec<String> {
        self.tool_registry.list().iter().map(|t| t.name().to_string()).collect()
    }

    /// Get the current permission mode.
    pub fn permission_mode(&self) -> PermissionMode {
        *self.permission_mode.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Set the permission mode.
    ///
    /// When switching to [`PermissionMode::Plan`], the `PlanModeMiddleware` is
    /// enabled so that write/execute tools are blocked.  Switching to any other
    /// mode disables it.
    pub fn set_permission_mode(&self, mode: PermissionMode) {
        let mut m = self.permission_mode.lock().unwrap_or_else(|e| e.into_inner());
        *m = mode;

        // Toggle plan mode via bus — PlanModeExtension registers PlanModeControl
        if let Some(ctrl) = self.inner.bus().get::<dyn PlanModeControl>() {
            ctrl.set_enabled(mode == PermissionMode::Plan);
        }
    }

    /// Switch the language model. Takes effect on the next prompt.
    pub async fn set_model(&self, model: Arc<dyn alva_kernel_abi::LanguageModel>) {
        let mut st = self.inner.state().lock().await;
        st.model = model;
    }

    /// Get the current model ID.
    pub async fn model_id(&self) -> String {
        let st = self.inner.state().lock().await;
        st.model.model_id().to_string()
    }

    /// Access the memory service (if enabled).
    pub fn memory(&self) -> Option<&MemoryService> {
        self.memory.as_ref()
    }

    /// Access the cross-layer coordination bus.
    pub fn bus(&self) -> &BusHandle {
        self.inner.bus()
    }

    /// Access the bus writer for post-init capability wiring.
    ///
    /// Lets outer layers register services on the bus after `build()`
    /// without the builder having to know every possible capability
    /// ahead of time. Used, e.g., by observers to provide a
    /// `ChildRunRecording` service for the `agent_spawn` plugin.
    pub fn bus_writer(&self) -> &BusWriter {
        &self.bus_writer
    }

    /// Access the runtime extension host (event dispatch and command registry).
    pub fn extension_host(&self) -> &Arc<std::sync::RwLock<crate::extension::ExtensionHost>> {
        self.inner.host()
    }

    /// Resolve a pending permission request. Called by the UI layer (CLI/GUI).
    pub async fn resolve_permission(
        &self,
        request_id: &str,
        tool_name: &str,
        decision: alva_agent_security::PermissionDecision,
    ) {
        if let Some(guard) = &self.security_guard {
            let mut g = guard.lock().await;
            g.resolve_permission(request_id, tool_name, decision);
        }
    }

    /// Register a checkpoint callback for auto-checkpointing before file writes.
    pub fn set_checkpoint_callback(
        &self,
        callback: Arc<dyn alva_host_native::middleware::CheckpointCallback>,
    ) {
        self.bus_writer.provide(Arc::new(
            alva_host_native::middleware::CheckpointCallbackRef(callback),
        ));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base_agent::BaseAgent;

    use alva_test::fixtures::make_assistant_message;
    use alva_test::mock_provider::MockLanguageModel;

    /// Helper: build a BaseAgent with minimal config using a mock model.
    async fn build_test_agent(model: Arc<dyn alva_kernel_abi::LanguageModel>) -> BaseAgent {
        let tmp = tempfile::tempdir().expect("failed to create tempdir");
        BaseAgent::builder()
            .workspace(tmp.path())
            .system_prompt("You are a test agent.")
            .build(model)
            .await
            .expect("build should succeed")
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
        use alva_kernel_abi::ToolOutput;

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
            .with_result(ToolOutput::text("tool executed"));
        let mock_tool_clone = mock_tool.clone();

        let tmp = tempfile::tempdir().expect("tempdir");
        let agent = BaseAgent::builder()
            .workspace(tmp.path())
            .system_prompt("You are a test agent.")
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
                    assert_eq!(result.model_text(), "tool executed");
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
    async fn test_base_agent_no_tools_by_default() {
        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("unused")),
        );

        let tmp = tempfile::tempdir().expect("tempdir");
        let agent = BaseAgent::builder()
            .workspace(tmp.path())
            .build(model)
            .await
            .expect("build should succeed");

        // Builder registers zero tools by default — caller must use .tools()
        let defs = agent.tool_registry().definitions();
        assert!(defs.is_empty(), "no tools should be registered by default, got: {:?}",
            defs.iter().map(|d| d.name.clone()).collect::<Vec<_>>());
    }

    #[tokio::test]
    async fn test_base_agent_with_tool_presets() {
        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("unused")),
        );

        let tmp = tempfile::tempdir().expect("tempdir");
        let agent = BaseAgent::builder()
            .workspace(tmp.path())
            .tools(alva_agent_extension_builtin::tool_presets::file_io())
            .tools(alva_agent_extension_builtin::tool_presets::shell())
            .build(model)
            .await
            .expect("build should succeed");

        let names: Vec<String> = agent.tool_registry().definitions().iter().map(|d| d.name.clone()).collect();
        assert!(names.contains(&"read_file".to_string()), "should have read_file");
        assert!(names.contains(&"execute_shell".to_string()), "should have execute_shell");
    }
}
