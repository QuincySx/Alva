// INPUT:  gpui, crate::models (ChatModel, WorkspaceModel), crate::theme
// OUTPUT: pub struct MessageList
// POS:    Scrollable GPUI view rendering chat messages.
//         Message rendering commented out during migration — depends on deleted UIMessage/UIMessagePart/ChatStatus types.
//         TODO: Rebuild on agent-core message types.

use gpui::{prelude::*, Context, Entity, Render, Window, div};

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
}

impl Render for MessageList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);

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
                                    .child("TODO: rebuild on agent-core"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(theme.text_muted)
                                    .child("Message rendering will be rebuilt with agent-core types"),
                            ),
                    ),
            )
    }
}
