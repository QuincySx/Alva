// INPUT:  gpui, gpui_component (Button), crate::models (WorkspaceModel, ChatModel, AgentModel, SettingsModel), crate::theme
// OUTPUT: pub struct Sidebar
// POS:    Left sidebar view containing a "New Chat" button and a placeholder session list.
use gpui::{prelude::*, Context, Entity, Render, Window, div};

use gpui_component::button::Button;

use crate::models::{AgentModel, ChatModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;

pub struct Sidebar {
    workspace_model: Entity<WorkspaceModel>,
    #[allow(dead_code)]
    chat_model: Entity<ChatModel>,
    #[allow(dead_code)]
    agent_model: Entity<AgentModel>,
    #[allow(dead_code)]
    settings_model: Entity<SettingsModel>,
}

impl Sidebar {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        chat_model: Entity<ChatModel>,
        agent_model: Entity<AgentModel>,
        settings_model: Entity<SettingsModel>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.subscribe(&workspace_model, |_this, _model, _event, cx| {
            cx.notify();
        })
        .detach();

        let view = Self {
            workspace_model,
            chat_model,
            agent_model,
            settings_model,
        };

        #[cfg(debug_assertions)]
        {
            if let Some(registry) = cx.try_global::<crate::DebugViewRegistry>() {
                registry.0.register(srow_debug::gpui::ViewEntry {
                    id: "sidebar".to_string(),
                    type_name: "Sidebar".to_string(),
                    parent_id: Some("root_view".to_string()),
                    snapshot_fn: Box::new(|| srow_debug::InspectNode {
                        id: "sidebar".to_string(),
                        type_name: "Sidebar".to_string(),
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

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let workspace_model = self.workspace_model.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.surface)
            // Header
            .child(
                div()
                    .px_3()
                    .pt_3()
                    .pb_1()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.text)
                    .child("Srow Agent"),
            )
            // "+ New Chat" button
            .child(
                div()
                    .mx_2()
                    .mt_1()
                    .mb_1()
                    .child(
                        Button::new("new-chat-btn")
                            .label("+ New Chat")
                            .outline()
                            .on_click(srow_debug::traced!("sidebar:new_chat", move |_, _, cx| {
                                tracing::info!("action_dispatch: new_chat");
                                let new_id = format!("sess-{}", chrono::Utc::now().timestamp_millis());
                                workspace_model.update(cx, |model, cx| {
                                    model.sidebar_items.insert(
                                        0,
                                        crate::models::SidebarItem::GlobalSession(
                                            crate::types::Session {
                                                id: new_id.clone(),
                                                workspace_id: None,
                                                name: "New Session".into(),
                                                created_at: chrono::Utc::now().timestamp_millis(),
                                                updated_at: chrono::Utc::now().timestamp_millis(),
                                            },
                                        ),
                                    );
                                    model.select_session(new_id, cx);
                                });
                            }))
                    )
            )
            // Placeholder for session list (will be built in Task 2)
            .child(
                div()
                    .flex_1()
                    .py_2()
                    .px_3()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Sessions will appear here"),
            )
    }
}
