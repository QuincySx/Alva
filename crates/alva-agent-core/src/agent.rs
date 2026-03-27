// INPUT:  alva_types (CancellationToken, LanguageModel, ModelConfig, Tool), tokio, tracing, crate::types (AgentHooks, AgentMessage, AgentState), crate::agent_loop, crate::event, alva_agent_context
// OUTPUT: Agent
// POS:    Public agent handle — owns model, hooks, state, and cancellation; exposes prompt(), steer(), follow_up().
use std::sync::Arc;

use alva_types::{CancellationToken, LanguageModel, ModelConfig, Tool};
use tokio::sync::{mpsc, Mutex};
use tracing::error;

use crate::agent_loop::run_agent_loop;
use crate::event::AgentEvent;
use crate::types::{AgentHooks, AgentMessage, AgentState};

/// The public agent handle.
///
/// Owns the model, config, state, and cancellation machinery. Callers interact
/// with the agent through this struct and receive events via an unbounded
/// channel returned from [`Agent::prompt`].
pub struct Agent {
    model: Arc<dyn LanguageModel>,
    config: Arc<Mutex<AgentHooks>>,
    state: Arc<Mutex<AgentState>>,
    cancel: CancellationToken,

    /// Channel used by external code to inject steering messages into the loop.
    steering_tx: mpsc::UnboundedSender<Vec<AgentMessage>>,
    steering_rx: Arc<Mutex<mpsc::UnboundedReceiver<Vec<AgentMessage>>>>,

    /// Channel used by external code to inject follow-up messages.
    follow_up_tx: mpsc::UnboundedSender<Vec<AgentMessage>>,
    follow_up_rx: Arc<Mutex<mpsc::UnboundedReceiver<Vec<AgentMessage>>>>,
}

impl Agent {
    /// Create a new agent.
    ///
    /// * `model` — the LLM backend.
    /// * `session_id` — unique session identifier.
    /// * `system_prompt` — initial system prompt.
    /// * `config` — hooks & settings.
    pub fn new(
        model: Arc<dyn LanguageModel>,
        session_id: impl Into<String>,
        system_prompt: impl Into<String>,
        config: AgentHooks,
    ) -> Self {
        let (steering_tx, steering_rx) = mpsc::unbounded_channel();
        let (follow_up_tx, follow_up_rx) = mpsc::unbounded_channel();

        Self {
            model,
            config: Arc::new(Mutex::new(config)),
            state: Arc::new(Mutex::new(AgentState::new(
                session_id,
                system_prompt.into(),
                ModelConfig::default(),
            ))),
            cancel: CancellationToken::new(),
            steering_tx,
            steering_rx: Arc::new(Mutex::new(steering_rx)),
            follow_up_tx,
            follow_up_rx: Arc::new(Mutex::new(follow_up_rx)),
        }
    }

    /// Set the context plugin and SDK.
    pub fn set_context_plugin(
        &self,
        plugin: Arc<dyn alva_agent_context::ContextHooks>,
        sdk: Arc<dyn alva_agent_context::ContextHandle>,
    ) {
        let mut config = self.config.blocking_lock();
        config.context_plugin = plugin;
        config.context_sdk = sdk;
    }

    /// Set the message store for turn-based persistence.
    pub fn set_message_store(&self, store: Arc<dyn alva_agent_context::MessageStore>) {
        let mut config = self.config.blocking_lock();
        config.message_store = Some(store);
    }

    /// Start executing the agent with the given user messages.
    ///
    /// Returns an unbounded receiver that yields `AgentEvent`s. The agent loop
    /// runs on a spawned tokio task.
    pub fn prompt(
        &self,
        messages: Vec<AgentMessage>,
    ) -> mpsc::UnboundedReceiver<AgentEvent> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let model = self.model.clone();
        let state = self.state.clone();
        let config = self.config.clone();
        let cancel = self.cancel.clone();
        let steering_rx = self.steering_rx.clone();
        let follow_up_rx = self.follow_up_rx.clone();

        tokio::spawn(async move {
            // Append user messages to state.
            {
                let mut st = state.lock().await;
                st.messages.extend(messages);
            }

            // Wire steering/follow-up channels into config hooks.
            {
                let mut cfg = config.lock().await;

                // Steering hook — drain any messages from the channel.
                let steering_rx_clone = steering_rx.clone();
                cfg.get_steering_messages.push(Arc::new(move |_ctx| {
                    // We need to try_lock since we are inside a sync closure.
                    // If the lock is contended (shouldn't happen in practice),
                    // we simply return no messages.
                    match steering_rx_clone.try_lock() {
                        Ok(mut rx) => {
                            let mut msgs = Vec::new();
                            while let Ok(batch) = rx.try_recv() {
                                msgs.extend(batch);
                            }
                            msgs
                        }
                        Err(_) => Vec::new(),
                    }
                }));

                // Follow-up hook — same pattern.
                let follow_up_rx_clone = follow_up_rx.clone();
                cfg.get_follow_up_messages.push(Arc::new(move |_ctx| {
                    match follow_up_rx_clone.try_lock() {
                        Ok(mut rx) => {
                            let mut msgs = Vec::new();
                            while let Ok(batch) = rx.try_recv() {
                                msgs.extend(batch);
                            }
                            msgs
                        }
                        Err(_) => Vec::new(),
                    }
                }));
            }

            // Run the loop. We lock the state for the entire duration —
            // callers use `messages()` to snapshot state, which takes the
            // lock briefly.
            let mut st = state.lock().await;
            let cfg = config.lock().await;

            if let Err(e) = run_agent_loop(
                &mut st,
                model.as_ref(),
                &cfg,
                &cancel,
                &event_tx,
            )
            .await
            {
                error!(error = %e, "agent loop failed");
                // AgentEnd with error is already emitted by run_agent_loop.
            }
        });

        event_rx
    }

    /// Cancel the currently running agent loop.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Inject steering messages into the running loop.
    ///
    /// These will be picked up by the steering hook at the end of the next
    /// tool-call cycle.
    pub fn steer(&self, messages: Vec<AgentMessage>) {
        let _ = self.steering_tx.send(messages);
    }

    /// Inject follow-up messages after the inner loop completes.
    pub fn follow_up(&self, messages: Vec<AgentMessage>) {
        let _ = self.follow_up_tx.send(messages);
    }

    /// Get a snapshot of the current message history.
    pub async fn messages(&self) -> Vec<AgentMessage> {
        let st = self.state.lock().await;
        st.messages.clone()
    }

    /// Replace the tool set.
    pub async fn set_tools(&self, tools: Vec<Arc<dyn Tool>>) {
        let mut st = self.state.lock().await;
        st.tools = tools;
    }

    /// Replace the model.
    pub fn set_model(&mut self, model: Arc<dyn LanguageModel>) {
        self.model = model;
    }

    /// Replace the model config (temperature, max_tokens, etc).
    pub async fn set_model_config(&self, model_config: ModelConfig) {
        let mut st = self.state.lock().await;
        st.model_config = model_config;
    }

    /// Enable or disable streaming mode.
    ///
    /// When streaming is enabled, the agent loop uses `model.stream()` instead
    /// of `model.complete()` and emits `AgentEvent::MessageUpdate` events with
    /// `StreamEvent` deltas.
    pub async fn set_streaming(&self, streaming: bool) {
        let mut st = self.state.lock().await;
        st.is_streaming = streaming;
    }

    /// Gracefully shut down the agent, releasing plugin resources.
    ///
    /// This calls `dispose()` on the context plugin. Must be called explicitly
    /// because `Drop` cannot run async code.
    pub async fn shutdown(&self) {
        let config = self.config.lock().await;
        if let Err(e) = config.context_plugin.dispose().await {
            error!(error = %e, "context plugin dispose failed");
        }
    }
}
