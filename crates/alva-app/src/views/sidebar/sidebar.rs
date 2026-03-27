// INPUT:  gpui, gpui_component (Button), crate::models (WorkspaceModel, ChatModel, AgentModel, SettingsModel), crate::theme, sub-views
// OUTPUT: pub struct Sidebar
// POS:    Left sidebar with new task button, navigation items, task history, and settings.
use gpui::{prelude::*, Context, Entity, FontWeight, Hsla, Render, Subscription, Window, div, px};

use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{Icon, IconName};
use gpui_component::Sizable;

use crate::models::{AgentModel, ChatModel, SettingsModel, WorkspaceModel};
use crate::theme::Theme;

use super::nav_items::render_nav_items;
use super::task_list::TaskList;

pub struct Sidebar {
    workspace_model: Entity<WorkspaceModel>,
    #[allow(dead_code)]
    chat_model: Entity<ChatModel>,
    #[allow(dead_code)]
    agent_model: Entity<AgentModel>,
    settings_model: Entity<SettingsModel>,
    task_list: Entity<TaskList>,
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
        let wm_for_list = workspace_model.clone();
        let task_list = cx.new(|cx| TaskList::new(wm_for_list, window, cx));

        let mut subscriptions = Vec::new();

        subscriptions.push(cx.subscribe(&workspace_model, |_this, _model, _event, cx| {
            cx.notify();
        }));

        Self {
            workspace_model,
            chat_model,
            agent_model,
            settings_model,
            task_list,
            _subscriptions: subscriptions,
        }
    }

    /// Render a horizontal divider line.
    fn render_divider(theme: &Theme) -> impl IntoElement {
        div()
            .mx(px(16.))
            .my(px(4.))
            .h(px(1.))
            .bg(theme.border_subtle)
    }
}

impl Render for Sidebar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let workspace_model = self.workspace_model.clone();

        // Render task list content
        let task_list_content = self.task_list.update(cx, |list, cx| {
            list.render_tasks("", &theme, cx)
        });

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.sidebar_bg)
            // "+ 新建任务" button (prominent, top)
            .child(
                div()
                    .px(px(12.))
                    .pt(px(16.))
                    .pb(px(8.))
                    .child(
                        Button::new("new-task-btn")
                            .label("+ 新建任务")
                            .primary()
                            .on_click(move |_, _, cx| {
                                let new_id = format!("task-{}", chrono::Utc::now().timestamp_millis());
                                workspace_model.update(cx, |model, cx| {
                                    model.sidebar_items.insert(
                                        0,
                                        crate::models::SidebarItem::GlobalSession(
                                            crate::types::Session {
                                                id: new_id.clone(),
                                                workspace_id: None,
                                                name: "新任务".into(),
                                                created_at: chrono::Utc::now().timestamp_millis(),
                                                updated_at: chrono::Utc::now().timestamp_millis(),
                                                status_text: None,
                                                duration_text: None,
                                            },
                                        ),
                                    );
                                    model.select_session(new_id, cx);
                                });
                            }),
                    ),
            )
            // Navigation items (Search, Schedule, Skills, MCP)
            .child(render_nav_items(&theme))
            // Divider
            .child(Self::render_divider(&theme))
            // Task history list (scrollable, flex-1)
            .child(
                div()
                    .id("task-list-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .child(task_list_content),
            )
            // Divider
            .child(Self::render_divider(&theme))
            // Settings button (fixed at bottom)
            .child(
                div()
                    .px(px(12.))
                    .py(px(8.))
                    .child(
                        div()
                            .id("settings-btn")
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .px(px(12.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .cursor_pointer()
                            .text_sm()
                            .text_color(theme.text_muted)
                            .hover(|style| style.bg(theme.surface_hover).text_color(theme.text))
                            .child(
                                Icon::new(IconName::Settings)
                                    .small()
                                    .text_color(Into::<Hsla>::into(theme.text_muted)),
                            )
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("设置"),
                            )
                            .on_click({
                                let settings_model = self.settings_model.clone();
                                move |_, window, cx| {
                                    crate::views::dialogs::open_settings_dialog(
                                        settings_model.clone(),
                                        window,
                                        cx,
                                    );
                                }
                            }),
                    ),
            )
    }
}
