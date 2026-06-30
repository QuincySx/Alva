// INPUT:  alva_agent_core::Agent, bus/session/tool registry, permission services
// OUTPUT: BaseAgent
// POS:    Harness-level agent facade exposing prompt/session/control and assembly observability.
use std::sync::Arc;

use alva_agent_core::Agent;
use alva_kernel_abi::{
    AgentMessage, BusHandle, BusWriter, CancellationToken, Message, ToolRegistry,
};
use alva_kernel_core::event::AgentEvent;
use alva_kernel_core::run::run_agent;

use tokio::sync::mpsc;

use super::builder::BaseAgentBuilder;
use super::permission::{PermissionMode, PermissionModeService};

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
    /// Wrapped in Arc so the PluginHost can also hold a reference for shutdown().
    pub(super) current_cancel: Arc<std::sync::Mutex<CancellationToken>>,
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
            let mut current = self
                .current_cancel
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            *current = cancel.clone();
        }

        let inner = self.inner.clone();

        tokio::spawn(async move {
            let mut st = inner.state().lock().await;
            let config = inner.config().await;
            if let Err(e) = run_agent(&mut st, &*config, cancel, messages, event_tx.clone()).await {
                tracing::error!(error = %e, "agent loop failed");
            }
        });

        event_rx
    }

    /// Cancel the currently running agent loop.
    pub fn cancel(&self) {
        let current = self
            .current_cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        current.cancel();
    }

    /// Set the reasoning effort applied to the next turn's LLM calls.
    /// `None` clears any override — provider-default behavior.
    ///
    /// Safe to call between turns; a running turn uses whatever effort
    /// was set when it started (reads via RwLock read guard).
    pub async fn set_reasoning_effort(&self, effort: Option<alva_kernel_abi::ReasoningEffort>) {
        self.inner.set_reasoning_effort(effort).await;
    }

    /// Per-turn override of the provider-specific JSON pass-through
    /// merged into the LLM request body. `None` / empty map clears.
    pub async fn set_extra_body(&self, extra: Option<serde_json::Map<String, serde_json::Value>>) {
        self.inner.set_extra_body(extra).await;
    }

    /// Per-turn override: when `true`, skip all tool injection on the
    /// next run (the request goes out without a `tools` field, even if
    /// the agent has tools registered).
    pub async fn set_disable_tools(&self, disabled: bool) {
        self.inner.set_disable_tools(disabled).await;
    }

    /// Snapshot of the layered system-prompt segments currently set on
    /// the agent's config. Stable→dynamic order. Used by the harness
    /// to record what was actually assembled (vs the raw user-typed
    /// prompt) into the session config snapshot for Inspector.
    pub async fn system_prompt_segments(&self) -> Vec<String> {
        let cfg = self.inner.config().await;
        cfg.system_prompt.clone()
    }

    /// Get a snapshot of the current message history.
    pub async fn messages(&self) -> Vec<AgentMessage> {
        let st = self.inner.state().lock().await;
        st.session.messages().await
    }

    /// Clear the current session's message history, starting fresh.
    pub async fn new_session(&self) {
        let st = self.inner.state().lock().await;
        let _ = st.session.clear().await;
    }

    /// Restore message history (e.g., when resuming a session).
    ///
    /// Clears any existing messages first, then appends the restored history.
    pub async fn restore_messages(&self, messages: Vec<AgentMessage>) {
        let st = self.inner.state().lock().await;
        let _ = st.session.clear().await;
        for msg in messages {
            st.session.append_message(msg, None).await;
        }
    }

    /// Swap the current session for a new one. After this call, all reads and
    /// writes go to `new_session`. Used by apps (like CLI) that want to switch
    /// between persistent session files without rebuilding the entire agent.
    ///
    /// The caller is responsible for calling `new_session.restore().await`
    /// before handing it over if the session needs to warm its cache from
    /// durable storage.
    pub async fn swap_session(
        &self,
        new_session: std::sync::Arc<dyn alva_kernel_abi::agent_session::AgentSession>,
    ) {
        let mut st = self.inner.state().lock().await;
        st.session = new_session;
    }

    /// Build a fresh [`ToolRegistry`] snapshot from the currently-registered
    /// tools. The registry is constructed on demand — `BaseAgent` does not
    /// cache it, because the inner agent already owns the authoritative
    /// tools list.
    pub fn tool_registry(&self) -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        for tool in self.inner.tools() {
            reg.register_arc(tool.clone());
        }
        reg
    }

    /// Get the names of all registered tools.
    pub fn tool_names(&self) -> Vec<String> {
        self.inner
            .tools()
            .iter()
            .map(|t| t.name().to_string())
            .collect()
    }

    /// Names of plugins that participated in the build, in registration order.
    pub fn plugin_names(&self) -> Vec<String> {
        self.inner.assembly_snapshot().plugin_names
    }

    /// Names of middleware layers in the final sorted middleware stack order.
    pub fn middleware_names(&self) -> Vec<String> {
        self.inner.assembly_snapshot().middleware_names
    }

    /// Full build-time plugin/middleware contribution snapshot.
    pub fn assembly_snapshot(&self) -> alva_agent_core::AgentAssemblySnapshot {
        self.inner.assembly_snapshot()
    }

    /// Get the current permission mode.
    ///
    /// Reads the value from the bus-published [`PermissionModeService`].
    /// If no service is registered (e.g. `PermissionPlugin` was not added),
    /// falls back to the default.
    pub fn permission_mode(&self) -> PermissionMode {
        self.inner
            .bus()
            .get::<PermissionModeService>()
            .map(|s| s.get())
            .unwrap_or_default()
    }

    /// Set the permission mode.
    ///
    /// Writes through to the bus-published [`PermissionModeService`], which
    /// fans out to whichever control handles are registered on the bus
    /// (`PlanModeControl`, `SecurityModeControl`).
    ///
    /// Returns `true` if the change was applied, `false` if no
    /// [`PermissionModeService`] is registered (e.g. the `permission`
    /// component / `PermissionPlugin` was not added). Callers that honor an
    /// explicit user request — e.g. a `--permission-mode` flag — MUST check
    /// the return value: a silent `false` means the requested mode (including
    /// the read-only `Plan` mode) had no effect, which is a safety-relevant
    /// surprise rather than a benign no-op.
    #[must_use = "a `false` return means the permission mode was NOT applied (no service registered)"]
    pub fn set_permission_mode(&self, mode: PermissionMode) -> bool {
        if let Some(service) = self.inner.bus().get::<PermissionModeService>() {
            service.set(mode);
            true
        } else {
            false
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

    /// Access the cross-layer coordination bus.
    pub fn bus(&self) -> &BusHandle {
        self.inner.bus()
    }

    /// Access the bus writer for post-init capability wiring.
    ///
    /// Lets outer layers register services on the bus after `build()`
    /// without the builder having to know every possible capability
    /// ahead of time.
    pub fn bus_writer(&self) -> &BusWriter {
        &self.bus_writer
    }

    /// Access the runtime plugin host (middleware and command registry).
    pub fn plugin_host(&self) -> &Arc<std::sync::RwLock<crate::extension::PluginHost>> {
        self.inner.host()
    }

    /// Resolve a pending permission request. Called by the UI layer (CLI/GUI).
    ///
    /// The `SecurityGuard` handle is looked up on the bus where the
    /// `SecurityPlugin` (or any user-provided replacement) publishes it.
    pub async fn resolve_permission(
        &self,
        request_id: &str,
        tool_name: &str,
        decision: alva_agent_security::PermissionDecision,
    ) {
        if let Some(guard) = self
            .inner
            .bus()
            .get::<tokio::sync::Mutex<alva_agent_security::SecurityGuard>>()
        {
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
            MockLanguageModel::new().with_response(make_assistant_message("Hello from mock!")),
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
        let model =
            Arc::new(MockLanguageModel::new().with_response(make_assistant_message("All good!")));

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
        assert!(
            error.is_none(),
            "AgentEnd should have no error, got: {:?}",
            error
        );
    }

    #[tokio::test]
    async fn test_base_agent_messages_after_prompt() {
        let model = Arc::new(
            MockLanguageModel::new().with_response(make_assistant_message("Response text")),
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
        use alva_kernel_abi::ToolOutput;
        use alva_test::fixtures::make_tool_call_message;
        use alva_test::mock_tool::MockTool;

        // The model will first return a tool call, then a final text response.
        let tool_call_resp =
            make_tool_call_message("my_test_tool", serde_json::json!({"key": "value"}));
        let final_resp = make_assistant_message("Done using the tool.");

        let mock_model = MockLanguageModel::new()
            .with_response(tool_call_resp)
            .with_response(final_resp);
        let model = Arc::new(mock_model);

        let mock_tool =
            MockTool::new("my_test_tool").with_result(ToolOutput::text("tool executed"));
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
        let model =
            Arc::new(MockLanguageModel::new().with_response(make_assistant_message("unused")));

        let tmp = tempfile::tempdir().expect("tempdir");
        let agent = BaseAgent::builder()
            .workspace(tmp.path())
            .build(model)
            .await
            .expect("build should succeed");

        // Builder registers zero tools by default — caller must use .tools()
        let defs = agent.tool_registry().definitions();
        assert!(
            defs.is_empty(),
            "no tools should be registered by default, got: {:?}",
            defs.iter().map(|d| d.name.clone()).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn test_base_agent_with_tool_presets() {
        let model =
            Arc::new(MockLanguageModel::new().with_response(make_assistant_message("unused")));

        let tmp = tempfile::tempdir().expect("tempdir");
        let agent = BaseAgent::builder()
            .workspace(tmp.path())
            .tools(alva_agent_extension_builtin::tool_presets::file_io())
            .tools(alva_agent_extension_builtin::tool_presets::shell())
            .build(model)
            .await
            .expect("build should succeed");

        let names: Vec<String> = agent
            .tool_registry()
            .definitions()
            .iter()
            .map(|d| d.name.clone())
            .collect();
        assert!(
            names.contains(&"read_file".to_string()),
            "should have read_file"
        );
        assert!(
            names.contains(&"execute_shell".to_string()),
            "should have execute_shell"
        );
    }
}
