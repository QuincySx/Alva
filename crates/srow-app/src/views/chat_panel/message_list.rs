// INPUT:  gpui, crate::models (ChatModel, WorkspaceModel), crate::chat (GpuiChat, GpuiChatEvent), crate::theme,
//         srow_core (UIMessage, UIMessagePart, UIMessageRole, TextPartState, ToolState, ChatStatus),
//         super::{message_bubble, tool_call_block, thinking_block, agent_block}
// OUTPUT: pub struct MessageList
// POS:    Scrollable GPUI view rendering UIMessage parts via dedicated component helpers.

use std::collections::HashMap;

use gpui::{prelude::*, AnyElement, Context, Entity, Render, Window, div};

use crate::chat::GpuiChatEvent;
use crate::models::{ChatModel, WorkspaceModel};
use crate::theme::Theme;
use srow_core::ui_message::{UIMessage, UIMessagePart, UIMessageRole, TextPartState};
use srow_core::ui_message_stream::ChatStatus;

use super::message_bubble::{render_user_message, render_assistant_message, render_system_message};
use super::tool_call_block::render_tool_call;
use super::thinking_block::render_thinking;
use super::agent_block::render_completed_agent;

pub struct MessageList {
    pub workspace_model: Entity<WorkspaceModel>,
    pub chat_model: Entity<ChatModel>,
    scroll_handle: gpui::ScrollHandle,
    _chat_subscription: Option<gpui::Subscription>,
    collapse_states: HashMap<String, bool>,
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

        let mut this = Self {
            workspace_model,
            chat_model,
            scroll_handle: gpui::ScrollHandle::new(),
            _chat_subscription: None,
            collapse_states: HashMap::new(),
        };
        this.subscribe_current_chat(cx);
        this
    }

    fn subscribe_current_chat(&mut self, cx: &mut Context<Self>) {
        let chat_entity = {
            let ws = self.workspace_model.read(cx);
            let sid = match ws.selected_session_id.as_ref() {
                Some(s) => s.clone(),
                None => {
                    self._chat_subscription = None;
                    return;
                }
            };
            let chat_model = self.chat_model.read(cx);
            chat_model.get_chat(&sid).cloned()
        };

        if let Some(entity) = chat_entity {
            self._chat_subscription =
                Some(cx.subscribe(&entity, |this, _chat, _event: &GpuiChatEvent, cx| {
                    this.scroll_handle.scroll_to_bottom();
                    cx.notify();
                }));
        } else {
            self._chat_subscription = None;
        }
    }

    #[allow(dead_code)]
    fn toggle_collapse(&mut self, part_id: String, _cx: &mut Context<Self>) {
        let entry = self.collapse_states.entry(part_id).or_insert(true);
        *entry = !*entry;
    }

    fn render_message(&self, msg: &UIMessage, theme: &Theme) -> AnyElement {
        let mut parts_elements: Vec<AnyElement> = Vec::new();
        let mut reasoning_index: usize = 0;

        for part in &msg.parts {
            let element: AnyElement = match part {
                UIMessagePart::Text { text, state } => {
                    let is_streaming = matches!(state, Some(TextPartState::Streaming));
                    match msg.role {
                        UIMessageRole::User => {
                            render_user_message(text, is_streaming, theme).into_any_element()
                        }
                        UIMessageRole::Assistant => {
                            render_assistant_message(text, is_streaming, theme).into_any_element()
                        }
                        UIMessageRole::System => {
                            render_system_message(text, theme).into_any_element()
                        }
                    }
                }
                UIMessagePart::Reasoning { text, state } => {
                    let is_streaming = matches!(state, Some(TextPartState::Streaming));
                    let part_id = format!("{}:reasoning:{}", msg.id, reasoning_index);
                    reasoning_index += 1;
                    // Default: collapsed (expanded = false), streaming always expanded
                    let expanded = if is_streaming {
                        true
                    } else {
                        self.collapse_states.get(&part_id).copied().unwrap_or(false)
                    };
                    render_thinking(text, is_streaming, expanded, theme).into_any_element()
                }
                UIMessagePart::Tool {
                    id,
                    tool_name,
                    input,
                    state: tool_state,
                    output,
                    error,
                    title,
                    ..
                } => {
                    let display_name = title.as_deref().unwrap_or(tool_name.as_str());
                    // Default: collapsed (collapsed = true)
                    let collapsed = self.collapse_states.get(id).copied().unwrap_or(true);
                    render_tool_call(
                        display_name,
                        input,
                        tool_state,
                        output.as_ref(),
                        error.as_deref(),
                        collapsed,
                        theme,
                    )
                    .into_any_element()
                }
                UIMessagePart::Custom { id: _, data } => {
                    // Check for agent_block convention
                    if data.get("type").and_then(|v| v.as_str()) == Some("agent_block") {
                        let agent_name = data
                            .get("agent_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Agent");
                        let summary = data
                            .get("summary")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");
                        let success =
                            data.get("status").and_then(|v| v.as_str()) == Some("completed");
                        render_completed_agent(agent_name, summary, success, theme)
                            .into_any_element()
                    } else {
                        div().into_any_element() // skip unknown custom parts
                    }
                }
                _ => div().into_any_element(), // skip unhandled part types
            };
            parts_elements.push(element);
        }

        div()
            .flex()
            .flex_col()
            .gap_1()
            .children(parts_elements)
            .into_any_element()
    }

    fn render_empty_state(&self, theme: &Theme) -> AnyElement {
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
                            .child("Messages will appear here"),
                    ),
            )
            .into_any_element()
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

        let text_muted = theme.text_muted;

        let mut container = div()
            .id("message-list")
            .flex()
            .flex_col()
            .flex_1()
            .overflow_scroll()
            .track_scroll(&self.scroll_handle)
            .p_4()
            .gap_3();

        if messages.is_empty() && status == ChatStatus::Ready {
            // Show empty state
            container = container.child(self.render_empty_state(&theme));
        } else {
            for msg in messages.iter() {
                container = container.child(self.render_message(msg, &theme));
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
