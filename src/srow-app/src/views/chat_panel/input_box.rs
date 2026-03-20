use gpui::{prelude::*, Context, Entity, FocusHandle, Focusable, FontWeight, Modifiers, Render, Window, div, px};

use crate::engine_bridge::EngineBridge;
use crate::models::{AgentModel, ChatModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;
use crate::types::AgentStatusKind;

pub struct InputBox {
    focus_handle: FocusHandle,
    draft: String,
    workspace_model: Entity<WorkspaceModel>,
    chat_model: Entity<ChatModel>,
    agent_model: Entity<AgentModel>,
    settings_model: Entity<SettingsModel>,
}

impl InputBox {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        chat_model: Entity<ChatModel>,
        agent_model: Entity<AgentModel>,
        settings_model: Entity<SettingsModel>,
        cx: &mut Context<Self>,
    ) -> Self {
        // Subscribe to agent model to update send button state
        cx.subscribe(&agent_model, |_this, _model, _event, cx| {
            cx.notify();
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            draft: String::new(),
            workspace_model,
            chat_model,
            agent_model,
            settings_model,
        }
    }

    fn send_message(&mut self, cx: &mut Context<Self>) {
        let text = self.draft.trim().to_string();
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

        // Push user message into chat model
        self.chat_model.update(cx, |model, cx| {
            model.push_user_message(&session_id, text.clone(), cx);
        });

        // Start real engine
        EngineBridge::send_message(
            session_id,
            text,
            self.chat_model.clone(),
            self.agent_model.clone(),
            self.settings_model.clone(),
            cx,
        );

        // Clear draft
        self.draft.clear();
        cx.notify();
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

impl Focusable for InputBox {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for InputBox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let draft_display = self.draft.clone();
        let has_text = !self.draft.trim().is_empty();
        let has_session = self.workspace_model.read(cx).selected_session_id.is_some();
        let is_running = self.is_agent_running(cx);

        let can_send = has_text && has_session && !is_running;

        let accent = theme.accent;
        let accent_hover = theme.accent_hover;
        let text_muted = theme.text_muted;

        div()
            .flex()
            .flex_row()
            .w_full()
            .p_3()
            .gap_2()
            .border_t_1()
            .border_color(theme.border)
            .bg(theme.background)
            .child(
                // Input area with real keyboard input
                div()
                    .id("input-area")
                    .track_focus(&self.focus_handle)
                    .flex_1()
                    .px_3()
                    .py_2()
                    .rounded_lg()
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface)
                    .text_sm()
                    .text_color(theme.text)
                    .min_h(px(36.))
                    .cursor_text()
                    .when(draft_display.is_empty(), |el| {
                        el.child(
                            div()
                                .text_color(text_muted)
                                .child(if is_running {
                                    "Agent is running..."
                                } else {
                                    "Type a message... (Enter to send)"
                                }),
                        )
                    })
                    .when(!draft_display.is_empty(), |el| {
                        // Show text with cursor indicator
                        el.child(format!("{}|", draft_display))
                    })
                    .on_click(cx.listener(|this, _, window, _cx| {
                        this.focus_handle.focus(window);
                    }))
                    .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _window, cx| {
                        let key = &event.keystroke.key;
                        if key == "backspace" {
                            this.draft.pop();
                            cx.notify();
                        } else if key == "enter" {
                            if event.keystroke.modifiers.shift {
                                // Shift+Enter = newline
                                this.draft.push('\n');
                                cx.notify();
                            } else {
                                // Enter = send
                                this.send_message(cx);
                            }
                        } else if key == "space" && event.keystroke.modifiers == Modifiers::none() {
                            this.draft.push(' ');
                            cx.notify();
                        } else {
                            // Handle regular text input
                            if let Some(ref key_char) = event.keystroke.key_char {
                                this.draft.push_str(key_char);
                                cx.notify();
                            } else if key.len() == 1 && event.keystroke.modifiers == Modifiers::none() {
                                this.draft.push_str(key);
                                cx.notify();
                            }
                        }
                    })),
            )
            .child(
                // Send button
                div()
                    .id("send-btn")
                    .px_4()
                    .py_2()
                    .rounded_lg()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .cursor_pointer()
                    .when(can_send, |el| {
                        el.bg(accent)
                            .text_color(gpui::white())
                            .hover(move |style| style.bg(accent_hover))
                    })
                    .when(!can_send, |el| {
                        el.bg(theme.surface_hover)
                            .text_color(text_muted)
                            .cursor_not_allowed()
                            .opacity(0.6)
                    })
                    .child(if is_running { "..." } else { "Send" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send_message(cx);
                    })),
            )
    }
}
