use gpui::{prelude::*, Context, Entity, FontWeight, Render, Window, div, px};

use crate::models::{ChatModel, WorkspaceModel};
use crate::theme::Theme;
use crate::types::{MessageContent, MessageRole};

pub struct MessageList {
    pub workspace_model: Entity<WorkspaceModel>,
    pub chat_model: Entity<ChatModel>,
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
        }
    }
}

impl Render for MessageList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window);
        let ws_model = self.workspace_model.read(cx);
        let current_session_id = ws_model.selected_session_id.clone();

        let chat = self.chat_model.read(cx);

        let messages: Vec<_> = current_session_id
            .as_ref()
            .map(|sid| chat.get_messages(sid).to_vec())
            .unwrap_or_default();

        let streaming_text = current_session_id
            .as_ref()
            .and_then(|sid| chat.get_streaming_buffer(sid))
            .map(|s| s.to_string());

        let text_color = theme.text;
        let text_muted = theme.text_muted;
        let surface = theme.surface;
        let accent = theme.accent;

        div()
            .id("message-list")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_scroll()
            .p_4()
            .gap_3()
            .children(messages.into_iter().map(move |msg| {
                let is_user = msg.role == MessageRole::User;
                let content_text = match &msg.content {
                    MessageContent::Text { text } => text.clone(),
                    MessageContent::ToolCallStart { tool_name, .. } => {
                        format!("[Calling tool: {}...]", tool_name)
                    }
                    MessageContent::ToolCallEnd { output, is_error, .. } => {
                        if *is_error {
                            format!("[Tool error: {}]", output)
                        } else {
                            format!("[Tool result: {}]", output)
                        }
                    }
                };

                div()
                    .flex()
                    .w_full()
                    .when(is_user, |el| el.justify_end())
                    .when(!is_user, |el| el.justify_start())
                    .child(
                        div()
                            .max_w(px(500.))
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .text_sm()
                            .when(is_user, |el| {
                                el.bg(accent).text_color(gpui::white())
                            })
                            .when(!is_user, |el| {
                                el.bg(surface).text_color(text_color)
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .mb_1()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .when(is_user, |el| el.text_color(gpui::white().opacity(0.8)))
                                    .when(!is_user, |el| el.text_color(text_muted))
                                    .child(if is_user { "You" } else { "Assistant" }),
                            )
                            .child(content_text),
                    )
            }))
            .when_some(streaming_text, |el, text| {
                el.child(
                    div()
                        .flex()
                        .justify_start()
                        .child(
                            div()
                                .max_w(px(500.))
                                .px_3()
                                .py_2()
                                .rounded_lg()
                                .text_sm()
                                .bg(surface)
                                .text_color(text_color)
                                .child(
                                    div()
                                        .text_xs()
                                        .mb_1()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(text_muted)
                                        .child("Assistant"),
                                )
                                .child(text)
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(text_muted)
                                        .child(" ..."),
                                ),
                        ),
                )
            })
    }
}
