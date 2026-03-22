// INPUT:  gpui, agent_base (MessageRole), agent_core (AgentMessage),
//         crate::models (ChatModel, WorkspaceModel), crate::theme
// OUTPUT: pub struct MessageList
// POS:    Scrollable GPUI view rendering chat messages from agent-core.

use gpui::{prelude::*, Context, Entity, Render, Window, div};

use agent_base::MessageRole;
use agent_core::AgentMessage;

use crate::models::{ChatModel, WorkspaceModel};
use crate::theme::Theme;

pub struct MessageList {
    pub workspace_model: Entity<WorkspaceModel>,
    pub chat_model: Entity<ChatModel>,
    scroll_handle: gpui::ScrollHandle,
}

impl MessageList {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        chat_model: Entity<ChatModel>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&workspace_model, |_this, _model, _event, cx| {
            cx.notify();
        })
        .detach();

        cx.subscribe(&chat_model, |_this, _model, _event, cx| {
            cx.notify();
        })
        .detach();

        Self {
            workspace_model,
            chat_model,
            scroll_handle: gpui::ScrollHandle::new(),
        }
    }

    /// Collect messages from the current session's GpuiChat, if one exists.
    fn get_messages(&self, cx: &Context<Self>) -> Vec<AgentMessage> {
        let ws = self.workspace_model.read(cx);
        let session_id = match ws.selected_session_id.as_ref() {
            Some(id) => id,
            None => return Vec::new(),
        };

        let cm = self.chat_model.read(cx);
        match cm.get_chat(session_id) {
            Some(chat_entity) => {
                let chat = chat_entity.read(cx);
                chat.messages().to_vec()
            }
            None => Vec::new(),
        }
    }

    /// Render the empty state shown when there are no messages.
    fn render_empty_state(&self, theme: &Theme) -> gpui::AnyElement {
        div()
            .id("message-list")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_scroll()
            .track_scroll(&self.scroll_handle)
            .p_4()
            .gap_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_1()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .text_lg()
                                    .text_color(theme.text_muted)
                                    .child("Start a conversation"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.text_muted)
                                    .child("Type a message below to begin"),
                            ),
                    ),
            )
            .into_any_element()
    }
}

impl Render for MessageList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let messages = self.get_messages(cx);

        if messages.is_empty() {
            return self.render_empty_state(&theme);
        }

        let mut elements: Vec<gpui::AnyElement> = Vec::new();

        for msg in &messages {
            match msg {
                AgentMessage::Standard(message) => match message.role {
                    MessageRole::User => {
                        let text = message.text_content();
                        elements.push(
                            render_user_bubble(&text, &theme).into_any_element(),
                        );
                    }
                    MessageRole::Assistant => {
                        let text = message.text_content();
                        elements.push(
                            render_assistant_bubble(&text, &theme).into_any_element(),
                        );
                    }
                    _ => {
                        // System / Tool messages are not rendered for now.
                    }
                },
                AgentMessage::Custom { .. } => {
                    // Custom messages are not rendered for now.
                }
            }
        }

        div()
            .id("message-list")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_scroll()
            .track_scroll(&self.scroll_handle)
            .p_4()
            .gap_3()
            .children(elements)
            .into_any_element()
    }
}

// ---------------------------------------------------------------------------
// Message bubble helpers
// ---------------------------------------------------------------------------

fn render_user_bubble(text: &str, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .justify_end()
        .child(
            div()
                .max_w(gpui::px(480.0))
                .px_4()
                .py_2()
                .rounded_lg()
                .bg(theme.accent)
                .text_color(theme.selected_text)
                .text_sm()
                .child(text.to_string()),
        )
}

fn render_assistant_bubble(text: &str, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .justify_start()
        .child(
            div()
                .max_w(gpui::px(480.0))
                .px_4()
                .py_2()
                .rounded_lg()
                .bg(theme.surface)
                .text_color(theme.text)
                .text_sm()
                .child(text.to_string()),
        )
}
