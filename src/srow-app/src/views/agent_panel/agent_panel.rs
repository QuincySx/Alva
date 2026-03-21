// INPUT:  gpui, crate::models (AgentModel, SettingsModel, WorkspaceModel), crate::theme, crate::views::settings_panel::SettingsPanel
// OUTPUT: pub struct AgentPanel
// POS:    Right-side panel with tab bar (Status/Settings/Preview); shows agent status, embeds SettingsPanel, and a preview placeholder.
use gpui::{prelude::*, Context, Entity, FontWeight, Render, Window, div, px};

use crate::models::{AgentModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;
use crate::views::settings_panel::SettingsPanel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentPanelTab {
    AgentStatus,
    Settings,
    Preview,
}

pub struct AgentPanel {
    agent_model: Entity<AgentModel>,
    workspace_model: Entity<WorkspaceModel>,
    settings_panel: Entity<SettingsPanel>,
    active_tab: AgentPanelTab,
}

impl AgentPanel {
    pub fn new(
        agent_model: Entity<AgentModel>,
        workspace_model: Entity<WorkspaceModel>,
        settings_model: Entity<SettingsModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&agent_model, |_this, _model, _event, cx| {
            cx.notify();
        })
        .detach();

        cx.subscribe(&workspace_model, |_this, _model, _event, cx| {
            cx.notify();
        })
        .detach();

        // If no API key configured, start on Settings tab
        let has_api_key = settings_model.read(cx).has_api_key();
        let active_tab = if has_api_key {
            AgentPanelTab::AgentStatus
        } else {
            AgentPanelTab::Settings
        };

        let sm = settings_model.clone();
        let settings_panel = cx.new(|cx| SettingsPanel::new(sm, window, cx));

        Self {
            agent_model,
            workspace_model,
            settings_panel,
            active_tab,
        }
    }

    pub fn switch_to_settings(&mut self, cx: &mut Context<Self>) {
        self.active_tab = AgentPanelTab::Settings;
        cx.notify();
    }
}

impl Render for AgentPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let active_tab = self.active_tab;
        let text_color = theme.text;
        let text_muted = theme.text_muted;
        let accent = theme.accent;

        let on_click_status = cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            this.active_tab = AgentPanelTab::AgentStatus;
            cx.notify();
        });
        let on_click_settings = cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            this.active_tab = AgentPanelTab::Settings;
            cx.notify();
        });
        let on_click_preview = cx.listener(|this, _: &gpui::ClickEvent, _, cx| {
            this.active_tab = AgentPanelTab::Preview;
            cx.notify();
        });

        let is_status_active = active_tab == AgentPanelTab::AgentStatus;
        let is_settings_active = active_tab == AgentPanelTab::Settings;
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
                            .child("Status")
                            .on_click(on_click_status),
                    )
                    // Settings tab
                    .child(
                        div()
                            .id("tab-settings")
                            .px_3()
                            .py_2()
                            .cursor_pointer()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .when(is_settings_active, |el| {
                                el.text_color(text_color)
                                    .border_b_2()
                                    .border_color(accent)
                            })
                            .when(!is_settings_active, |el| el.text_color(text_muted))
                            .child("Settings")
                            .on_click(on_click_settings),
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
                AgentPanelTab::Settings => {
                    div()
                        .flex_1()
                        .size_full()
                        .child(self.settings_panel.clone())
                        .into_any_element()
                }
                AgentPanelTab::Preview => {
                    self.render_preview(window, cx).into_any_element()
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
        let theme = Theme::for_appearance(window, cx);
        let model = self.agent_model.read(cx);

        // Show the status for the currently selected session prominently
        let ws = self.workspace_model.read(cx);
        let selected_id = ws.selected_session_id.clone();

        let text_color = theme.text;
        let text_muted = theme.text_muted;

        let mut container = div()
            .flex()
            .flex_col()
            .flex_1()
            .p_2()
            .gap_2();

        // Current session status (highlighted)
        if let Some(ref sid) = selected_id {
            if let Some(status) = model.get_status(sid) {
                let indicator_color = status.kind.color();
                let label = status.kind.label();
                let detail = status
                    .detail
                    .clone()
                    .unwrap_or_else(|| "Current Session".to_string());

                container = container.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(theme.background)
                        .border_1()
                        .border_color(theme.border)
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(text_muted)
                                .child("Current Session"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .size(px(10.))
                                        .rounded_full()
                                        .bg(indicator_color),
                                )
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
                        ),
                );
            } else {
                container = container.child(
                    div()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .bg(theme.background)
                        .border_1()
                        .border_color(theme.border)
                        .child(
                            div()
                                .text_xs()
                                .text_color(text_muted)
                                .child("Current Session"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .size(px(10.))
                                        .rounded_full()
                                        .bg(gpui::rgba(0x6B7280FF)),
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(text_color)
                                        .child("Idle"),
                                ),
                        ),
                );
            }
        }

        // All agent statuses
        let mut statuses: Vec<_> = model.statuses.values().cloned().collect();
        statuses.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        if !statuses.is_empty() {
            container = container.child(
                div()
                    .mt_2()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(text_muted)
                    .child("All Agents"),
            );
        }

        for status in statuses {
            let indicator_color = status.kind.color();
            let label = status.kind.label();
            let detail = status
                .detail
                .clone()
                .unwrap_or_else(|| status.session_id.clone());

            container = container.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .child(
                        div()
                            .size(px(8.))
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
                                    .text_xs()
                                    .text_color(text_color)
                                    .child(detail),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child(label.to_string()),
                            ),
                    ),
            );
        }

        container
    }

    fn render_preview(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);

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
