// INPUT:  gpui, crate::models (WorkspaceModel, SidebarItem), crate::types::Session, crate::theme::Theme, chrono
// OUTPUT: pub struct SessionList
// POS:    Renders sessions grouped by time (Today, Yesterday, Last 7 Days, Earlier) with selection highlighting.
use gpui::{prelude::*, *};

use crate::models::{SidebarItem, WorkspaceModel};
use crate::theme::Theme;
use crate::types::Session;

/// Time group label for session grouping.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum TimeGroup {
    Today,
    Yesterday,
    Last7Days,
    Earlier,
}

impl TimeGroup {
    fn label(&self) -> &'static str {
        match self {
            TimeGroup::Today => "Today",
            TimeGroup::Yesterday => "Yesterday",
            TimeGroup::Last7Days => "Last 7 Days",
            TimeGroup::Earlier => "Earlier",
        }
    }

    fn from_timestamp(ts_millis: i64) -> Self {
        use chrono::{Local, TimeZone, Duration};

        let now = Local::now();
        let today_start = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let today_start = Local.from_local_datetime(&today_start).unwrap();

        let yesterday_start = today_start - Duration::days(1);
        let week_ago_start = today_start - Duration::days(7);

        let ts_secs = ts_millis / 1000;
        let nanos = ((ts_millis % 1000) * 1_000_000) as u32;
        let item_time = match Local.timestamp_opt(ts_secs, nanos) {
            chrono::offset::LocalResult::Single(t) => t,
            _ => return TimeGroup::Earlier,
        };

        if item_time >= today_start {
            TimeGroup::Today
        } else if item_time >= yesterday_start {
            TimeGroup::Yesterday
        } else if item_time >= week_ago_start {
            TimeGroup::Last7Days
        } else {
            TimeGroup::Earlier
        }
    }
}

/// A flattened session entry carrying enough context for rendering.
#[derive(Clone)]
struct FlatSession {
    session: Session,
    group: TimeGroup,
}

pub struct SessionList {
    workspace_model: Entity<WorkspaceModel>,
    _subscriptions: Vec<Subscription>,
}

impl SessionList {
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

    /// Flatten all sessions from sidebar_items, filter by query, sort and group.
    fn collect_sessions(&self, search_query: &str, cx: &Context<Self>) -> Vec<FlatSession> {
        let ws = self.workspace_model.read(cx);
        let query = search_query.to_lowercase();

        let mut sessions: Vec<FlatSession> = Vec::new();

        for item in &ws.sidebar_items {
            match item {
                SidebarItem::GlobalSession(s) => {
                    if query.is_empty() || s.name.to_lowercase().contains(&query) {
                        sessions.push(FlatSession {
                            group: TimeGroup::from_timestamp(s.updated_at),
                            session: s.clone(),
                        });
                    }
                }
                SidebarItem::Workspace(w) => {
                    for s in &w.sessions {
                        if query.is_empty() || s.name.to_lowercase().contains(&query) {
                            sessions.push(FlatSession {
                                group: TimeGroup::from_timestamp(s.updated_at),
                                session: s.clone(),
                            });
                        }
                    }
                }
            }
        }

        // Sort by updated_at descending (most recent first)
        sessions.sort_by(|a, b| b.session.updated_at.cmp(&a.session.updated_at));

        sessions
    }

    /// Render a single session item row.
    fn render_session_item(
        &self,
        flat: &FlatSession,
        selected_id: &Option<String>,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_selected = selected_id.as_deref() == Some(&flat.session.id);
        let session_id = flat.session.id.clone();
        let session_name = flat.session.name.clone();

        let accent = theme.accent;
        let selected_text = theme.selected_text;
        let text_color = theme.text;
        let surface_hover = theme.surface_hover;

        let workspace_model = self.workspace_model.clone();

        div()
            .id(ElementId::Name(format!("session-{}", session_id).into()))
            .px_2()
            .py(px(4.))
            .mx_1()
            .rounded_md()
            .cursor_pointer()
            .text_xs()
            .overflow_x_hidden()
            .whitespace_nowrap()
            .when(is_selected, move |el: gpui::Stateful<gpui::Div>| {
                el.bg(accent).text_color(selected_text)
            })
            .when(!is_selected, move |el: gpui::Stateful<gpui::Div>| {
                el.text_color(text_color)
                    .hover(move |style| style.bg(surface_hover))
            })
            .child(session_name)
            .on_click(cx.listener(
                srow_debug::traced_listener!("sidebar:select_session", move |_this, _, _, cx| {
                    tracing::info!(session_id = %session_id, "select_session");
                    workspace_model.update(cx, |model, cx| {
                        model.select_session(session_id.clone(), cx);
                    });
                }),
            ))
    }

    /// Render all sessions grouped by time.
    pub fn render_grouped(
        &mut self,
        search_query: &str,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let sessions = self.collect_sessions(search_query, cx);
        let selected_id = self.workspace_model.read(cx).selected_session_id.clone();
        let text_muted = theme.text_muted;

        let mut container = div().flex().flex_col().w_full();

        if sessions.is_empty() {
            return container.child(
                div()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(text_muted)
                    .child("No sessions found"),
            );
        }

        // Group sessions maintaining order (already sorted by updated_at desc)
        let mut current_group: Option<TimeGroup> = None;

        for flat in &sessions {
            if current_group.as_ref() != Some(&flat.group) {
                current_group = Some(flat.group.clone());
                // Render group header
                container = container.child(
                    div()
                        .px_3()
                        .pt_2()
                        .pb(px(2.))
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(text_muted)
                        .child(flat.group.label()),
                );
            }
            container = container.child(self.render_session_item(flat, &selected_id, theme, cx));
        }

        container
    }
}

impl Render for SessionList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        // When rendered standalone (without search), show all
        self.render_grouped("", &theme, cx)
    }
}
