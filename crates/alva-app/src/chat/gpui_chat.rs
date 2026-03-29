// INPUT:  gpui, alva_app_core (V2: AgentState, AgentConfig, run_agent), alva_types, tokio, std::sync::Arc, std::pin::Pin
// OUTPUT: pub struct GpuiChat, pub struct GpuiChatConfig, pub enum GpuiChatEvent, pub struct SharedRuntime
// POS:    GPUI Entity wrapping V2 agent engine that bridges async agent events to GPUI's sync UI thread.
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use gpui::{Context, EventEmitter};
use tokio::sync::mpsc;

use alva_app_core::alva_types::{
    AgentError, ContentBlock, LanguageModel, Message, MessageRole, ModelConfig, StreamEvent, Tool,
};
use alva_app_core::{AgentEvent, AgentMessage};

/// GPUI Global holding a shared tokio runtime for all GpuiChat instances.
pub struct SharedRuntime(pub Arc<tokio::runtime::Runtime>);

impl gpui::Global for SharedRuntime {}

/// Events emitted by GpuiChat to any GPUI subscriber.
pub enum GpuiChatEvent {
    Updated,
}

impl EventEmitter<GpuiChatEvent> for GpuiChat {}

/// Configuration for creating a GpuiChat instance.
pub struct GpuiChatConfig {
    pub session_id: String,
}

// ---------------------------------------------------------------------------
// PlaceholderModel — returns canned echo responses until real LLM integration
// ---------------------------------------------------------------------------

struct PlaceholderModel;

#[async_trait]
impl LanguageModel for PlaceholderModel {
    async fn complete(
        &self,
        messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<Message, AgentError> {
        let last_user = messages
            .iter()
            .rev()
            .find(|m| m.role == MessageRole::User)
            .map(|m| m.text_content())
            .unwrap_or_default();

        Ok(Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text {
                text: format!("Echo: {}", last_user),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        })
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Pin<Box<dyn futures::Stream<Item = StreamEvent> + Send>> {
        Box::pin(futures::stream::empty())
    }

    fn model_id(&self) -> &str {
        "placeholder"
    }
}

// ---------------------------------------------------------------------------
// GpuiChat — GPUI Entity wrapping V2 agent engine
// ---------------------------------------------------------------------------

/// GPUI Entity wrapping V2 agent engine.
///
/// Holds a shared V2 AgentState + AgentConfig, a local copy of messages
/// (for synchronous UI reads), and the running state. The async agent loop
/// runs on the shared tokio runtime and sends events back via an `mpsc`
/// channel that is drained in a GPUI timer/callback.
pub struct GpuiChat {
    state: Arc<tokio::sync::Mutex<alva_app_core::AgentState>>,
    config: Arc<alva_app_core::AgentConfig>,
    cancel: alva_app_core::alva_types::CancellationToken,
    /// Local snapshot of messages for synchronous UI reads.
    messages: Vec<AgentMessage>,
    is_running: bool,
    runtime: Arc<tokio::runtime::Runtime>,
    _session_id: String,
}

impl GpuiChat {
    pub fn new(config: GpuiChatConfig, cx: &mut Context<Self>) -> Self {
        // Obtain the shared tokio runtime from the GPUI global.
        let runtime = cx
            .try_global::<SharedRuntime>()
            .map(|sr| sr.0.clone())
            .unwrap_or_else(|| {
                tracing::warn!("SharedRuntime global not set; creating a fallback runtime");
                Arc::new(
                    tokio::runtime::Builder::new_multi_thread()
                        .enable_all()
                        .build()
                        .expect("failed to build fallback tokio runtime"),
                )
            });

        // Build V2 AgentState
        let model: Arc<dyn LanguageModel> = Arc::new(PlaceholderModel);
        let session: Arc<dyn alva_app_core::alva_types::session::AgentSession> =
            Arc::new(alva_app_core::alva_types::session::InMemorySession::new());
        let state = alva_app_core::AgentState {
            model,
            tools: vec![],
            session,
            extensions: alva_app_core::Extensions::new(),
        };

        // Build V2 AgentConfig
        let agent_config = alva_app_core::AgentConfig {
            middleware: alva_app_core::MiddlewareStack::new(),
            system_prompt: "You are a helpful assistant.".to_string(),
            max_iterations: 100,
            model_config: alva_app_core::alva_types::ModelConfig::default(),
            context_window: 0,
        };

        let cancel = alva_app_core::alva_types::CancellationToken::new();

        // Register this component in the debug ActionRegistry for HTTP inspection.
        #[cfg(debug_assertions)]
        {
            if let Some(action_reg) = cx.try_global::<crate::DebugActionRegistry>() {
                let _weak = cx.entity().downgrade();
                let _weak2 = _weak.clone();
                action_reg.0.register(
                    "chat_panel",
                    alva_app_debug::RegisteredView {
                        action_fn: Box::new(move |method, _args| {
                            match method {
                                "send_message" => {
                                    Ok(serde_json::json!({"status": "acknowledged"}))
                                }
                                _ => Err(format!("unknown method: {method}")),
                            }
                        }),
                        state_fn: Box::new(move || {
                            Some(serde_json::json!({
                                "registered": true,
                                "type": "GpuiChat"
                            }))
                        }),
                        methods: vec!["send_message".into()],
                    },
                );
            }
        }

        Self {
            state: Arc::new(tokio::sync::Mutex::new(state)),
            config: Arc::new(agent_config),
            cancel,
            messages: Vec::new(),
            is_running: false,
            runtime,
            _session_id: config.session_id,
        }
    }

    /// Send a user message through the agent.
    pub fn send_message(&mut self, text: &str, cx: &mut Context<Self>) {
        let user_msg = AgentMessage::Standard(Message::user(text));

        // Add user message to local snapshot immediately.
        self.messages.push(user_msg.clone());
        self.is_running = true;
        cx.emit(GpuiChatEvent::Updated);
        cx.notify();

        // Prepare for the V2 run_agent call
        let state = self.state.clone();
        let config = self.config.clone();
        let cancel = self.cancel.clone();
        let runtime = self.runtime.clone();

        let (notify_tx, notify_rx) = mpsc::unbounded_channel::<AgentEvent>();

        // Spawn the agent loop on the tokio runtime.
        runtime.spawn(async move {
            let (event_tx, mut event_rx) = mpsc::unbounded_channel();

            let state_clone = state.clone();
            let config_clone = config.clone();
            let cancel_clone = cancel.clone();

            // Spawn the V2 run_agent
            tokio::spawn(async move {
                let mut st = state_clone.lock().await;
                let _ = alva_app_core::run_agent(
                    &mut st,
                    &config_clone,
                    cancel_clone,
                    vec![user_msg],
                    event_tx,
                )
                .await;
            });

            // Forward events to the notify channel
            while let Some(event) = event_rx.recv().await {
                if notify_tx.send(event).is_err() {
                    break;
                }
            }
        });

        // Spawn a GPUI async task to drain notifications and update state.
        cx.spawn({
            let mut notify_rx = notify_rx;
            async move |this: gpui::WeakEntity<GpuiChat>, cx: &mut gpui::AsyncApp| {
                while let Some(event) = notify_rx.recv().await {
                    let should_break = matches!(event, AgentEvent::AgentEnd { .. });

                    this.update(cx, |chat: &mut GpuiChat, cx: &mut Context<GpuiChat>| {
                        chat.handle_agent_event(event, cx);
                    })
                    .ok();

                    if should_break {
                        break;
                    }
                }
            }
        })
        .detach();
    }

    /// Process a single agent event and update local state accordingly.
    fn handle_agent_event(&mut self, event: AgentEvent, cx: &mut Context<Self>) {
        match event {
            AgentEvent::MessageEnd { message } => {
                self.messages.push(message);
                cx.emit(GpuiChatEvent::Updated);
                cx.notify();
            }
            AgentEvent::AgentEnd { error } => {
                self.is_running = false;
                if let Some(err) = error {
                    tracing::error!(error = %err, "agent loop ended with error");
                }
                cx.emit(GpuiChatEvent::Updated);
                cx.notify();
            }
            _ => {}
        }
    }

    /// Get the current messages for rendering.
    pub fn messages(&self) -> &[AgentMessage] {
        &self.messages
    }

    /// Cancel the currently running agent loop.
    pub fn stop(&self) {
        self.cancel.cancel();
    }

    /// Whether the agent is currently running.
    pub fn is_running(&self) -> bool {
        self.is_running
    }

    /// Session ID accessor.
    pub fn id(&self) -> String {
        self._session_id.clone()
    }
}
