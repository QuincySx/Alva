// INPUT:  gpui, crate::models (AgentModel, ChatModel, SettingsModel, WorkspaceModel), crate::theme, views::side_panel, views::chat_panel, views::agent_panel
// OUTPUT: pub struct RootView
// POS:    Top-level GPUI view composing SidePanel, ChatPanel, and AgentPanel in a three-column layout.
use gpui::{prelude::*, Context, Entity, Render, Window, div, px};

use crate::models::{AgentModel, ChatModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;
use super::side_panel::SidePanel;
use super::chat_panel::ChatPanel;
use super::agent_panel::AgentPanel;

pub struct RootView {
    side_panel: Entity<SidePanel>,
    chat_panel: Entity<ChatPanel>,
    agent_panel: Entity<AgentPanel>,
}

impl RootView {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        chat_model: Entity<ChatModel>,
        agent_model: Entity<AgentModel>,
        settings_model: Entity<SettingsModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let wm1 = workspace_model.clone();
        let wm2 = workspace_model.clone();
        let wm3 = workspace_model;
        let cm1 = chat_model.clone();
        let am1 = agent_model.clone();
        let am2 = agent_model;
        let sm1 = settings_model.clone();
        let sm2 = settings_model;

        let side_panel = cx.new(|cx| SidePanel::new(wm1, cx));
        let chat_panel = cx.new(|cx| ChatPanel::new(wm2, cm1, am1, sm1, window, cx));
        let agent_panel = cx.new(|cx| AgentPanel::new(am2, wm3, sm2, window, cx));

        let view = Self {
            side_panel,
            chat_panel,
            agent_panel,
        };

        #[cfg(debug_assertions)]
        {
            if let Some(registry) = cx.try_global::<crate::DebugViewRegistry>() {
                registry.0.register(srow_debug::gpui::ViewEntry {
                    id: "root_view".to_string(),
                    type_name: "RootView".to_string(),
                    parent_id: None,
                    snapshot_fn: Box::new(|| srow_debug::InspectNode {
                        id: "root_view".to_string(),
                        type_name: "RootView".to_string(),
                        bounds: None,
                        properties: std::collections::HashMap::new(),
                        children: vec![],
                    }),
                });
            }
        }

        view
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);

        div()
            .flex()
            .flex_row()
            .size_full()
            .bg(theme.background)
            .child(
                // Left side panel -- fixed width 220px
                div()
                    .w(px(220.))
                    .h_full()
                    .flex_none()
                    .border_r_1()
                    .border_color(theme.border)
                    .child(self.side_panel.clone()),
            )
            .child(
                // Center chat panel -- flex grow
                div()
                    .flex_1()
                    .h_full()
                    .child(self.chat_panel.clone()),
            )
            .child(
                // Right agent panel -- fixed width 280px
                div()
                    .w(px(280.))
                    .h_full()
                    .flex_none()
                    .border_l_1()
                    .border_color(theme.border)
                    .child(self.agent_panel.clone()),
            )
    }
}
