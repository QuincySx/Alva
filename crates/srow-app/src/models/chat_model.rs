// INPUT:  std::collections::HashMap, gpui (Context, Entity, EventEmitter), crate::chat (GpuiChat, GpuiChatConfig)
// OUTPUT: pub struct ChatModel, pub enum ChatModelEvent
// POS:    GPUI model managing per-session GpuiChat instances and input drafts.
use std::collections::HashMap;

use gpui::{AppContext, Context, Entity, EventEmitter};

use crate::chat::{GpuiChat, GpuiChatConfig};

pub struct ChatModel {
    /// session_id -> GpuiChat entity
    pub chats: HashMap<String, Entity<GpuiChat>>,
}

pub enum ChatModelEvent {
    ChatCreated { session_id: String },
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
        self.chats.insert(session_id.to_string(), chat.clone());
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
        }
    }
}
