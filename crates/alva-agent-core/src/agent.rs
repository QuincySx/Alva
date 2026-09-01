// INPUT:  AgentState, AgentConfig, run_agent, PluginHost, Tool
// OUTPUT: Agent, AgentAssemblySnapshot
// POS:    SDK-level agent handle with compatible unbounded and native bounded streaming entry points.
//! Agent — SDK-level assembled agent handle.
//!
//! Produced by `AgentBuilder::build()`. Holds the wired-up `AgentState` +
//! `AgentConfig` + bus/plugin-host bookkeeping. Runs the agent loop via
//! `alva_kernel_core::run_agent` when `.run()` is called.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex, RwLock};

use alva_kernel_abi::{
    AgentError, AgentMessage, BusHandle, CancellationToken, ReasoningEffort, Tool,
};
use alva_kernel_core::event::AgentEvent;
use alva_kernel_core::state::{AgentConfig, AgentState};
use alva_kernel_core::AgentLoop;
use alva_kernel_core::DefaultAgentLoop;

use crate::extension::PluginHost;

/// Per-plugin build-time contribution metadata for observability.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginAssemblySnapshot {
    pub name: String,
    pub description: String,
    pub registered_tool_names: Vec<String>,
    pub finalized_tool_names: Vec<String>,
    pub middleware_names: Vec<String>,
    #[serde(default)]
    pub phase_contribution_names: Vec<String>,
    pub command_names: Vec<String>,
    pub system_prompt_fragments: usize,
}

/// Build-time assembly metadata for observability.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentAssemblySnapshot {
    /// Plugin names in registration order.
    pub plugin_names: Vec<String>,
    /// Middleware names in final sorted execution order.
    pub middleware_names: Vec<String>,
    /// Middleware names registered directly on AgentBuilder/BaseAgentBuilder,
    /// outside any plugin.
    #[serde(default)]
    pub direct_middleware_names: Vec<String>,
    /// Per-plugin contribution records in registration order.
    pub plugins: Vec<PluginAssemblySnapshot>,
}

/// A fully-assembled, ready-to-run agent.
///
/// Use `Agent::builder()` to construct one.
///
/// `config` is wrapped in a `RwLock` so per-turn overrides (e.g.
/// `reasoning_effort`) can be set between runs without rebuilding the
/// whole agent. Write lock is very brief (just flip one field); read
/// lock is held across the full `run_agent` call.
pub struct Agent {
    pub(crate) state: Arc<Mutex<AgentState>>,
    pub(crate) config: Arc<RwLock<AgentConfig>>,
    pub(crate) bus: BusHandle,
    pub(crate) host: Arc<std::sync::RwLock<PluginHost>>,
    /// Snapshot of the tools the agent was built with. Cached so callers
    /// can inspect tool definitions without locking `state`.
    pub(crate) tools: Vec<Arc<dyn Tool>>,
    pub(crate) assembly: AgentAssemblySnapshot,
}

impl Agent {
    /// Start building a new agent.
    pub fn builder() -> crate::agent_builder::AgentBuilder {
        crate::agent_builder::AgentBuilder::new()
    }

    /// Run one conversation turn. Returns a channel that streams
    /// `AgentEvent`s until the turn completes.
    ///
    /// Native targets with an active Tokio runtime drive the loop in the
    /// background and return immediately. Other executors, including wasm32,
    /// have no portable spawn contract, so the loop is driven inline and the
    /// returned channel is already closed after its buffered events. This
    /// keeps the SDK executor-agnostic while preserving live streaming in the
    /// native product harnesses.
    ///
    /// `cancel` lets the caller interrupt the loop mid-turn.
    pub async fn run(
        &self,
        input: Vec<AgentMessage>,
        cancel: CancellationToken,
    ) -> Result<mpsc::UnboundedReceiver<AgentEvent>, AgentError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let state = self.state.clone();
        let config = self.config.clone();
        let drive = async move {
            let state = state.lock().await;
            let config = config.read().await;
            if let Err(error) = DefaultAgentLoop
                .drive(&state, &config, cancel, input, tx)
                .await
            {
                tracing::error!(%error, "agent loop failed");
            }
        };
        #[cfg(not(target_family = "wasm"))]
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(drive);
        } else {
            drive.await;
        }
        #[cfg(target_family = "wasm")]
        drive.await;
        Ok(rx)
    }

    /// Run one native conversation turn with a bounded event channel.
    ///
    /// Unlike [`Self::run`], this path applies producer backpressure all the
    /// way through the default kernel loop. It requires an active Tokio
    /// runtime so the producer can keep running while the caller drains the
    /// returned receiver. The executor-neutral wasm API remains `run()`.
    #[cfg(not(target_family = "wasm"))]
    pub async fn run_bounded(
        &self,
        input: Vec<AgentMessage>,
        cancel: CancellationToken,
        capacity: usize,
    ) -> Result<mpsc::Receiver<AgentEvent>, AgentError> {
        if capacity == 0 {
            return Err(AgentError::ConfigError(
                "agent event channel capacity must be greater than zero".to_string(),
            ));
        }
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| {
            AgentError::ConfigError(
                "Agent::run_bounded requires an active Tokio runtime".to_string(),
            )
        })?;
        let (tx, rx) = mpsc::channel(capacity);
        let state = self.state.clone();
        let config = self.config.clone();
        runtime.spawn(async move {
            let state = state.lock().await;
            let config = config.read().await;
            if let Err(error) = DefaultAgentLoop
                .drive_bounded(&state, &config, cancel, input, tx)
                .await
            {
                tracing::error!(%error, "agent loop failed");
            }
        });
        Ok(rx)
    }

    /// Set (or clear) the reasoning effort applied to every LLM call in
    /// the next run. Takes effect immediately for the next `run()` — a
    /// currently-running `run()` is unaffected (it holds a read guard).
    ///
    /// Per-provider translation happens inside each provider's request-
    /// body assembly; `None` means "don't send the field" (use provider
    /// default).
    pub async fn set_reasoning_effort(&self, effort: Option<ReasoningEffort>) {
        let mut config = self.config.write().await;
        config.model_config.reasoning_effort = effort;
    }

    /// Per-turn override of the provider-specific JSON pass-through
    /// (`ModelConfig::extra_body`). Same write-lock semantics as
    /// `set_reasoning_effort`: takes effect on the next `run()`. `None`
    /// or an empty map clears any previous override.
    pub async fn set_extra_body(&self, extra: Option<serde_json::Map<String, serde_json::Value>>) {
        let mut config = self.config.write().await;
        config.model_config.extra_body = match extra {
            Some(m) if !m.is_empty() => Some(m),
            _ => None,
        };
    }

    /// Per-turn override of `ModelConfig::disable_tools`. When set to
    /// `true`, the next `run()` skips ALL tool injection (state.tools
    /// stays as-is, but the provider sees `tools: []` → omits the
    /// field). Use when the active model doesn't support function
    /// calling.
    pub async fn set_disable_tools(&self, disabled: bool) {
        let mut config = self.config.write().await;
        config.model_config.disable_tools = disabled;
    }

    /// Per-turn manual tool allow-list. `Some(names)` restricts the next
    /// `run()` to exactly those tools; `None` clears the restriction. The
    /// agent's registered tool set is untouched, so a later turn can widen the
    /// selection again — this is the non-destructive counterpart to
    /// `retain_tools`, meant for a UI control that changes per turn.
    pub async fn set_allowed_tools(&self, allowed: Option<Vec<String>>) {
        let mut config = self.config.write().await;
        config.model_config.allowed_tools = allowed;
    }

    /// Per-turn system-prompt additions (UI-selected skill bodies, etc.).
    /// Replaces any previous turn's list; pass an empty vec to clear.
    /// Applied only to the next `run()`'s stable system-prompt bucket —
    /// the agent's assembled base prompt is not mutated.
    pub async fn set_turn_system_additions(&self, additions: Vec<String>) {
        let mut config = self.config.write().await;
        config.model_config.turn_system_additions = additions;
    }

    /// Access the bus for out-of-band communication (e.g. injecting
    /// steering messages, reading capability registrations).
    pub fn bus(&self) -> &BusHandle {
        &self.bus
    }

    /// Access the runtime plugin host (middleware + command registry,
    /// agent binding for cancellation/pending messages).
    pub fn host(&self) -> &Arc<std::sync::RwLock<PluginHost>> {
        &self.host
    }

    /// Snapshot of the tools the agent was built with.
    pub fn tools(&self) -> &[Arc<dyn Tool>] {
        &self.tools
    }

    /// Build-time plugin and middleware assembly metadata.
    pub fn assembly_snapshot(&self) -> AgentAssemblySnapshot {
        self.assembly.clone()
    }

    /// Access the agent config (read-only). Returns a read guard that
    /// dereferences to `&AgentConfig`. The guard is held across the
    /// caller's usage — keep it short if another turn might need to
    /// update config via `set_reasoning_effort`.
    pub async fn config(&self) -> tokio::sync::RwLockReadGuard<'_, AgentConfig> {
        self.config.read().await
    }

    /// Access the underlying state mutex (advanced — most callers should
    /// use `run`/`messages` accessors via a wrapping handle).
    pub fn state(&self) -> &Mutex<AgentState> {
        &self.state
    }
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::time::Duration;

    use async_trait::async_trait;
    use futures_core::Stream;

    use alva_kernel_abi::{CompletionResponse, LanguageModel, Message, ModelConfig, StreamEvent};

    use super::*;

    struct PendingModel;

    struct ImmediateModel;

    #[async_trait]
    impl LanguageModel for PendingModel {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Result<CompletionResponse, AgentError> {
            Err(AgentError::LlmError("unexpected fallback".into()))
        }

        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
            Box::pin(tokio_stream::pending())
        }

        fn model_id(&self) -> &str {
            "pending"
        }
    }

    #[async_trait]
    impl LanguageModel for ImmediateModel {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Result<CompletionResponse, AgentError> {
            Err(AgentError::LlmError("unexpected fallback".into()))
        }

        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
            Box::pin(tokio_stream::iter([
                StreamEvent::Start,
                StreamEvent::TextDelta {
                    text: "done".into(),
                },
                StreamEvent::Done,
            ]))
        }

        fn model_id(&self) -> &str {
            "immediate"
        }
    }

    #[tokio::test]
    async fn run_returns_receiver_before_the_model_finishes() {
        let agent = Agent::builder()
            .model(Arc::new(PendingModel))
            .build()
            .await
            .expect("agent builds");
        let cancel = CancellationToken::new();

        let mut events = tokio::time::timeout(
            Duration::from_millis(100),
            agent.run(
                vec![AgentMessage::Standard(Message::user("hello"))],
                cancel.clone(),
            ),
        )
        .await
        .expect("run must return before the pending stream completes")
        .expect("run starts");

        assert!(matches!(events.recv().await, Some(AgentEvent::AgentStart)));
        cancel.cancel();

        let end = tokio::time::timeout(Duration::from_millis(100), async {
            loop {
                if let Some(AgentEvent::AgentEnd { error }) = events.recv().await {
                    break error;
                }
            }
        })
        .await
        .expect("cancelled run must finish");
        assert!(end.is_some(), "cancellation must be visible in AgentEnd");
    }

    #[test]
    fn run_works_without_a_tokio_runtime() {
        futures_executor::block_on(async {
            let agent = Agent::builder()
                .model(Arc::new(ImmediateModel))
                .build()
                .await
                .expect("agent builds");
            let mut events = agent
                .run(
                    vec![AgentMessage::Standard(Message::user("hello"))],
                    CancellationToken::new(),
                )
                .await
                .expect("run starts");

            let mut saw_end = false;
            while let Some(event) = events.recv().await {
                if matches!(event, AgentEvent::AgentEnd { error: None }) {
                    saw_end = true;
                }
            }
            assert!(saw_end, "inline drive must complete the event stream");
        });
    }
}
