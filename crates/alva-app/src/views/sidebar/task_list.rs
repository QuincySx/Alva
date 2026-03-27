// INPUT:  gpui, crate::models (WorkspaceModel, SidebarItem), crate::types::Session, crate::theme::Theme
// OUTPUT: pub struct TaskList
// POS:    Renders task history items in the sidebar with name, duration, and status.
use gpui::{prelude::*, *};

use crate::models::{SidebarItem, WorkspaceModel};
use crate::theme::Theme;
use crate::types::Session;

pub struct TaskList {
    workspace_model: Entity<WorkspaceModel>,
    _subscriptions: Vec<Subscription>,
}

impl TaskList {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut subscriptions = Vec::new();

        subscriptions.push(cx.subscribe(&workspace_model, |_this, _model, _event, cx| {
            cx.notify();
        }));

        Self {
            workspace_model,
            _subscriptions: subscriptions,
        }
    }

    /// Flatten all sessions from sidebar_items, filter by query, sort by updated_at.
    fn collect_sessions(&self, search_query: &str, cx: &Context<Self>) -> Vec<Session> {
        let ws = self.workspace_model.read(cx);
        let query = search_query.to_lowercase();

        let mut sessions: Vec<Session> = Vec::new();

        for item in &ws.sidebar_items {
            match item {
                SidebarItem::GlobalSession(s) => {
                    if query.is_empty() || s.name.to_lowercase().contains(&query) {
                        sessions.push(s.clone());
                    }
                }
                SidebarItem::Workspace(w) => {
                    for s in &w.sessions {
                        if query.is_empty() || s.name.to_lowercase().contains(&query) {
                            sessions.push(s.clone());
                        }
                    }
                }
            }
        }

        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sessions
    }

    /// Render a single task item row.
    fn render_task_item(
        &self,
        session: &Session,
        selected_id: &Option<String>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_selected = selected_id.as_deref() == Some(&session.id);
        let session_id = session.id.clone();
        let session_name = session.name.clone();
        let status_text = session.status_text.clone().unwrap_or_default();
        let duration_text = session.duration_text.clone().unwrap_or_default();

        let accent = theme.accent;
        let selected_text = theme.selected_text;
        let text_color = theme.text;
        let text_muted = theme.text_muted;
        let text_subtle = theme.text_subtle;
        let surface_hover = theme.surface_hover;
        let success = theme.success;

        let is_running = status_text == "运行中";
        let workspace_model = self.workspace_model.clone();

        div()
            .id(ElementId::Name(format!("task-{}", session_id).into()))
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(10.))
            .py(px(8.))
            .mx(px(8.))
            .rounded(px(6.))
            .cursor_pointer()
            .overflow_x_hidden()
            .when(is_selected, move |el: Stateful<Div>| {
                el.bg(accent).text_color(selected_text)
            })
            .when(!is_selected, move |el: Stateful<Div>| {
                el.hover(move |style| style.bg(surface_hover))
            })
            // Task name (truncated)
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .whitespace_nowrap()
                    .overflow_x_hidden()
                    .when(is_selected, move |el| el.text_color(selected_text))
                    .when(!is_selected, move |el| el.text_color(text_color))
                    .child(session_name),
            )
            // Duration + Status row
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .when(!duration_text.is_empty(), {
                        let dt = duration_text.clone();
                        move |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .when(is_selected, move |el| el.text_color(selected_text))
                                    .when(!is_selected, move |el| el.text_color(text_subtle))
                                    .child(dt),
                            )
                        }
                    })
                    .when(!duration_text.is_empty() && !status_text.is_empty(), move |el| {
                        el.child(
                            div()
                                .text_xs()
                                .when(is_selected, move |el| el.text_color(selected_text))
                                .when(!is_selected, move |el| el.text_color(text_subtle))
                                .child("·"),
                        )
                    })
                    .when(!status_text.is_empty(), {
                        let st = status_text.clone();
                        move |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .when(is_selected, move |el| el.text_color(selected_text))
                                    .when(!is_selected && is_running, move |el| el.text_color(success))
                                    .when(!is_selected && !is_running, move |el| el.text_color(text_muted))
                                    .child(st),
                            )
                        }
                    }),
            )
            .on_click(cx.listener(
                move |_this, _, _, cx| {
                    workspace_model.update(cx, |model, cx| {
                        model.select_session(session_id.clone(), cx);
                    });
                },
            ))
    }

    /// Render the task list with section header.
    pub fn render_tasks(
        &mut self,
        search_query: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let sessions = self.collect_sessions(search_query, cx);
        let selected_id = self.workspace_model.read(cx).selected_session_id.clone();
        let text_muted = theme.text_muted;

        let mut container = div().flex().flex_col().w_full();

        // Section header
        container = container.child(
            div()
                .px(px(20.))
                .pt(px(8.))
                .pb(px(4.))
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(text_muted)
                .child("任务记录"),
        );

        if sessions.is_empty() {
            return container.child(
                div()
                    .px(px(20.))
                    .py(px(8.))
                    .text_xs()
                    .text_color(text_muted)
                    .child("暂无任务"),
            );
        }

        for session in &sessions {
            container = container.child(self.render_task_item(session, &selected_id, theme, cx));
        }

        container
    }
}

impl Render for TaskList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        self.render_tasks("", &theme, cx)
    }
}
