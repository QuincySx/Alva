// INPUT:  gpui, crate::models (AgentModel, ChatModel, SettingsModel, WorkspaceModel), crate::theme, message_list, input_box, running_agents_zone
// OUTPUT: pub struct ChatPanel, pub enum ChatPanelEvent
// POS:    Composite GPUI view combining a header, MessageList, RunningAgentsZone, and InputBox into the central chat column.
use gpui::{prelude::*, Context, Entity, EventEmitter, FontWeight, Render, Subscription, Window, div};

use crate::models::{AgentModel, ChatModel, SettingsModel, SidebarItem, WorkspaceModel};
use crate::theme::Theme;
use super::message_list::MessageList;
use super::input_box::InputBox;
use super::running_agents_zone::{RunningAgentsZone, RunningAgentsZoneEvent};

pub struct ChatPanel {
    workspace_model: Entity<WorkspaceModel>,
    message_list: Entity<MessageList>,
    running_agents_zone: Entity<RunningAgentsZone>,
    input_box: Entity<InputBox>,
    _subscriptions: Vec<Subscription>,
}

pub enum ChatPanelEvent {
    AgentClicked { session_id: String },
}

impl EventEmitter<ChatPanelEvent> for ChatPanel {}

impl ChatPanel {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        chat_model: Entity<ChatModel>,
        agent_model: Entity<AgentModel>,
        settings_model: Entity<SettingsModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let wm0 = workspace_model.clone();
        let wm1 = workspace_model.clone();
        let cm1 = chat_model.clone();
        let wm2 = workspace_model;
        let cm2 = chat_model;
        let am_zone = agent_model.clone();

        let message_list = cx.new(|cx| MessageList::new(wm1, cm1, cx));
        let running_agents_zone = cx.new(|cx| RunningAgentsZone::new(am_zone, cx));
        let input_box = cx.new(|cx| InputBox::new(wm2, cm2, agent_model, settings_model, window, cx));

        let mut subscriptions = Vec::new();

        // Re-emit agent click events from RunningAgentsZone as ChatPanelEvent
        subscriptions.push(cx.subscribe(
            &running_agents_zone,
            |_this, _zone, event: &RunningAgentsZoneEvent, cx| {
                match event {
                    RunningAgentsZoneEvent::AgentClicked { session_id } => {
                        cx.emit(ChatPanelEvent::AgentClicked {
                            session_id: session_id.clone(),
                        });
                    }
                }
            },
        ));

        let view = Self {
            workspace_model: wm0,
            message_list,
            running_agents_zone,
            input_box,
            _subscriptions: subscriptions,
        };

        #[cfg(debug_assertions)]
        {
            if let Some(registry) = cx.try_global::<crate::DebugViewRegistry>() {
                registry.0.register(alva_app_debug::gpui::ViewEntry {
                    id: "chat_panel".to_string(),
                    type_name: "ChatPanel".to_string(),
                    parent_id: Some("root_view".to_string()),
                    snapshot_fn: Box::new(|| alva_app_debug::InspectNode {
                        id: "chat_panel".to_string(),
                        type_name: "ChatPanel".to_string(),
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

impl ChatPanel {
    /// Look up the session name for the currently selected session.
    fn session_title(&self, cx: &Context<Self>) -> String {
        let ws = self.workspace_model.read(cx);
        let sid = match ws.selected_session_id.as_ref() {
            Some(s) => s,
            None => return "Chat".to_string(),
        };
        for item in &ws.sidebar_items {
            match item {
                SidebarItem::GlobalSession(s) if s.id == *sid => return s.name.clone(),
                SidebarItem::Workspace(w) => {
                    for s in &w.sessions {
                        if s.id == *sid {
                            return s.name.clone();
                        }
                    }
                }
                _ => {}
            }
        }
        "Chat".to_string()
    }
}

impl Render for ChatPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let title = self.session_title(cx);

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
                            .child(title),
                    ),
            )
            .child(self.message_list.clone())
            .child(self.running_agents_zone.clone())
            .child(self.input_box.clone())
    }
}
