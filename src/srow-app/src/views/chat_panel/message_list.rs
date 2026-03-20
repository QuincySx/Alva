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
        let theme = Theme::for_appearance(window, cx);
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

        let thinking_text = current_session_id
            .as_ref()
            .and_then(|sid| chat.get_thinking_buffer(sid))
            .map(|s| s.to_string());

        let text_color = theme.text;
        let text_muted = theme.text_muted;
        let surface = theme.surface;
        let accent = theme.accent;
        let border = theme.border;
        let background = theme.background;

        let mut container = div()
            .id("message-list")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_scroll()
            .p_4()
            .gap_3();

        for msg in messages.iter() {
            let is_user = msg.role == MessageRole::User;
            let is_system = msg.role == MessageRole::System;

            match &msg.content {
                MessageContent::Text { text } => {
                    let content_text = text.clone();
                    container = container.child(
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
                                    .when(is_system, |el| {
                                        el.bg(gpui::rgba(0xEF444420))
                                            .text_color(gpui::rgba(0xEF4444FF))
                                            .border_1()
                                            .border_color(gpui::rgba(0xEF444440))
                                    })
                                    .when(!is_user && !is_system, |el| {
                                        el.bg(surface).text_color(text_color)
                                    })
                                    .child(
                                        div()
                                            .text_xs()
                                            .mb_1()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .when(is_user, |el| {
                                                el.text_color(gpui::white().opacity(0.8))
                                            })
                                            .when(is_system, |el| {
                                                el.text_color(gpui::rgba(0xEF4444FF))
                                            })
                                            .when(!is_user && !is_system, |el| {
                                                el.text_color(text_muted)
                                            })
                                            .child(if is_user {
                                                "You"
                                            } else if is_system {
                                                "System"
                                            } else {
                                                "Assistant"
                                            }),
                                    )
                                    .child(content_text),
                            ),
                    );
                }
                MessageContent::Thinking { text } => {
                    let content_text = text.clone();
                    container = container.child(
                        div()
                            .flex()
                            .w_full()
                            .justify_start()
                            .child(
                                div()
                                    .max_w(px(500.))
                                    .px_3()
                                    .py_2()
                                    .rounded_lg()
                                    .text_sm()
                                    .bg(background)
                                    .border_1()
                                    .border_color(border)
                                    .text_color(text_muted)
                                    .child(
                                        div()
                                            .text_xs()
                                            .mb_1()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(text_muted)
                                            .child("Thinking..."),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .italic()
                                            .max_h(px(100.))
                                            .overflow_hidden()
                                            .child(content_text),
                                    ),
                            ),
                    );
                }
                MessageContent::ToolCallStart { tool_name, call_id } => {
                    let tool_display = tool_name.clone();
                    let _call_display = call_id.clone();
                    container = container.child(
                        div()
                            .flex()
                            .w_full()
                            .justify_start()
                            .child(
                                div()
                                    .max_w(px(500.))
                                    .px_3()
                                    .py_2()
                                    .rounded_lg()
                                    .text_xs()
                                    .bg(background)
                                    .border_1()
                                    .border_color(border)
                                    .text_color(text_muted)
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .size(px(8.))
                                            .rounded_full()
                                            .bg(gpui::rgba(0xF59E0BFF)), // yellow spinner
                                    )
                                    .child(format!("Calling tool: {}", tool_display)),
                            ),
                    );
                }
                MessageContent::ToolCallEnd {
                    output,
                    is_error,
                    call_id: _,
                } => {
                    let output_text = output.clone();
                    let is_err = *is_error;
                    container = container.child(
                        div()
                            .flex()
                            .w_full()
                            .justify_start()
                            .child(
                                div()
                                    .max_w(px(500.))
                                    .px_3()
                                    .py_2()
                                    .rounded_lg()
                                    .text_xs()
                                    .when(is_err, |el| {
                                        el.bg(gpui::rgba(0xEF444410))
                                            .border_1()
                                            .border_color(gpui::rgba(0xEF444430))
                                            .text_color(gpui::rgba(0xEF4444FF))
                                    })
                                    .when(!is_err, |el| {
                                        el.bg(gpui::rgba(0x10B98110))
                                            .border_1()
                                            .border_color(gpui::rgba(0x10B98130))
                                            .text_color(gpui::rgba(0x10B981FF))
                                    })
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap_2()
                                            .child(
                                                div()
                                                    .size(px(8.))
                                                    .rounded_full()
                                                    .when(is_err, |el| {
                                                        el.bg(gpui::rgba(0xEF4444FF))
                                                    })
                                                    .when(!is_err, |el| {
                                                        el.bg(gpui::rgba(0x10B981FF))
                                                    }),
                                            )
                                            .child(if is_err {
                                                "Tool Error"
                                            } else {
                                                "Tool Result"
                                            }),
                                    )
                                    .child(
                                        div()
                                            .mt_1()
                                            .max_h(px(120.))
                                            .overflow_hidden()
                                            .text_color(text_muted)
                                            .child(if output_text.len() > 300 {
                                                let end = output_text
                                                    .char_indices()
                                                    .map(|(i, _)| i)
                                                    .take_while(|&i| i <= 300)
                                                    .last()
                                                    .unwrap_or(0);
                                                format!("{}...", &output_text[..end])
                                            } else {
                                                output_text
                                            }),
                                    ),
                            ),
                    );
                }
            }
        }

        // Show thinking buffer if active
        if let Some(thinking) = thinking_text {
            if !thinking.is_empty() {
                container = container.child(
                    div()
                        .flex()
                        .justify_start()
                        .child(
                            div()
                                .max_w(px(500.))
                                .px_3()
                                .py_2()
                                .rounded_lg()
                                .text_xs()
                                .bg(background)
                                .border_1()
                                .border_color(border)
                                .text_color(text_muted)
                                .italic()
                                .child(
                                    div()
                                        .text_xs()
                                        .mb_1()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Thinking..."),
                                )
                                .child(
                                    div()
                                        .max_h(px(80.))
                                        .overflow_hidden()
                                        .child(if thinking.len() > 200 {
                                            let start = thinking
                                                .char_indices()
                                                .map(|(i, _)| i)
                                                .find(|&i| i >= thinking.len().saturating_sub(200))
                                                .unwrap_or(0);
                                            format!("...{}", &thinking[start..])
                                        } else {
                                            thinking
                                        }),
                                ),
                        ),
                );
            }
        }

        // Show streaming buffer if active
        if let Some(text) = streaming_text {
            if !text.is_empty() {
                container = container.child(
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
                );
            }
        }

        container
    }
}
