// INPUT:  gpui (Context, EventEmitter), srow_ai (AbstractChat, ChatInit, SendOptions, ChatTransport),
//         srow_core (UIMessage, UIMessagePart, ChatStatus, ChatError), tokio, futures
// OUTPUT: pub struct GpuiChat, pub struct GpuiChatConfig, pub enum GpuiChatEvent
// POS:    GPUI Entity wrapping AbstractChat; bridges async chat events to GPUI's notify cycle.
use std::sync::Arc;

use futures::StreamExt;
use gpui::{Context, EventEmitter};

use srow_ai::chat::{AbstractChat, ChatInit, SendOptions};
use srow_ai::transport::ChatTransport;
use srow_core::error::ChatError;
use srow_core::ui_message::{UIMessage, UIMessagePart};
use srow_core::ui_message_stream::ChatStatus;

use srow_ai::chat::ChatState;

use super::gpui_chat_state::GpuiChatState;

/// GPUI Global holding a shared tokio runtime for all GpuiChat instances.
pub struct SharedRuntime(pub Arc<tokio::runtime::Runtime>);

impl gpui::Global for SharedRuntime {}

/// Events emitted by GpuiChat to any GPUI subscriber.
pub enum GpuiChatEvent {
    /// The chat state was updated (messages, status, or error changed).
    Updated,
}

impl EventEmitter<GpuiChatEvent> for GpuiChat {}

/// Configuration for creating a GpuiChat instance.
pub struct GpuiChatConfig {
    pub session_id: String,
    pub transport: Box<dyn ChatTransport>,
}

/// GPUI Entity wrapping `AbstractChat<GpuiChatState>`.
///
/// The inner `AbstractChat` uses `Arc<Mutex<...>>` for interior mutability,
/// so all public methods take `&self`. The `Arc` wrapper makes it shareable
/// across the tokio runtime and the GPUI foreground.
pub struct GpuiChat {
    inner: Arc<AbstractChat<GpuiChatState>>,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl GpuiChat {
    pub fn new(config: GpuiChatConfig, cx: &mut Context<Self>) -> Self {
        let runtime = cx.global::<SharedRuntime>().0.clone();

        let (notify_tx, mut notify_rx) = futures::channel::mpsc::unbounded();

        let state = GpuiChatState::new(notify_tx);

        let chat = AbstractChat::new(ChatInit {
            id: config.session_id,
            state,
            transport: config.transport,
            runtime_handle: runtime.handle().clone(),
            generate_id: None,
            initial_messages: vec![],
            on_tool_call: None,
            on_finish: None,
            on_error: None,
            send_automatically_when: None,
        });

        let inner = Arc::new(chat);

        // Spawn a GPUI foreground task that consumes the notify channel
        // and calls cx.notify() + emits Updated event to trigger re-renders.
        cx.spawn(async move |this, cx| {
            while let Some(_kind) = notify_rx.next().await {
                let _ = this.update(cx, |_, cx| {
                    cx.emit(GpuiChatEvent::Updated);
                    cx.notify();
                });
            }
        })
        .detach();

        Self { inner, runtime }
    }

    /// Send a user text message. Spawns the request on the tokio runtime.
    pub fn send_message(&self, text: &str) {
        tracing::info!("gpui_chat: send_message");
        let inner = self.inner.clone();
        let text = text.to_string();
        self.runtime.spawn(async move {
            inner
                .send_message(
                    vec![UIMessagePart::Text {
                        text,
                        state: None,
                    }],
                    SendOptions::default(),
                )
                .await;
        });
    }

    /// Stop the current in-flight request.
    pub fn stop(&self) {
        let inner = self.inner.clone();
        self.runtime.spawn(async move {
            inner.stop().await;
        });
    }

    /// Read the current messages from the chat state.
    pub fn messages(&self) -> Vec<UIMessage> {
        self.inner.with_state(|s| s.messages())
    }

    /// Read the current chat status.
    pub fn status(&self) -> ChatStatus {
        self.inner.with_state(|s| s.status())
    }

    /// Read the current error, if any.
    pub fn error(&self) -> Option<ChatError> {
        self.inner.with_state(|s| s.error())
    }

    /// Get the chat/session ID.
    pub fn id(&self) -> String {
        self.inner.id()
    }

    /// Add a tool output for a pending tool call.
    pub fn add_tool_output(&self, tool_call_id: &str, output: serde_json::Value) {
        let inner = self.inner.clone();
        let id = tool_call_id.to_string();
        self.runtime.spawn(async move {
            inner.add_tool_output(&id, output).await;
        });
    }

    /// Approve or deny a tool call.
    pub fn add_tool_approval(&self, tool_call_id: &str, approved: bool) {
        self.inner
            .add_tool_approval_response(tool_call_id, approved);
    }

    /// Clear error and reset status to Ready.
    pub fn clear_error(&self) {
        self.inner.clear_error();
    }
}
