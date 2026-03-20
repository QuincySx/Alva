use gpui::{prelude::*, Context, Entity, FontWeight, Render, Window, div};

use crate::models::{AgentModel, ChatModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;
use super::message_list::MessageList;
use super::input_box::InputBox;

pub struct ChatPanel {
    message_list: Entity<MessageList>,
    input_box: Entity<InputBox>,
}

impl ChatPanel {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        chat_model: Entity<ChatModel>,
        agent_model: Entity<AgentModel>,
        settings_model: Entity<SettingsModel>,
        cx: &mut Context<Self>,
    ) -> Self {
        let wm1 = workspace_model.clone();
        let cm1 = chat_model.clone();
        let wm2 = workspace_model;
        let cm2 = chat_model;

        let message_list = cx.new(|cx| MessageList::new(wm1, cm1, cx));
        let input_box = cx.new(|cx| InputBox::new(wm2, cm2, agent_model, settings_model, cx));

        Self {
            message_list,
            input_box,
        }
    }
}

impl Render for ChatPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            .child(
                // Header
                div()
                    .flex()
                    .items_center()
                    .px_4()
                    .py_2()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child("Chat"),
                    ),
            )
            .child(self.message_list.clone())
            .child(self.input_box.clone())
    }
}
