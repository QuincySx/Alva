use std::future::Future;
use std::pin::Pin;
use srow_core::ui_message::UIMessage;
use srow_core::ui_message_stream::{FinishReason, TokenUsage};
use srow_core::error::ChatError;
use crate::transport::ChatTransport;
use super::chat_state::ChatState;

/// Initialization parameters for AbstractChat
pub struct ChatInit<S: ChatState> {
    pub id: String,
    pub state: S,
    pub transport: Box<dyn ChatTransport>,
    pub runtime_handle: tokio::runtime::Handle,
    pub generate_id: Option<Box<dyn Fn() -> String + Send + Sync>>,
    pub initial_messages: Vec<UIMessage>,
    pub on_tool_call: Option<AsyncToolCallHandler>,
    pub on_finish: Option<Box<dyn Fn(FinishInfo) + Send + Sync>>,
    pub on_error: Option<Box<dyn Fn(ChatError) + Send + Sync>>,
    pub send_automatically_when: Option<Box<dyn Fn(&UIMessage) -> bool + Send + Sync>>,
}

/// Async tool call handler — tool execution is inherently async
pub type AsyncToolCallHandler = Box<
    dyn Fn(ToolCallInfo) -> Pin<Box<dyn Future<Output = ToolCallResult> + Send>>
        + Send
        + Sync,
>;

pub struct SendOptions {
    pub metadata: Option<serde_json::Value>,
}

impl Default for SendOptions {
    fn default() -> Self {
        Self { metadata: None }
    }
}

pub struct RegenerateOptions {
    pub metadata: Option<serde_json::Value>,
}

impl Default for RegenerateOptions {
    fn default() -> Self {
        Self { metadata: None }
    }
}

#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone)]
pub enum ToolCallResult {
    Output(serde_json::Value),
    Error(String),
    /// Don't handle client-side, let the engine execute it
    Unhandled,
}

#[derive(Debug, Clone)]
pub struct FinishInfo {
    pub message: UIMessage,
    pub finish_reason: FinishReason,
    pub usage: Option<TokenUsage>,
}
