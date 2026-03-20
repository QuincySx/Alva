use std::collections::HashMap;

use gpui::{Context, EventEmitter};

use crate::types::{Message, MessageContent, MessageRole};

pub struct ChatModel {
    /// session_id -> messages
    pub messages: HashMap<String, Vec<Message>>,
    /// session_id -> draft text
    pub drafts: HashMap<String, String>,
    /// session_id -> streaming buffer (incomplete assistant message)
    pub streaming_buffers: HashMap<String, String>,
    /// session_id -> thinking buffer (incomplete thinking)
    pub thinking_buffers: HashMap<String, String>,
}

pub enum ChatModelEvent {
    MessageAppended { session_id: String },
    StreamDelta { session_id: String },
    StreamCompleted { session_id: String },
}

impl EventEmitter<ChatModelEvent> for ChatModel {}

impl ChatModel {
    pub fn push_user_message(&mut self, session_id: &str, text: String, cx: &mut Context<Self>) {
        let msg = Message {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            role: MessageRole::User,
            content: MessageContent::Text { text },
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        self.messages
            .entry(session_id.to_string())
            .or_default()
            .push(msg);
        cx.emit(ChatModelEvent::MessageAppended {
            session_id: session_id.to_string(),
        });
        cx.notify();
    }

    pub fn append_text_delta(&mut self, session_id: &str, delta: &str, cx: &mut Context<Self>) {
        self.streaming_buffers
            .entry(session_id.to_string())
            .or_default()
            .push_str(delta);
        cx.emit(ChatModelEvent::StreamDelta {
            session_id: session_id.to_string(),
        });
        cx.notify();
    }

    pub fn append_thinking_delta(&mut self, session_id: &str, delta: &str, cx: &mut Context<Self>) {
        self.thinking_buffers
            .entry(session_id.to_string())
            .or_default()
            .push_str(delta);
        cx.notify();
    }

    pub fn finalize_thinking(&mut self, session_id: &str, cx: &mut Context<Self>) {
        if let Some(buffer) = self.thinking_buffers.remove(session_id) {
            if !buffer.is_empty() {
                let msg = Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: session_id.to_string(),
                    role: MessageRole::Assistant,
                    content: MessageContent::Thinking { text: buffer },
                    created_at: chrono::Utc::now().timestamp_millis(),
                };
                self.messages
                    .entry(session_id.to_string())
                    .or_default()
                    .push(msg);
            }
        }
        cx.notify();
    }

    pub fn push_tool_call_start(
        &mut self,
        session_id: &str,
        tool_name: String,
        call_id: String,
        cx: &mut Context<Self>,
    ) {
        let msg = Message {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            role: MessageRole::Assistant,
            content: MessageContent::ToolCallStart { tool_name, call_id },
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        self.messages
            .entry(session_id.to_string())
            .or_default()
            .push(msg);
        cx.emit(ChatModelEvent::MessageAppended {
            session_id: session_id.to_string(),
        });
        cx.notify();
    }

    pub fn push_tool_call_end(
        &mut self,
        session_id: &str,
        call_id: String,
        output: String,
        is_error: bool,
        cx: &mut Context<Self>,
    ) {
        let msg = Message {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            role: MessageRole::Assistant,
            content: MessageContent::ToolCallEnd {
                call_id,
                output,
                is_error,
            },
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        self.messages
            .entry(session_id.to_string())
            .or_default()
            .push(msg);
        cx.emit(ChatModelEvent::MessageAppended {
            session_id: session_id.to_string(),
        });
        cx.notify();
    }

    pub fn push_error_message(&mut self, session_id: &str, error: String, cx: &mut Context<Self>) {
        let msg = Message {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.to_string(),
            role: MessageRole::System,
            content: MessageContent::Text {
                text: format!("[Error] {}", error),
            },
            created_at: chrono::Utc::now().timestamp_millis(),
        };
        self.messages
            .entry(session_id.to_string())
            .or_default()
            .push(msg);
        cx.emit(ChatModelEvent::MessageAppended {
            session_id: session_id.to_string(),
        });
        cx.notify();
    }

    pub fn finalize_stream(&mut self, session_id: &str, cx: &mut Context<Self>) {
        // Finalize thinking first if present
        self.finalize_thinking(session_id, cx);

        if let Some(buffer) = self.streaming_buffers.remove(session_id) {
            if !buffer.is_empty() {
                let msg = Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id: session_id.to_string(),
                    role: MessageRole::Assistant,
                    content: MessageContent::Text { text: buffer },
                    created_at: chrono::Utc::now().timestamp_millis(),
                };
                self.messages
                    .entry(session_id.to_string())
                    .or_default()
                    .push(msg);
            }
        }
        cx.emit(ChatModelEvent::StreamCompleted {
            session_id: session_id.to_string(),
        });
        cx.notify();
    }

    pub fn get_messages(&self, session_id: &str) -> &[Message] {
        self.messages
            .get(session_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn get_streaming_buffer(&self, session_id: &str) -> Option<&str> {
        self.streaming_buffers.get(session_id).map(|s| s.as_str())
    }

    pub fn get_thinking_buffer(&self, session_id: &str) -> Option<&str> {
        self.thinking_buffers.get(session_id).map(|s| s.as_str())
    }
}

impl Default for ChatModel {
    fn default() -> Self {
        Self {
            messages: HashMap::new(),
            drafts: HashMap::new(),
            streaming_buffers: HashMap::new(),
            thinking_buffers: HashMap::new(),
        }
    }
}
