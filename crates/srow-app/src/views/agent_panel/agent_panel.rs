use gpui::{prelude::*, Context, Entity, FontWeight, Render, Window, div, px};

use crate::models::AgentModel;
use crate::theme::Theme;

pub struct AgentPanel {
    agent_model: Entity<AgentModel>,
}

impl AgentPanel {
    pub fn new(agent_model: Entity<AgentModel>, cx: &mut Context<Self>) -> Self {
        cx.subscribe(&agent_model, |_this, _model, _event, cx| {
            cx.notify();
        })
        .detach();

        Self { agent_model }
    }
}

impl Render for AgentPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window);
        let model = self.agent_model.read(cx);

        let mut statuses: Vec<_> = model.statuses.values().cloned().collect();
        statuses.sort_by(|a, b| a.session_id.cmp(&b.session_id));

        let text_color = theme.text;
        let text_muted = theme.text_muted;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.surface)
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
                            .text_color(text_color)
                            .child("Agent Status"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .p_2()
                    .gap_1()
                    .children(statuses.into_iter().map(move |status| {
                        let indicator_color = status.kind.color();
                        let label = status.kind.label();
                        let detail = status
                            .detail
                            .clone()
                            .unwrap_or_else(|| status.session_id.clone());

                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .child(
                                // Status indicator dot
                                div()
                                    .size(px(10.))
                                    .rounded_full()
                                    .bg(indicator_color),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(text_color)
                                            .child(detail),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(text_muted)
                                            .child(label.to_string()),
                                    ),
                            )
                    })),
            )
    }
}
