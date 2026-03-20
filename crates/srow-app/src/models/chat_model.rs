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

    pub fn finalize_stream(&mut self, session_id: &str, cx: &mut Context<Self>) {
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
}

impl Default for ChatModel {
    fn default() -> Self {
        let mut messages = HashMap::new();
        // Pre-populate first global session with some mock messages
        messages.insert(
            "sess-g1".to_string(),
            vec![
                Message {
                    id: "msg-1".into(),
                    session_id: "sess-g1".into(),
                    role: MessageRole::User,
                    content: MessageContent::Text {
                        text: "What is the weather like today?".into(),
                    },
                    created_at: 1710950000000,
                },
                Message {
                    id: "msg-2".into(),
                    session_id: "sess-g1".into(),
                    role: MessageRole::Assistant,
                    content: MessageContent::Text {
                        text: "It looks like it will be sunny with a high of 22C today. Perfect weather for a walk!".into(),
                    },
                    created_at: 1710950010000,
                },
            ],
        );
        Self {
            messages,
            drafts: HashMap::new(),
            streaming_buffers: HashMap::new(),
        }
    }
}
