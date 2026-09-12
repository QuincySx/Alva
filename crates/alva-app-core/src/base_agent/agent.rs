// INPUT:  alva_agent_core::Agent, bus/session/tool registry, permission services
// OUTPUT: BaseAgent
// POS:    Harness facade exposing compatible and production-bounded prompt streams plus session/control observability.
use std::sync::Arc;

use alva_agent_core::Agent;
use alva_agent_extension_builtin::skill_tool::{SkillRegistry, SkillRegistryError};
use alva_kernel_abi::{
    AgentMessage, BusHandle, BusWriter, CancellationToken, Message, ToolRegistry,
};
use alva_kernel_core::event::AgentEvent;
use alva_kernel_core::AgentLoop;
use alva_kernel_core::DefaultAgentLoop;
use alva_kernel_core::DEFAULT_AGENT_EVENT_CHANNEL_CAPACITY;

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
    /// R05/R09/R10: 完整会话经共享 cell 持有——state.session 只是对话记录；
    /// clear/restore/flush/swap 等存储动词与压缩中间件、工具上下文全部读取
    /// 同一 cell，swap_session 原地更新后所有消费方立即看到当前会话。
    pub(super) full_session: alva_kernel_abi::agent_session::StoreSessionCell,
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
    pub async fn prompt_text(&self, text: &str) -> mpsc::UnboundedReceiver<AgentEvent> {
        let msg = AgentMessage::Standard(Message::user(text));
        self.prompt(vec![msg]).await
    }

    /// Production-facing text prompt with bounded event backpressure.
    pub async fn prompt_text_bounded(&self, text: &str) -> mpsc::Receiver<AgentEvent> {
        let msg = AgentMessage::Standard(Message::user(text));
        self.prompt_bounded(vec![msg]).await
    }

    /// Send messages to the agent and receive events via an unbounded channel.
    ///
    /// A fresh `CancellationToken` is created for each call so that a
    /// previously-cancelled token does not block future prompts.
    ///
    /// **Turn ownership:** this future does not resolve until the spawned run
    /// has acquired `AgentState` (and a config read lock). Callers that set
    /// per-turn knobs / `swap_session` before `prompt` can therefore drop their
    /// harness lock as soon as `prompt` returns, knowing the turn already owns
    /// the shared agent — a concurrent `swap_session` will block until the run
    /// finishes rather than stealing the session mid-flight.
    pub async fn prompt(&self, messages: Vec<AgentMessage>) -> mpsc::UnboundedReceiver<AgentEvent> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let cancel = CancellationToken::new();
        let inner = self.inner.clone();
        let current_cancel = self.current_cancel.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            // Acquire state first so concurrent swap_session/retain_tools
            // cannot interleave between spawn and run_agent. Hold config
            // read for the whole turn so per-turn knob writes serialize
            // behind this run (single shared agent = one active turn).
            let st = inner.state().lock().await;
            let config = inner.config().await;
            {
                let mut current = current_cancel.lock().unwrap_or_else(|e| e.into_inner());
                *current = cancel.clone();
            }
            let _ = started_tx.send(());
            if let Err(e) = DefaultAgentLoop
                .drive(
                    &st,
                    &config,
                    cancel,
                    messages,
                    alva_kernel_core::event::AgentEventSender::from(event_tx),
                )
                .await
            {
                tracing::error!(error = %e, "agent loop failed");
            }
        });

        // Close the spawn→lock race window before returning to the harness.
        let _ = started_rx.await;
        event_rx
    }

    /// Send messages with a bounded production event stream.
    ///
    /// Lifecycle, message, and terminal tool events push back on the kernel at
    /// [`DEFAULT_AGENT_EVENT_CHANNEL_CAPACITY`]. Synchronous high-frequency
    /// tool progress updates are best-effort and may be dropped while the
    /// channel is full; the matching ToolExecutionEnd is
    /// always delivered losslessly.
    pub async fn prompt_bounded(&self, messages: Vec<AgentMessage>) -> mpsc::Receiver<AgentEvent> {
        let (event_tx, event_rx) = mpsc::channel(DEFAULT_AGENT_EVENT_CHANNEL_CAPACITY);

        let cancel = CancellationToken::new();
        let inner = self.inner.clone();
        let current_cancel = self.current_cancel.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let st = inner.state().lock().await;
            let config = inner.config().await;
            {
                let mut current = current_cancel.lock().unwrap_or_else(|e| e.into_inner());
                *current = cancel.clone();
            }
            let _ = started_tx.send(());
            if let Err(error) = DefaultAgentLoop
                .drive(
                    &st,
                    &config,
                    cancel,
                    messages,
                    alva_kernel_core::event::AgentEventSender::from(event_tx),
                )
                .await
            {
                tracing::error!(%error, "agent loop failed");
            }
        });

        let _ = started_rx.await;
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

    /// Per-turn manual tool allow-list: `Some(names)` restricts the next run
    /// to those tools without mutating the registered set; `None` clears it.
    pub async fn set_allowed_tools(&self, allowed: Option<Vec<String>>) {
        self.inner.set_allowed_tools(allowed).await;
    }

    /// Per-turn system-prompt additions (e.g. UI-selected skill bodies).
    /// Replaces the previous turn's list; pass `vec![]` to clear.
    pub async fn set_turn_system_additions(&self, additions: Vec<String>) {
        self.inner.set_turn_system_additions(additions).await;
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
        st.session.messages().await.unwrap_or_default()
    }

    /// Clone the authoritative session handle for inspection/storage wiring.
    /// The handle owns its own synchronization and remains valid after this
    /// short state-lock scope ends.
    pub async fn session_handle(&self) -> Arc<dyn alva_kernel_abi::agent_session::AgentSession> {
        self.full_session.get()
    }

    /// Clear the current session's message history, starting fresh.
    pub async fn new_session(&self) -> bool {
        self.full_session.get().clear().await.is_ok()
    }

    /// Restore message history (e.g., when resuming a session).
    ///
    /// Clears any existing messages first, then appends the restored history.
    pub async fn restore_messages(&self, messages: Vec<AgentMessage>) -> bool {
        let full = self.full_session.get();
        if let Err(error) = full.clear().await {
            tracing::error!(%error, "failed to clear session before restoring messages");
            return false;
        }
        for msg in messages {
            let _ = full.append_message(msg, None).await;
        }
        true
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
    ) -> bool {
        // R05/R10/R15: 换会话是装配层动作，且必须是**原子**切换。整个交换
        // （flush 旧会话 → 日志与 store cell 同时指向新会话）在 state 锁内
        // 完成——turn 启动/运行同样持有该锁，因此任何观察者要么看到完全
        // 旧、要么完全新的（日志, store) 组合，不存在半切换窗口；并发
        // swap 也被该锁串行化，最终状态与完成顺序一致。
        let mut st = self.inner.state().lock().await;
        let full = self.full_session.get();
        if let Err(error) = full.flush().await {
            tracing::error!(%error, "refusing to swap away from a session that failed to flush");
            return false;
        }
        st.session = new_session.clone();
        self.full_session.set(new_session);
        true
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

    /// Restrict the live tool set to `allowed` (per-invocation allowlist,
    /// `--allowed-tools`). Returns the names that were removed. Unknown
    /// names in `allowed` are the CALLER's job to reject up front (compare
    /// against `tool_names()` first) — a typo silently allowing nothing is
    /// the component-toggle lesson all over again.
    pub async fn retain_tools(&self, allowed: &[String]) -> Vec<String> {
        let mut st = self.inner.state().lock().await;
        let removed: Vec<String> = st
            .tools
            .iter()
            .map(|t| t.name().to_string())
            .filter(|n| !allowed.contains(n))
            .collect();
        st.tools.retain(|t| allowed.iter().any(|a| a == t.name()));
        removed
    }

    /// `(name, description)` for every registered tool — the discovery
    /// surface for orchestrators deciding a per-invocation allowlist
    /// (`alva tools list`).
    pub fn tool_summaries(&self) -> Vec<(String, String)> {
        self.inner
            .tools()
            .iter()
            .map(|t| (t.name().to_string(), t.description().to_string()))
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

    /// Invoke a named skill directly through the same registry used by the
    /// model-facing `skill` tool. `None` means the skills plugin is not
    /// installed; registry lookup/load failures are returned separately.
    pub async fn invoke_skill(
        &self,
        name: &str,
        args: Option<&str>,
    ) -> Option<Result<String, SkillRegistryError>> {
        let registry = self.inner.bus().get::<dyn SkillRegistry>()?;
        Some(registry.invoke(name, args).await)
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
    ) -> bool {
        if let Some(guard) = self
            .inner
            .bus()
            .get::<tokio::sync::Mutex<alva_agent_security::SecurityGuard>>()
        {
            let mut g = guard.lock().await;
            return g.resolve_permission(request_id, tool_name, decision);
        }
        false
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
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use alva_test::fixtures::make_assistant_message;
    use alva_test::mock_provider::MockLanguageModel;
    use async_trait::async_trait;
    use futures::Stream;

    struct FirstPendingThenEcho {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl alva_kernel_abi::LanguageModel for FirstPendingThenEcho {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[&dyn alva_kernel_abi::Tool],
            _config: &alva_kernel_abi::ModelConfig,
        ) -> Result<alva_kernel_abi::CompletionResponse, alva_kernel_abi::AgentError> {
            Err(alva_kernel_abi::AgentError::LlmError(
                "unexpected fallback".into(),
            ))
        }

        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[&dyn alva_kernel_abi::Tool],
            _config: &alva_kernel_abi::ModelConfig,
        ) -> Pin<Box<dyn Stream<Item = alva_kernel_abi::StreamEvent> + Send>> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Box::pin(tokio_stream::pending())
            } else {
                Box::pin(tokio_stream::iter([
                    alva_kernel_abi::StreamEvent::Start,
                    alva_kernel_abi::StreamEvent::TextDelta {
                        text: "second completed".into(),
                    },
                    alva_kernel_abi::StreamEvent::Done,
                ]))
            }
        }

        fn model_id(&self) -> &str {
            "first-pending-then-echo"
        }
    }

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

    /// R15 acceptance: concurrent swaps leave log and store pointing at the
    /// SAME session — the state lock serializes swap against turn start and
    /// against the other swap. Also: flush failure keeps the old session.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_swaps_keep_log_and_store_on_the_same_session() {
        let model = Arc::new(MockLanguageModel::new().with_response(make_assistant_message("ok")));
        let agent = build_test_agent(model).await;

        let session_b: std::sync::Arc<dyn alva_kernel_abi::agent_session::AgentSession> = Arc::new(
            alva_agent_core::agent_session::InMemoryAgentSession::with_id("session-b".into()),
        );
        let session_c: std::sync::Arc<dyn alva_kernel_abi::agent_session::AgentSession> =
            std::sync::Arc::new(
                alva_agent_core::agent_session::InMemoryAgentSession::with_id("session-c".into()),
            );

        // Two concurrent swaps; whichever completes last, log and cell must
        // agree on the same session id.
        let a = agent.swap_session(session_b.clone());
        let b = agent.swap_session(session_c.clone());
        let (ra, rb) = tokio::join!(a, b);
        assert!(ra && rb);

        let handle_id = agent.session_handle().await.session_id().to_string();
        let st = agent.inner.state().lock().await;
        let log_id = st.session.session_id().to_string();
        drop(st);
        assert_eq!(
            handle_id, log_id,
            "log and store capability must be the same session after concurrent swaps"
        );
        assert!(
            handle_id == "session-b" || handle_id == "session-c",
            "final session must be one of the swapped targets: {handle_id}"
        );
    }

    #[tokio::test]
    async fn failed_flush_keeps_the_current_session() {
        use alva_agent_core::agent_session::InMemoryAgentSession;
        use alva_kernel_abi::{
            ConversationLog, EventMatch, EventQuery, SessionError, SessionStore,
        };

        struct FailingFlushSession(InMemoryAgentSession);

        #[async_trait::async_trait]
        impl ConversationLog for FailingFlushSession {
            fn session_id(&self) -> &str {
                self.0.session_id()
            }
            async fn messages(&self) -> Result<Vec<alva_kernel_abi::AgentMessage>, SessionError> {
                self.0.messages().await
            }
            async fn recent_messages(
                &self,
                n: usize,
            ) -> Result<Vec<alva_kernel_abi::AgentMessage>, SessionError> {
                self.0.recent_messages(n).await
            }
            async fn append_message(
                &self,
                m: AgentMessage,
                p: Option<String>,
            ) -> Result<(), SessionError> {
                self.0.append_message(m, p).await
            }
            async fn append(&self, _: alva_kernel_abi::SessionEvent) -> Result<(), SessionError> {
                Ok(())
            }
            async fn append_records(
                &self,
                r: Vec<alva_kernel_abi::SessionRecord>,
            ) -> Result<alva_kernel_abi::AppendReceipt, SessionError> {
                self.0.append_records(r).await
            }
            fn supports_atomic_records(&self) -> bool {
                true
            }
            async fn flush(&self) -> Result<(), SessionError> {
                Err(SessionError::Other("injected flush failure".into()))
            }
        }
        #[async_trait::async_trait]
        impl SessionStore for FailingFlushSession {
            fn parent_session_id(&self) -> Option<&str> {
                None
            }
            async fn query(&self, q: &EventQuery) -> Vec<alva_kernel_abi::EventMatch> {
                self.0.query(q).await
            }
            async fn count(&self, q: &EventQuery) -> usize {
                self.0.count(q).await
            }
            async fn rollback_after(&self, u: &str) -> usize {
                self.0.rollback_after(u).await
            }
            async fn save_snapshot(&self, d: &[u8]) {
                self.0.save_snapshot(d).await
            }
            async fn load_snapshot(&self) -> Option<Vec<u8>> {
                self.0.load_snapshot().await
            }
            async fn restore(&self) -> Result<(), SessionError> {
                Ok(())
            }
            async fn close(&self) -> Result<(), SessionError> {
                Ok(())
            }
            async fn clear(&self) -> Result<(), SessionError> {
                self.0.clear().await
            }
            async fn compact_messages(&self, m: Vec<AgentMessage>) {
                self.0.compact_messages(m).await
            }
        }
        impl alva_kernel_abi::agent_session::AgentSession for FailingFlushSession {}

        let model = Arc::new(MockLanguageModel::new().with_response(make_assistant_message("ok")));
        let agent = build_test_agent(model).await;

        let ok_session: std::sync::Arc<dyn alva_kernel_abi::agent_session::AgentSession> =
            Arc::new(InMemoryAgentSession::with_id("ok-base".into()));
        let failing: std::sync::Arc<dyn alva_kernel_abi::agent_session::AgentSession> = Arc::new(
            FailingFlushSession(InMemoryAgentSession::with_id("doomed".into())),
        );

        // 先切到 flush-fail 会话（swap INTO 成功：flush 的是旧会话），
        // 再切走——flush 失败必须拒绝切换并保留当前会话。
        assert!(
            agent.swap_session(failing).await,
            "swap into failing session"
        );
        assert!(
            !agent.swap_session(ok_session).await,
            "flush failure must refuse the swap"
        );
        assert_eq!(
            agent.session_handle().await.session_id(),
            "doomed",
            "current session unchanged after refused swap"
        );
    }

    #[tokio::test]
    async fn test_base_agent_prompt_produces_events() {
        let model = Arc::new(
            MockLanguageModel::new().with_response(make_assistant_message("Hello from mock!")),
        );

        let agent = build_test_agent(model).await;
        let mut rx = agent.prompt_text("hi").await;

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
        let mut rx = agent.prompt_text("Tell me something.").await;

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
        let mut rx = agent.prompt_text("hello").await;

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

    /// R24: DETERMINISTIC swap/turn gate (no sleep-based racing). The model's
    /// first stream parks until released, so the turn is provably in-flight
    /// inside the run loop when we probe the swap. While the turn owns
    /// AgentState the swap must block; after the turn unwinds the swap lands
    /// and the next turn runs on the new session. The started turn's records
    /// stay in session A — its identity is preserved, not stolen.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn deterministic_swap_gate_blocks_until_turn_releases_state() {
        use alva_agent_core::agent_session::InMemoryAgentSession;
        use std::time::Duration;

        // Simple gated model: first stream() call signals `entered`, then
        // parks forever; later calls answer immediately.
        struct GateModel {
            entered: std::sync::Arc<tokio::sync::Semaphore>,
            release: std::sync::Arc<tokio::sync::Semaphore>,
            calls: std::sync::atomic::AtomicUsize,
        }
        impl GateModel {
            fn new() -> (Arc<Self>, Arc<tokio::sync::Semaphore>, Arc<tokio::sync::Semaphore>) {
                let entered = Arc::new(tokio::sync::Semaphore::new(0));
                let release = Arc::new(tokio::sync::Semaphore::new(0));
                (
                    Arc::new(Self {
                        entered: entered.clone(),
                        release: release.clone(),
                        calls: std::sync::atomic::AtomicUsize::new(0),
                    }),
                    entered,
                    release,
                )
            }
        }
        #[async_trait]
        impl alva_kernel_abi::LanguageModel for GateModel {
            async fn complete(
                &self,
                _messages: &[Message],
                _tools: &[&dyn alva_kernel_abi::Tool],
                _config: &alva_kernel_abi::ModelConfig,
            ) -> Result<alva_kernel_abi::CompletionResponse, alva_kernel_abi::AgentError> {
                Err(alva_kernel_abi::AgentError::LlmError("stream only".into()))
            }
            fn stream(
                &self,
                _messages: &[Message],
                _tools: &[&dyn alva_kernel_abi::Tool],
                _config: &alva_kernel_abi::ModelConfig,
            ) -> Pin<Box<dyn Stream<Item = alva_kernel_abi::StreamEvent> + Send>> {
                if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    self.entered.add_permits(1);
                    let release = self.release.clone();
                    Box::pin(futures::stream::once(async move {
                        // Park inside the stream until the test releases the
                        // gate; then end the turn without text.
                        let _ = release.acquire().await;
                        alva_kernel_abi::StreamEvent::Done
                    }))
                } else {
                    Box::pin(tokio_stream::iter([
                        alva_kernel_abi::StreamEvent::Start,
                        alva_kernel_abi::StreamEvent::TextDelta {
                            text: "turn-b done".into(),
                        },
                        alva_kernel_abi::StreamEvent::Done,
                    ]))
                }
            }
            fn model_id(&self) -> &str {
                "gated-swap-model"
            }
        }

        let (model, entered, release) = GateModel::new();
        let agent = Arc::new(build_test_agent(model).await);

        let session_a = Arc::new(InMemoryAgentSession::with_id("gate-a".into()))
            as Arc<dyn alva_kernel_abi::agent_session::AgentSession>;
        let session_b = Arc::new(InMemoryAgentSession::with_id("gate-b".into()))
            as Arc<dyn alva_kernel_abi::agent_session::AgentSession>;
        agent.swap_session(session_a.clone()).await;
        let mut rx_a = agent.prompt_text("turn-a").await;

        // Deterministic "turn in flight": the model's first stream call has
        // entered and is parked. prompt_text already returned — the turn owns
        // AgentState for as long as the stream is pending.
        entered.acquire().await.unwrap().forget();

        // A swap now must BLOCK — probe with a bounded timeout, no sleeps.
        let agent_for_swap = agent.clone();
        let session_b_for_swap = session_b.clone();
        let mut swap =
            tokio::spawn(async move { agent_for_swap.swap_session(session_b_for_swap).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut swap)
                .await
                .is_err(),
            "swap must block while the in-flight turn holds AgentState"
        );

        // Release the turn: it unwinds (AgentEnd), then the blocked swap
        // lands on session B.
        release.add_permits(1);
        while let Some(event) = rx_a.recv().await {
            if matches!(event, AgentEvent::AgentEnd { .. }) {
                break;
            }
        }
        assert!(
            tokio::time::timeout(Duration::from_secs(2), async {
                while agent.session_handle().await.session_id() != "gate-b" {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .is_ok(),
            "swap must complete once the turn releases the state lock"
        );
        swap.abort();

        // The started turn's identity is preserved: its user message lives in
        // session A, and B is untouched.
        let text_a = session_text(&session_a).await;
        assert!(
            text_a.contains("turn-a"),
            "session A must keep the started turn's user message, got {text_a:?}"
        );
        assert!(
            !text_a.contains("turn-b"),
            "session A must not see the next turn, got {text_a:?}"
        );

        // Next turn runs on B end-to-end.
        let mut rx_b = agent.prompt_text("turn-b").await;
        while let Some(event) = rx_b.recv().await {
            if matches!(event, AgentEvent::AgentEnd { .. }) {
                break;
            }
        }
        let text_b = session_text(&session_b).await;
        assert!(
            text_b.contains("turn-b") && text_b.contains("turn-b done"),
            "session B must hold the second turn's exchange, got {text_b:?}"
        );
    }

    async fn session_text(
        session: &Arc<dyn alva_kernel_abi::agent_session::AgentSession>,
    ) -> String {
        session
            .messages()
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|m| match m {
                AgentMessage::Standard(msg) => Some(msg.text_content()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Regression: concurrent `swap_session` after `prompt` returns must not
    /// move an already-started turn onto a different session. Before the
    /// spawn→lock handshake, the second swap could win the state lock first.
    #[tokio::test]
    async fn concurrent_swap_cannot_steal_an_in_flight_turn_session() {
        use alva_agent_core::agent_session::InMemoryAgentSession;
        use std::sync::Arc;
        use std::time::Duration;

        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("from-first"))
                .with_response(make_assistant_message("from-second")),
        );
        let agent = Arc::new(build_test_agent(model).await);

        let session_a = Arc::new(InMemoryAgentSession::new())
            as Arc<dyn alva_kernel_abi::agent_session::AgentSession>;
        let session_b = Arc::new(InMemoryAgentSession::new())
            as Arc<dyn alva_kernel_abi::agent_session::AgentSession>;
        let id_a = session_a.session_id().to_string();
        let id_b = session_b.session_id().to_string();
        assert_ne!(id_a, id_b);

        agent.swap_session(session_a.clone()).await;
        let mut rx_a = agent.prompt_text("message-for-a").await;

        // prompt_text returned ⇒ turn A owns AgentState. A concurrent swap
        // must block until A finishes rather than re-pointing mid-run.
        let agent_b = agent.clone();
        let session_b_clone = session_b.clone();
        let second = tokio::spawn(async move {
            agent_b.swap_session(session_b_clone).await;
            agent_b.prompt_text("message-for-b").await
        });

        // Give the second task a chance to race if the handshake were broken.
        tokio::time::sleep(Duration::from_millis(20)).await;

        while let Some(event) = rx_a.recv().await {
            if matches!(event, AgentEvent::AgentEnd { .. }) {
                break;
            }
        }
        let mut rx_b = second.await.expect("second turn task");
        while let Some(event) = rx_b.recv().await {
            if matches!(event, AgentEvent::AgentEnd { .. }) {
                break;
            }
        }

        let msgs_a = session_a.messages().await.unwrap_or_default();
        let msgs_b = session_b.messages().await.unwrap_or_default();
        let text_a: Vec<String> = msgs_a
            .iter()
            .filter_map(|m| match m {
                AgentMessage::Standard(msg) => Some(msg.text_content()),
                _ => None,
            })
            .collect();
        let text_b: Vec<String> = msgs_b
            .iter()
            .filter_map(|m| match m {
                AgentMessage::Standard(msg) => Some(msg.text_content()),
                _ => None,
            })
            .collect();

        assert!(
            text_a.iter().any(|t| t.contains("message-for-a")),
            "session A must keep the first user prompt, got {text_a:?}"
        );
        assert!(
            text_b.iter().any(|t| t.contains("message-for-b")),
            "session B must receive the second user prompt, got {text_b:?}"
        );
        assert!(
            !text_a.iter().any(|t| t.contains("message-for-b")),
            "session A must not be polluted by the second turn, got {text_a:?}"
        );
        assert!(
            !text_b.iter().any(|t| t.contains("message-for-a")),
            "session B must not steal the first turn, got {text_b:?}"
        );
    }

    #[tokio::test]
    async fn cancel_targets_the_active_turn_not_a_queued_prompt() {
        let model = Arc::new(FirstPendingThenEcho {
            calls: AtomicUsize::new(0),
        });
        let agent = Arc::new(build_test_agent(model).await);

        let mut first = agent.prompt_text("first").await;
        let queued_agent = agent.clone();
        let queued = tokio::spawn(async move { queued_agent.prompt_text("second").await });
        tokio::time::sleep(Duration::from_millis(20)).await;
        agent.cancel();

        let first_error = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(AgentEvent::AgentEnd { error }) = first.recv().await {
                    break error;
                }
            }
        })
        .await
        .expect("the active turn must be cancelled");
        assert!(first_error.is_some());

        let mut second = tokio::time::timeout(Duration::from_secs(1), queued)
            .await
            .expect("queued prompt must acquire the turn after cancellation")
            .expect("queued prompt task joins");
        let second_error = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(AgentEvent::AgentEnd { error }) = second.recv().await {
                    break error;
                }
            }
        })
        .await
        .expect("queued prompt must complete normally");
        assert!(second_error.is_none());
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

        let mut rx = agent.prompt_text("Use the tool please.").await;

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
