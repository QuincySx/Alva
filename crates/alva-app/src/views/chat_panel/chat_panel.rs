// INPUT:  gpui, crate::models (AgentModel, ChatModel, SettingsModel, WorkspaceModel), crate::theme, sub-views
// OUTPUT: pub struct ChatPanel, pub enum ChatPanelEvent
// POS:    Composite GPUI view: shows SessionWelcomeView when session has no messages, or chat when it does.
use gpui::{prelude::*, Context, Entity, EventEmitter, FontWeight, Render, Subscription, Window, div, px};

use crate::models::{AgentModel, ChatModel, ChatModelEvent, SettingsModel, SidebarItem, WorkspaceModel};
use crate::theme::Theme;
use crate::views::session_welcome::SessionWelcomeView;
use super::message_list::MessageList;
use super::input_box::InputBox;
use super::running_agents_zone::{RunningAgentsZone, RunningAgentsZoneEvent};

pub struct ChatPanel {
    workspace_model: Entity<WorkspaceModel>,
    chat_model: Entity<ChatModel>,
    session_welcome: Entity<SessionWelcomeView>,
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
        let wm2 = workspace_model.clone();
        let cm2 = chat_model.clone();
        let am_zone = agent_model.clone();

        // Session welcome (shown when session has no messages)
        let wm_welcome = workspace_model.clone();
        let cm_welcome = chat_model.clone();
        let am_welcome = agent_model.clone();
        let sm_welcome = settings_model.clone();
        let session_welcome = cx.new(|cx| {
            SessionWelcomeView::new(wm_welcome, cm_welcome, am_welcome, sm_welcome, window, cx)
        });

        let message_list = cx.new(|cx| MessageList::new(wm1, cm1, cx));
        let running_agents_zone = cx.new(|cx| RunningAgentsZone::new(am_zone, cx));
        let input_box = cx.new(|cx| InputBox::new(wm2, cm2, agent_model, settings_model, window, cx));

        let mut subscriptions = Vec::new();

        // Re-emit agent click events from RunningAgentsZone
        subscriptions.push(cx.subscribe(
            &running_agents_zone,
            |_this, _zone, event: &RunningAgentsZoneEvent, cx| {
                let RunningAgentsZoneEvent::AgentClicked { session_id } = event;
                cx.emit(ChatPanelEvent::AgentClicked {
                    session_id: session_id.clone(),
                });
            },
        ));

        // Re-render when session selection changes
        subscriptions.push(cx.subscribe(&wm0, |_this, _model, _event, cx| {
            cx.notify();
        }));

        // Re-render when chat messages arrive (to transition welcome → chat)
        let cm_sub = chat_model.clone();
        subscriptions.push(cx.subscribe(&cm_sub, |_this, _model, _event: &ChatModelEvent, cx| {
            cx.notify();
        }));

        Self {
            workspace_model: wm0,
            chat_model,
            session_welcome,
            message_list,
            running_agents_zone,
            input_box,
            _subscriptions: subscriptions,
        }
    }

    /// Check if the currently selected session has any messages.
    fn session_has_messages(&self, cx: &Context<Self>) -> bool {
        let ws = self.workspace_model.read(cx);
        let sid = match ws.selected_session_id.as_ref() {
            Some(s) => s,
            None => return false,
        };
        let cm = self.chat_model.read(cx);
        match cm.get_chat(sid) {
            Some(chat) => !chat.read(cx).messages().is_empty(),
            None => false,
        }
    }

    /// Look up the session name for the currently selected session.
    fn session_title(&self, cx: &Context<Self>) -> String {
        let ws = self.workspace_model.read(cx);
        let sid = match ws.selected_session_id.as_ref() {
            Some(s) => s,
            None => return String::new(),
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
        "任务".to_string()
    }
}

impl Render for ChatPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let has_session = self.workspace_model.read(cx).selected_session_id.is_some();
        let has_messages = self.session_has_messages(cx);

        // Show session welcome when: session selected but no messages yet
        if !has_session || !has_messages {
            return div()
                .size_full()
                .bg(theme.background)
                .child(self.session_welcome.clone())
                .into_any_element();
        }

        // Show chat view
        let title = self.session_title(cx);

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.background)
            // Header
            .child(
                div()
                    .flex()
                    .items_center()
                    .px(px(20.))
                    .py(px(12.))
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
            // Message list (flex-1, scrollable)
            .child(self.message_list.clone())
            // Running agents zone
            .child(self.running_agents_zone.clone())
            // Input box
            .child(self.input_box.clone())
            .into_any_element()
    }
}
