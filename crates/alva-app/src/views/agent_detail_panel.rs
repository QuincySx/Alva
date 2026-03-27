// INPUT:  gpui (prelude, Context, Entity, EventEmitter, FontWeight, Hsla, Render, etc.), crate::models::AgentModel, crate::types, crate::theme
// OUTPUT: pub struct AgentDetailPanel, pub enum AgentDetailPanelEvent
// POS:    GPUI view showing agent details in a sliding right-side panel.
use gpui::{prelude::*, Context, Entity, EventEmitter, FontWeight, Hsla, Render, Subscription, Window, div, px};

use crate::models::AgentModel;
use crate::theme::Theme;
use crate::types::AgentStatusKind;

pub struct AgentDetailPanel {
    session_id: String,
    agent_model: Entity<AgentModel>,
    _subscription: Subscription,
}

pub enum AgentDetailPanelEvent {
    Close,
}

impl EventEmitter<AgentDetailPanelEvent> for AgentDetailPanel {}

impl AgentDetailPanel {
    pub fn new(
        session_id: String,
        agent_model: Entity<AgentModel>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.subscribe(&agent_model, |_this, _model, _event, cx| {
            cx.notify();
        });

        Self {
            session_id,
            agent_model,
            _subscription: subscription,
        }
    }

    /// Render a colored status dot.
    fn render_status_dot(kind: &AgentStatusKind) -> impl IntoElement {
        let color: Hsla = kind.color().into();
        div()
            .size(px(8.))
            .rounded_full()
            .bg(color)
    }
}

impl Render for AgentDetailPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let model = self.agent_model.read(cx);
        let status = model.get_status(&self.session_id);

        let (kind, detail_text) = match status {
            Some(s) => (s.kind.clone(), s.detail.clone()),
            None => (AgentStatusKind::Offline, None),
        };

        div()
            .id("agent-detail-panel")
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.surface)
            .overflow_y_scroll()
            // Header: agent name + close button
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text)
                            .child(self.session_id.clone()),
                    )
                    .child(
                        div()
                            .id("agent-detail-close-btn")
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_sm()
                            .text_color(theme.text_muted)
                            .hover(|s| s.bg(theme.surface_hover).text_color(theme.text))
                            .on_click(cx.listener(alva_app_debug::traced_listener!(
                                "agent_detail:close",
                                |_this, _: &gpui::ClickEvent, _, cx| {
                                    cx.emit(AgentDetailPanelEvent::Close);
                                }
                            )))
                            .child("\u{2715}"), // ✕
                    ),
            )
            // Status section
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_muted)
                            .child("STATUS"),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(Self::render_status_dot(&kind))
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.text)
                                    .child(kind.label().to_string()),
                            ),
                    )
                    .when(detail_text.is_some(), |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(theme.text_muted)
                                .child(detail_text.unwrap_or_default()),
                        )
                    }),
            )
            // Activity Log placeholder
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_muted)
                            .child("ACTIVITY LOG"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child("Activity log coming in Phase 2"),
                    ),
            )
            // Config placeholder
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.text_muted)
                            .child("CONFIGURATION"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child("Config details coming in Phase 2"),
                    ),
            )
    }
}
