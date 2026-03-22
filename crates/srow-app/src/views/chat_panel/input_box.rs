// INPUT:  gpui, gpui_component (Input, InputState, InputEvent, Button, ButtonVariants, Disableable),
//         crate::models, crate::chat, crate::theme, crate::types::AgentStatusKind
// OUTPUT: pub struct InputBox
// POS:    Chat input widget using gpui-component Input/Button, Enter-to-send via InputEvent subscription.
//         Transport/LLM creation commented out — depends on deleted srow-ai and Provider V4 types.
//         TODO: Rebuild transport on agent-core.
use gpui::{prelude::*, Context, Entity, Render, Subscription, Window, div};

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{InputEvent, InputState, Input};
use gpui_component::{Disableable, Sizable};

use crate::chat::GpuiChatConfig;
use crate::models::{AgentModel, ChatModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;
use crate::types::AgentStatusKind;

pub struct InputBox {
    input_state: Entity<InputState>,
    workspace_model: Entity<WorkspaceModel>,
    chat_model: Entity<ChatModel>,
    agent_model: Entity<AgentModel>,
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
                .placeholder("Type a message...")
        });

        let mut subscriptions = Vec::new();

        // Subscribe to InputEvent from the input state
        subscriptions.push(cx.subscribe_in(
            &input_state,
            window,
            |this, _state, event: &InputEvent, window, cx| {
                match event {
                    InputEvent::PressEnter { secondary } => {
                        if !secondary {
                            this.send_message(window, cx);
                        }
                    }
                    _ => {}
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
        tracing::info!("action_dispatch: send_message");
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

        // Check if agent is already running for this session
        {
            let agent = self.agent_model.read(cx);
            if let Some(status) = agent.get_status(&session_id) {
                if status.kind == AgentStatusKind::Running {
                    return; // Don't send while running
                }
            }
        }

        // Ensure the GpuiChat exists for this session
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

        // Send via GpuiChat
        // TODO: This is a no-op stub until agent-core chat transport is rebuilt
        chat_entity.read(cx).send_message(&text);

        // Clear input
        self.input_state.update(cx, |state, cx| {
            state.set_value("", window, cx);
        });
        cx.notify();
    }

    fn stop_agent(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        tracing::info!("action_dispatch: stop_agent");
        let session_id = {
            let ws = self.workspace_model.read(cx);
            match ws.selected_session_id.clone() {
                Some(id) => id,
                None => return,
            }
        };

        // Stop the chat if it exists
        {
            let cm = self.chat_model.read(cx);
            if let Some(chat) = cm.get_chat(&session_id) {
                chat.read(cx).stop();
            }
        }

        // Mark agent as idle
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

        // Attach button -- disabled placeholder
        let attach_button = Button::new("attach-btn")
            .label("Attach")
            .outline()
            .small()
            .disabled(true);

        // Agent selector -- simple label for now
        let agent_label = div()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .py_1()
            .rounded_md()
            .text_xs()
            .text_color(theme.text_muted)
            .child("Main Agent");

        // Send / Stop button
        let action_button = if is_running {
            Button::new("stop-btn")
                .label("Stop")
                .outline()
                .small()
                .on_click(cx.listener(srow_debug::traced_listener!("input:stop_agent", |this, _, window, cx| {
                    this.stop_agent(window, cx);
                })))
        } else {
            Button::new("send-btn")
                .label("Send")
                .primary()
                .small()
                .disabled(!can_send)
                .on_click(cx.listener(srow_debug::traced_listener!("input:send_message", |this, _, window, cx| {
                    this.send_message(window, cx);
                })))
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.background)
            // Multi-line input area
            .child(
                div()
                    .flex_1()
                    .p_3()
                    .child(
                        Input::new(&self.input_state)
                            .disabled(is_running)
                    )
            )
            // Toolbar
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .px_3()
                    .py_2()
                    .gap_2()
                    .border_t_1()
                    .border_color(theme.border)
                    .child(attach_button)
                    .child(div().flex_1()) // spacer
                    .child(agent_label)
                    .child(action_button)
            )
            // Hint text
            .child(
                div()
                    .px_3()
                    .pb_1()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Enter send \u{00B7} Shift+Enter newline")
            )
    }
}
