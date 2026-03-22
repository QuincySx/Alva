// INPUT:  gpui, crate::models (AgentModel, ChatModel, SettingsModel, WorkspaceModel), crate::theme, views::sidebar, views::chat_panel
// OUTPUT: pub struct RootView
// POS:    Top-level GPUI view composing Sidebar and ChatPanel in a two-column layout, with an optional Agent Detail panel.
use gpui::{prelude::*, Context, Entity, Render, Window, div, px};

use crate::models::{AgentModel, ChatModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;
use super::sidebar::Sidebar;
use super::chat_panel::ChatPanel;

pub struct RootView {
    sidebar: Entity<Sidebar>,
    chat_panel: Entity<ChatPanel>,
    #[allow(dead_code)]
    show_agent_detail: bool,
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
        let wm2 = workspace_model;
        let cm1 = chat_model.clone();
        let cm2 = chat_model;
        let am1 = agent_model.clone();
        let am2 = agent_model;
        let sm1 = settings_model.clone();
        let sm2 = settings_model;

        let sidebar = cx.new(|cx| Sidebar::new(wm1, cm1, am1, sm1, window, cx));
        let chat_panel = cx.new(|cx| ChatPanel::new(wm2, cm2, am2, sm2, window, cx));

        let view = Self {
            sidebar,
            chat_panel,
            show_agent_detail: false,
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

        let mut root = div()
            .flex()
            .flex_row()
            .size_full()
            .bg(theme.background);

        // Sidebar: 240px
        root = root.child(
            div()
                .w(px(240.))
                .h_full()
                .flex_none()
                .border_r_1()
                .border_color(theme.border)
                .child(self.sidebar.clone()),
        );

        // Chat: flex-1
        root = root.child(
            div()
                .flex_1()
                .h_full()
                .child(self.chat_panel.clone()),
        );

        // Agent Detail: 320px (conditional — for Task 8)
        if self.show_agent_detail {
            root = root.child(
                div()
                    .w(px(320.))
                    .h_full()
                    .flex_none()
                    .border_l_1()
                    .border_color(theme.border)
                    .child(div().p_4().child("Agent Detail Panel (coming in Task 8)")),
            );
        }

        root
    }
}
