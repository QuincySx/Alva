// INPUT:  gpui, gpui_component (Button, Input, InputState, InputEvent),
//         crate::models (WorkspaceModel, ChatModel, AgentModel, SettingsModel), crate::theme,
//         super::session_list::SessionList, super::management_buttons
// OUTPUT: pub struct Sidebar
// POS:    Left sidebar view with search, management buttons, new-chat, time-grouped session list, and settings.
use gpui::{prelude::*, Context, Entity, FontWeight, Render, Subscription, Window, div, px};

use gpui_component::button::Button;
use gpui_component::input::{InputEvent, InputState, Input};

use crate::models::{AgentModel, ChatModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;

use super::management_buttons::render_management_buttons;
use super::session_list::SessionList;

pub struct Sidebar {
    workspace_model: Entity<WorkspaceModel>,
    #[allow(dead_code)]
    chat_model: Entity<ChatModel>,
    #[allow(dead_code)]
    agent_model: Entity<AgentModel>,
    #[allow(dead_code)]
    settings_model: Entity<SettingsModel>,
    search_input: Entity<InputState>,
    search_query: String,
    session_list: Entity<SessionList>,
    _subscriptions: Vec<Subscription>,
}

impl Sidebar {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        chat_model: Entity<ChatModel>,
        agent_model: Entity<AgentModel>,
        settings_model: Entity<SettingsModel>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search_input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search sessions...")
        });

        let wm_for_list = workspace_model.clone();
        let session_list = cx.new(|cx| SessionList::new(wm_for_list, window, cx));

        let mut subscriptions = Vec::new();

        // Subscribe to workspace model for session changes
        subscriptions.push(cx.subscribe(&workspace_model, |_this, _model, _event, cx| {
            cx.notify();
        }));

        // Subscribe to search input for filtering
        subscriptions.push(cx.subscribe_in(
            &search_input,
            window,
            |this, state, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.search_query = state.read(cx).value().to_string();
                    cx.notify();
                }
            },
        ));

        let view = Self {
            workspace_model,
            chat_model,
            agent_model,
            settings_model,
            search_input,
            search_query: String::new(),
            session_list,
            _subscriptions: subscriptions,
        };

        #[cfg(debug_assertions)]
        {
            if let Some(registry) = cx.try_global::<crate::DebugViewRegistry>() {
                registry.0.register(alva_app_debug::gpui::ViewEntry {
                    id: "sidebar".to_string(),
                    type_name: "Sidebar".to_string(),
                    parent_id: Some("root_view".to_string()),
                    snapshot_fn: Box::new(|| alva_app_debug::InspectNode {
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

    /// Render a horizontal divider line.
    fn render_divider(theme: &Theme) -> impl IntoElement {
        div()
            .mx_2()
            .h(px(1.))
            .bg(theme.border)
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let workspace_model = self.workspace_model.clone();
        let search_query = self.search_query.clone();

        // Render the session list content via the SessionList entity
        let session_list_content = self.session_list.update(cx, |list, cx| {
            list.render_grouped(&search_query, &theme, cx)
        });

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.surface)
            // Search input (top)
            .child(
                div()
                    .px_2()
                    .pt_2()
                    .pb_1()
                    .child(Input::new(&self.search_input)),
            )
            // Management buttons (Agents, Skills)
            .child(render_management_buttons(&theme))
            // Divider
            .child(Self::render_divider(&theme))
            // "+ New Chat" button
            .child(
                div()
                    .mx_2()
                    .my_1()
                    .child(
                        Button::new("new-chat-btn")
                            .label("+ New Chat")
                            .outline()
                            .on_click(alva_app_debug::traced!("sidebar:new_chat", move |_, _, cx| {
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
                            })),
                    ),
            )
            // Session list (scrollable, flex-1)
            .child(
                div()
                    .id("session-list-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(session_list_content),
            )
            // Divider
            .child(Self::render_divider(&theme))
            // Settings button (fixed at bottom)
            .child(
                div()
                    .px_2()
                    .py_1()
                    .child(
                        div()
                            .id("settings-btn")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .px_2()
                            .py(px(4.))
                            .rounded_md()
                            .cursor_pointer()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .hover(|style| style.bg(theme.surface_hover))
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Settings"),
                            )
                            .on_click({
                                let settings_model = self.settings_model.clone();
                                alva_app_debug::traced!("sidebar:settings", move |_, window, cx| {
                                    tracing::info!("open settings dialog");
                                    crate::views::dialogs::open_settings_dialog(
                                        settings_model.clone(),
                                        window,
                                        cx,
                                    );
                                })
                            }),
                    ),
            )
    }
}
