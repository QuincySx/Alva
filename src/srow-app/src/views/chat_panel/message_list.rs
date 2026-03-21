// INPUT:  gpui, crate::models (ChatModel, WorkspaceModel), crate::chat (GpuiChat, GpuiChatEvent), crate::theme,
//         srow_core (UIMessage, UIMessagePart, UIMessageRole, TextPartState, ToolState, ChatStatus)
// OUTPUT: pub struct MessageList
// POS:    Scrollable GPUI view rendering UIMessage parts: text, reasoning, tool calls, and streaming status.
use gpui::{prelude::*, Context, Entity, FontWeight, Render, Window, div, px};

use crate::chat::GpuiChatEvent;
use crate::models::{ChatModel, WorkspaceModel};
use crate::theme::Theme;
use srow_core::ui_message::{UIMessagePart, UIMessageRole, TextPartState, ToolState};
use srow_core::ui_message_stream::ChatStatus;

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
        cx.subscribe(&workspace_model, |this, _model, _event, cx| {
            // When selected session changes, re-subscribe to the new GpuiChat
            this.subscribe_current_chat(cx);
            cx.notify();
        })
        .detach();

        cx.subscribe(&chat_model, |this, _model, _event, cx| {
            // When a new chat is created, re-subscribe
            this.subscribe_current_chat(cx);
            cx.notify();
        })
        .detach();

        Self {
            workspace_model,
            chat_model,
        }
    }

    fn subscribe_current_chat(&self, cx: &mut Context<Self>) {
        let chat_entity = {
            let ws = self.workspace_model.read(cx);
            let sid = match ws.selected_session_id.as_ref() {
                Some(s) => s.clone(),
                None => return,
            };
            let chat_model = self.chat_model.read(cx);
            chat_model.get_chat(&sid).cloned()
        };

        if let Some(entity) = chat_entity {
            cx.subscribe(&entity, |_this, _chat, _event: &GpuiChatEvent, cx| {
                cx.notify();
            })
            .detach();
        }
    }
}

impl Render for MessageList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let ws_model = self.workspace_model.read(cx);
        let current_session_id = ws_model.selected_session_id.clone();

        let chat_model = self.chat_model.read(cx);

        // Get messages and status from the GpuiChat if it exists
        let (messages, status) = current_session_id
            .as_ref()
            .and_then(|sid| chat_model.get_chat(sid))
            .map(|chat_entity| {
                let chat = chat_entity.read(cx);
                (chat.messages(), chat.status())
            })
            .unwrap_or_else(|| (vec![], ChatStatus::Ready));

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
            let is_user = msg.role == UIMessageRole::User;
            let is_system = msg.role == UIMessageRole::System;

            for part in &msg.parts {
                match part {
                    UIMessagePart::Text { text, state } => {
                        let content_text = text.clone();
                        let is_streaming = state.as_ref() == Some(&TextPartState::Streaming);

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
                                        .child(content_text)
                                        .when(is_streaming, |el| {
                                            el.child(
                                                div()
                                                    .text_xs()
                                                    .text_color(text_muted)
                                                    .child(" ..."),
                                            )
                                        }),
                                ),
                        );
                    }
                    UIMessagePart::Reasoning { text, state } => {
                        let content_text = text.clone();
                        let is_streaming = state.as_ref() == Some(&TextPartState::Streaming);

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
                                                .child(if is_streaming {
                                                    "Thinking..."
                                                } else {
                                                    "Thought"
                                                }),
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
                    UIMessagePart::Tool {
                        id: _,
                        tool_name,
                        state: tool_state,
                        output,
                        error,
                        title,
                        ..
                    } => {
                        let tool_display = title
                            .as_deref()
                            .unwrap_or(tool_name.as_str())
                            .to_string();

                        let is_running = matches!(
                            tool_state,
                            ToolState::InputStreaming
                                | ToolState::InputAvailable
                                | ToolState::ApprovalRequested
                                | ToolState::ApprovalResponded
                        );
                        let is_error = matches!(
                            tool_state,
                            ToolState::OutputError | ToolState::OutputDenied
                        );
                        let _is_done = matches!(tool_state, ToolState::OutputAvailable);

                        if is_running {
                            // Tool in progress
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
                                                    .bg(gpui::rgba(0xF59E0BFF)),
                                            )
                                            .child(format!("Calling tool: {}", tool_display)),
                                    ),
                            );
                        } else {
                            // Tool completed (success or error)
                            let output_text = if is_error {
                                error
                                    .as_deref()
                                    .unwrap_or("Tool error")
                                    .to_string()
                            } else {
                                output
                                    .as_ref()
                                    .map(|v| {
                                        if let Some(s) = v.as_str() {
                                            s.to_string()
                                        } else {
                                            serde_json::to_string_pretty(v)
                                                .unwrap_or_else(|_| v.to_string())
                                        }
                                    })
                                    .unwrap_or_default()
                            };

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
                                            .when(is_error, |el| {
                                                el.bg(gpui::rgba(0xEF444410))
                                                    .border_1()
                                                    .border_color(gpui::rgba(0xEF444430))
                                                    .text_color(gpui::rgba(0xEF4444FF))
                                            })
                                            .when(!is_error, |el| {
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
                                                            .when(is_error, |el| {
                                                                el.bg(gpui::rgba(0xEF4444FF))
                                                            })
                                                            .when(!is_error, |el| {
                                                                el.bg(gpui::rgba(0x10B981FF))
                                                            }),
                                                    )
                                                    .child(format!(
                                                        "{}: {}",
                                                        if is_error {
                                                            "Tool Error"
                                                        } else {
                                                            "Tool Result"
                                                        },
                                                        tool_display,
                                                    )),
                                            )
                                            .when(!output_text.is_empty(), |el| {
                                                let display = if output_text.len() > 300 {
                                                    let end = output_text
                                                        .char_indices()
                                                        .map(|(i, _)| i)
                                                        .take_while(|&i| i <= 300)
                                                        .last()
                                                        .unwrap_or(0);
                                                    format!("{}...", &output_text[..end])
                                                } else {
                                                    output_text.clone()
                                                };
                                                el.child(
                                                    div()
                                                        .mt_1()
                                                        .max_h(px(120.))
                                                        .overflow_hidden()
                                                        .text_color(text_muted)
                                                        .child(display),
                                                )
                                            }),
                                    ),
                            );
                        }
                    }
                    // Other part types: File, SourceUrl, SourceDocument, StepStart, Custom, Data
                    // Render a minimal placeholder for now
                    _ => {}
                }
            }
        }

        // Show streaming indicator
        if status == ChatStatus::Streaming || status == ChatStatus::Submitted {
            container = container.child(
                div()
                    .flex()
                    .justify_start()
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .text_xs()
                            .text_color(text_muted)
                            .child(if status == ChatStatus::Submitted {
                                "Sending..."
                            } else {
                                ""
                            }),
                    ),
            );
        }

        container
    }
}
