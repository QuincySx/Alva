// INPUT:  gpui, gpui_component (Input, InputState, InputEvent, Button, ButtonVariants, Disableable), crate::models, crate::chat, crate::theme
// OUTPUT: pub struct InputBox
// POS:    Chat input widget with model selector, attachment, skills, and send/stop buttons.
use gpui::{prelude::*, Context, Entity, Render, Subscription, Window, div, px};

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{InputEvent, InputState, Input};
use gpui_component::{Disableable, Sizable};

use crate::chat::{GpuiChatConfig, GpuiChatEvent};
use crate::models::{AgentModel, ChatModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;
use crate::types::AgentStatusKind;

pub struct InputBox {
    input_state: Entity<InputState>,
    workspace_model: Entity<WorkspaceModel>,
    chat_model: Entity<ChatModel>,
    agent_model: Entity<AgentModel>,
    #[allow(dead_code)]
    settings_model: Entity<SettingsModel>,
    _subscriptions: Vec<Subscription>,
}

impl InputBox {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        chat_model: Entity<ChatModel>,
        agent_model: Entity<AgentModel>,
        settings_model: Entity<SettingsModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("继续对话...")
        });

        let mut subscriptions = Vec::new();

        // Enter to send
        subscriptions.push(cx.subscribe_in(
            &input_state,
            window,
            |this, _state, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { secondary } = event {
                    if !secondary {
                        this.send_message(window, cx);
                    }
                }
            },
        ));

        // Subscribe to agent model to update send button state
        subscriptions.push(cx.subscribe(&agent_model, |_this, _model, _event, cx| {
            cx.notify();
        }));

        Self {
            input_state,
            workspace_model,
            chat_model,
            agent_model,
            settings_model,
            _subscriptions: subscriptions,
        }
    }

    fn send_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.input_state.read(cx).value().to_string();
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }

        let session_id = {
            let ws = self.workspace_model.read(cx);
            match ws.selected_session_id.clone() {
                Some(id) => id,
                None => return,
            }
        };

        // Check if agent is already running
        {
            let agent = self.agent_model.read(cx);
            if let Some(status) = agent.get_status(&session_id) {
                if status.kind == AgentStatusKind::Running {
                    return;
                }
            }
        }

        // Ensure GpuiChat exists
        let chat_entity = {
            let needs_create = {
                let cm = self.chat_model.read(cx);
                cm.get_chat(&session_id).is_none()
            };

            if needs_create {
                let config = GpuiChatConfig {
                    session_id: session_id.clone(),
                };
                self.chat_model.update(cx, |model, cx| {
                    model.get_or_create_chat(&session_id, config, cx)
                })
            } else {
                self.chat_model
                    .read(cx)
                    .get_chat(&session_id)
                    .unwrap()
                    .clone()
            }
        };

        // Mark agent as running
        self.agent_model.update(cx, |model, cx| {
            model.set_status(&session_id, AgentStatusKind::Running, cx);
        });

        // Subscribe to chat events
        {
            let agent_model = self.agent_model.clone();
            let sid = session_id.clone();
            let sub = cx.subscribe(&chat_entity, move |_this, chat, _event: &GpuiChatEvent, cx| {
                let chat = chat.read(cx);
                if !chat.is_running() {
                    agent_model.update(cx, |model, cx| {
                        model.set_status(&sid, AgentStatusKind::Idle, cx);
                    });
                }
                cx.notify();
            });
            self._subscriptions.push(sub);
        }

        // Send
        chat_entity.update(cx, |chat, cx| {
            chat.send_message(&text, cx);
        });

        // Clear input
        self.input_state.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        cx.notify();
    }

    fn stop_agent(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let session_id = {
            let ws = self.workspace_model.read(cx);
            match ws.selected_session_id.clone() {
                Some(id) => id,
                None => return,
            }
        };

        {
            let cm = self.chat_model.read(cx);
            if let Some(chat) = cm.get_chat(&session_id) {
                chat.read(cx).stop();
            }
        }

        self.agent_model.update(cx, |model, cx| {
            model.set_status(&session_id, AgentStatusKind::Idle, cx);
        });
    }

    fn is_agent_running(&self, cx: &Context<Self>) -> bool {
        let ws = self.workspace_model.read(cx);
        if let Some(ref sid) = ws.selected_session_id {
            let agent = self.agent_model.read(cx);
            if let Some(status) = agent.get_status(sid) {
                return status.kind == AgentStatusKind::Running;
            }
        }
        false
    }
}

impl Render for InputBox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let has_text = !self.input_state.read(cx).value().trim().is_empty();
        let has_session = self.workspace_model.read(cx).selected_session_id.is_some();
        let is_running = self.is_agent_running(cx);
        let can_send = has_text && has_session && !is_running;

        div()
            .flex()
            .flex_col()
            .w_full()
            .px(px(16.))
            .pb(px(12.))
            // Input container with border
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .rounded(px(12.))
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface)
                    .overflow_hidden()
                    // Text input area
                    .child(
                        div()
                            .p(px(12.))
                            .child(
                                Input::new(&self.input_state)
                                    .disabled(is_running),
                            ),
                    )
                    // Toolbar
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .px(px(12.))
                            .py(px(8.))
                            .border_t_1()
                            .border_color(theme.border_subtle)
                            // Model selector (left)
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(4.))
                                    .px(px(8.))
                                    .py(px(4.))
                                    .rounded(px(6.))
                                    .text_xs()
                                    .text_color(theme.text_muted)
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.surface_hover))
                                    .child("Alva Agent")
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme.text_subtle)
                                            .child("\u{25BE}"),
                                    ),
                            )
                            // Spacer
                            .child(div().flex_1())
                            // Attachment button
                            .child(
                                Button::new("attach-btn")
                                    .label("\u{1F4CE}")
                                    .ghost()
                                    .small()
                                    .disabled(true),
                            )
                            // Skills button
                            .child(
                                Button::new("skills-btn")
                                    .label("\u{26A1}")
                                    .ghost()
                                    .small()
                                    .disabled(true),
                            )
                            // Send / Stop button
                            .child(if is_running {
                                Button::new("stop-btn")
                                    .label("停止")
                                    .outline()
                                    .small()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.stop_agent(window, cx);
                                    }))
                                    .into_any_element()
                            } else {
                                Button::new("send-btn")
                                    .label("发送")
                                    .primary()
                                    .small()
                                    .disabled(!can_send)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.send_message(window, cx);
                                    }))
                                    .into_any_element()
                            }),
                    ),
            )
    }
}
