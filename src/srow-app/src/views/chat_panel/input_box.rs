use gpui::{prelude::*, Context, Entity, FocusHandle, Focusable, FontWeight, Render, Window, div, px};

use crate::engine_bridge::EngineBridge;
use crate::models::{AgentModel, ChatModel, WorkspaceModel};
use crate::theme::Theme;

pub struct InputBox {
    focus_handle: FocusHandle,
    draft: String,
    workspace_model: Entity<WorkspaceModel>,
    chat_model: Entity<ChatModel>,
    agent_model: Entity<AgentModel>,
}

impl InputBox {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        chat_model: Entity<ChatModel>,
        agent_model: Entity<AgentModel>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            draft: String::new(),
            workspace_model,
            chat_model,
            agent_model,
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

        // Push user message into chat model
        self.chat_model.update(cx, |model, cx| {
            model.push_user_message(&session_id, text.clone(), cx);
        });

        // Start mock engine
        EngineBridge::send_message(
            session_id,
            text,
            self.chat_model.clone(),
            self.agent_model.clone(),
            cx,
        );

        // Clear draft
        self.draft.clear();
        cx.notify();
    }
}

impl Focusable for InputBox {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for InputBox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window);
        let draft_display = self.draft.clone();
        let has_text = !self.draft.trim().is_empty();
        let has_session = self.workspace_model.read(cx).selected_session_id.is_some();

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
                // Input area (simplified -- displays draft text, click to populate)
                div()
                    .id("input-area")
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
                    .when(draft_display.is_empty(), |el| {
                        el.child(
                            div()
                                .text_color(text_muted)
                                .child("Type a message... (click Send to send mock message)"),
                        )
                    })
                    .when(!draft_display.is_empty(), |el| {
                        el.child(draft_display)
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        // Simulate typing a message when input area is clicked
                        if this.draft.is_empty() {
                            this.draft = "Hello! Can you help me with this task?".into();
                            cx.notify();
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
                    .when(has_text && has_session, |el| {
                        el.bg(accent)
                            .text_color(gpui::white())
                            .hover(move |style| style.bg(accent_hover))
                    })
                    .when(!(has_text && has_session), |el| {
                        el.bg(theme.surface_hover)
                            .text_color(text_muted)
                            .cursor_not_allowed()
                            .opacity(0.6)
                    })
                    .child("Send")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.send_message(cx);
                    })),
            )
    }
}
