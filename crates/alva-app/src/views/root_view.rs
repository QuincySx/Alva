// INPUT:  gpui, crate::models (AgentModel, ChatModel, SettingsModel, WorkspaceModel), crate::theme, views::sidebar, views::chat_panel, views::agent_detail_panel
// OUTPUT: pub struct RootView
// POS:    Top-level GPUI view composing Sidebar and ChatPanel in a two-column layout, with an optional Agent Detail panel.
use gpui::{prelude::*, Context, Entity, Render, Subscription, Window, div, px};

use crate::models::{AgentModel, ChatModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;
use super::sidebar::Sidebar;
use super::chat_panel::{ChatPanel, ChatPanelEvent};
use super::agent_detail_panel::{AgentDetailPanel, AgentDetailPanelEvent};

pub struct RootView {
    sidebar: Entity<Sidebar>,
    chat_panel: Entity<ChatPanel>,
    agent_model: Entity<AgentModel>,
    agent_detail: Option<Entity<AgentDetailPanel>>,
    _subscriptions: Vec<Subscription>,
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
        let am2 = agent_model.clone();
        let am_root = agent_model;
        let sm1 = settings_model.clone();
        let sm2 = settings_model;

        let sidebar = cx.new(|cx| Sidebar::new(wm1, cm1, am1, sm1, window, cx));
        let chat_panel = cx.new(|cx| ChatPanel::new(wm2, cm2, am2, sm2, window, cx));

        let mut subscriptions = Vec::new();

        // Subscribe to ChatPanel events for agent click propagation
        subscriptions.push(cx.subscribe(
            &chat_panel,
            |this: &mut Self, _panel, event: &ChatPanelEvent, cx| {
                match event {
                    ChatPanelEvent::AgentClicked { session_id } => {
                        this.open_agent_detail(session_id.clone(), cx);
                    }
                }
            },
        ));

        let view = Self {
            sidebar,
            chat_panel,
            agent_model: am_root,
            agent_detail: None,
            _subscriptions: subscriptions,
        };

        #[cfg(debug_assertions)]
        {
            if let Some(registry) = cx.try_global::<crate::DebugViewRegistry>() {
                registry.0.register(alva_app_debug::gpui::ViewEntry {
                    id: "root_view".to_string(),
                    type_name: "RootView".to_string(),
                    parent_id: None,
                    snapshot_fn: Box::new(|| alva_app_debug::InspectNode {
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

    fn open_agent_detail(&mut self, session_id: String, cx: &mut Context<Self>) {
        let am = self.agent_model.clone();
        let detail = cx.new(|cx| AgentDetailPanel::new(session_id, am, cx));

        // Subscribe to AgentDetailPanel close event
        let sub = cx.subscribe(&detail, |this: &mut Self, _panel, event: &AgentDetailPanelEvent, cx| {
            match event {
                AgentDetailPanelEvent::Close => {
                    this.agent_detail = None;
                    cx.notify();
                }
            }
        });
        self._subscriptions.push(sub);

        self.agent_detail = Some(detail);
        cx.notify();
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

        // Agent Detail: 320px (conditional)
        if let Some(detail) = &self.agent_detail {
            root = root.child(
                div()
                    .w(px(320.))
                    .h_full()
                    .flex_none()
                    .border_l_1()
                    .border_color(theme.border)
                    .child(detail.clone()),
            );
        }

        root
    }
}
