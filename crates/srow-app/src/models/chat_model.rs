// INPUT:  std::collections::HashMap, gpui (Context, Entity, EventEmitter, Subscription),
//         crate::chat (GpuiChat, GpuiChatConfig, GpuiChatEvent)
// OUTPUT: pub struct ChatModel, pub enum ChatModelEvent
// POS:    GPUI model managing per-session GpuiChat instances.
//         Subscribes to each GpuiChat and re-emits ChatUpdated so views like MessageList
//         can re-render when messages change.
use std::collections::HashMap;

use gpui::{AppContext, Context, Entity, EventEmitter, Subscription};

use crate::chat::{GpuiChat, GpuiChatConfig, GpuiChatEvent};

pub struct ChatModel {
    /// session_id -> GpuiChat entity
    pub chats: HashMap<String, Entity<GpuiChat>>,
    /// Keep subscriptions alive so we continue to receive GpuiChatEvent.
    _subscriptions: Vec<Subscription>,
}

pub enum ChatModelEvent {
    ChatCreated { session_id: String },
    /// Forwarded from GpuiChatEvent::Updated — signals that messages changed.
    ChatUpdated { session_id: String },
}

impl EventEmitter<ChatModelEvent> for ChatModel {}

impl ChatModel {
    /// Get or create a GpuiChat for the given session.
    pub fn get_or_create_chat(
        &mut self,
        session_id: &str,
        config: GpuiChatConfig,
        cx: &mut Context<Self>,
    ) -> Entity<GpuiChat> {
        if let Some(chat) = self.chats.get(session_id) {
            return chat.clone();
        }

        let chat = cx.new(|cx| GpuiChat::new(config, cx));

        // Subscribe to the GpuiChat entity and forward its Updated events.
        let sid = session_id.to_string();
        let sub = cx.subscribe(&chat, move |_this, _chat, _event: &GpuiChatEvent, cx| {
            cx.emit(ChatModelEvent::ChatUpdated {
                session_id: sid.clone(),
            });
            cx.notify();
        });
        self._subscriptions.push(sub);

        self.chats.insert(session_id.to_string(), chat.clone());
        tracing::info!(session_id = %session_id, "model_event: chat_created");
        cx.emit(ChatModelEvent::ChatCreated {
            session_id: session_id.to_string(),
        });
        cx.notify();
        chat
    }

    /// Get an existing GpuiChat, if one exists for this session.
    pub fn get_chat(&self, session_id: &str) -> Option<&Entity<GpuiChat>> {
        self.chats.get(session_id)
    }
}

impl Default for ChatModel {
    fn default() -> Self {
        Self {
            chats: HashMap::new(),
            _subscriptions: Vec::new(),
        }
    }
}
