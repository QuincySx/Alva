use gpui::{prelude::*, Context, Entity, FontWeight, Render, Window, div, px};

use crate::models::AgentModel;
use crate::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentPanelTab {
    AgentStatus,
    Preview,
}

pub struct AgentPanel {
    agent_model: Entity<AgentModel>,
    active_tab: AgentPanelTab,
}

impl AgentPanel {
    pub fn new(agent_model: Entity<AgentModel>, cx: &mut Context<Self>) -> Self {
        cx.subscribe(&agent_model, |_this, _model, _event, cx| {
            cx.notify();
        })
        .detach();

        Self {
            agent_model,
            active_tab: AgentPanelTab::AgentStatus,
        }
    }
}

impl Render for AgentPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window);
        let active_tab = self.active_tab;
        let text_color = theme.text;
        let text_muted = theme.text_muted;
        let accent = theme.accent;

        let on_click_status = cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            this.active_tab = AgentPanelTab::AgentStatus;
            cx.notify();
        });
        let on_click_preview = cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            this.active_tab = AgentPanelTab::Preview;
            cx.notify();
        });

        let is_status_active = active_tab == AgentPanelTab::AgentStatus;
        let is_preview_active = active_tab == AgentPanelTab::Preview;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.surface)
            // Tab bar
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .px_2()
                    .border_b_1()
                    .border_color(theme.border)
                    // Agent Status tab
                    .child(
                        div()
                            .id("tab-agent-status")
                            .px_3()
                            .py_2()
                            .cursor_pointer()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .when(is_status_active, |el| {
                                el.text_color(text_color)
                                    .border_b_2()
                                    .border_color(accent)
                            })
                            .when(!is_status_active, |el| el.text_color(text_muted))
                            .child("Agent Status")
                            .on_click(on_click_status),
                    )
                    // Preview tab
                    .child(
                        div()
                            .id("tab-preview")
                            .px_3()
                            .py_2()
                            .cursor_pointer()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .when(is_preview_active, |el| {
                                el.text_color(text_color)
                                    .border_b_2()
                                    .border_color(accent)
                            })
                            .when(!is_preview_active, |el| el.text_color(text_muted))
                            .child("Preview")
                            .on_click(on_click_preview),
                    ),
            )
            // Tab content
            .child(match active_tab {
                AgentPanelTab::AgentStatus => {
                    self.render_agent_status(window, cx).into_any_element()
                }
                AgentPanelTab::Preview => {
                    self.render_preview(window).into_any_element()
                }
            })
    }
}

impl AgentPanel {
    fn render_agent_status(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let theme = Theme::for_appearance(window);
        let model = self.agent_model.read(cx);

        let mut statuses: Vec<_> = model.statuses.values().cloned().collect();
        statuses.sort_by(|a, b| a.session_id.cmp(&b.session_id));

        let text_color = theme.text;
        let text_muted = theme.text_muted;

        div()
            .flex()
            .flex_col()
            .flex_1()
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
            }))
    }

    fn render_preview(&self, window: &mut Window) -> impl IntoElement {
        let theme = Theme::for_appearance(window);

        div()
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(theme.text_muted)
            .child("Preview Panel (coming soon)")
    }
}
