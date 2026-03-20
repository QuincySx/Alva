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
        let chat_panel = cx.new(|cx| ChatPanel::new(wm2, cm1, am1, sm1, cx));
        let agent_panel = cx.new(|cx| AgentPanel::new(am2, wm3, sm2, cx));

        Self {
            side_panel,
            chat_panel,
            agent_panel,
        }
    }
}

impl Render for RootView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window);

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
