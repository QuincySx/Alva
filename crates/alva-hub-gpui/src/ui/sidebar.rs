// INPUT:  HubApp state (sessions, workspace, store)
// OUTPUT: sidebar — brand, workspace panel, sessions panel (search / new /
//         list with active highlight); readiness / fallback / team panels
//         live in sibling modules and are appended by `sidebar()`.
// POS:    1:1 GPUI port of the React sidebar in App.tsx (292px column).

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent},
    scroll::ScrollableElement,
    ActiveTheme, *,
};

use alva_hub_gpui::HubSessionSummary;

use super::app::HubApp;
use super::format_relative_time;

impl HubApp {
    pub fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("hub-sidebar")
            .flex_col()
            .gap_2()
            .px_3()
            .py_3()
            .w(px(292.))
            .bg(cx.theme().colors.sidebar)
            .border_r_1()
            .border_color(cx.theme().colors.border)
            .overflow_y_scrollbar()
            .child(self.brand_block(cx))
            .child(self.workspace_panel(window, cx))
            .child(self.sessions_panel(window, cx))
            .child(self.readiness_panel(window, cx))
            .child(self.fallback_panel(window, cx))
            .child(self.team_panel(window, cx))
            .child(self.ticket_note(cx))
    }

    /// Brand mark + title + tagline.
    fn brand_block(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .size_8()
                    .rounded_lg()
                    .bg(cx.theme().colors.primary)
                    .text_color(cx.theme().colors.primary_foreground)
                    .v_flex()
                    .items_center()
                    .justify_center()
                    .text_lg()
                    .font_weight(FontWeight(800.))
                    .child("A"),
            )
            .child(
                v_flex()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight(700.))
                            .child("ALVA Hub"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child("One project. Every engine."),
                    ),
            )
    }

    /// Workspace picker panel.
    fn workspace_panel(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self
            .workspace
            .as_deref()
            .map(|w| w.split('/').next_back().unwrap_or(w).to_string())
            .unwrap_or_else(|| "Not selected".to_string());
        let path = self
            .workspace
            .clone()
            .unwrap_or_else(|| "Choose a local project to begin.".to_string());
        v_flex()
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors.border)
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight(650.))
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("Workspace"),
            )
            .child(div().text_sm().font_weight(FontWeight(600.)).child(name))
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(path),
            )
            .child(
                Button::new("choose-workspace")
                    .secondary()
                    .small()
                    .label("Choose workspace")
                    .on_click(move |_, window, app| {
                        let pick = app.prompt_for_paths(gpui::PathPromptOptions {
                            files: false,
                            directories: true,
                            multiple: false,
                            prompt: Some("Choose a local project".into()),
                        });
                        let handle = window.window_handle();
                        app.spawn(move |cx: &mut AsyncApp| {
                            let mut cx = cx.clone();
                            async move {
                                if let Ok(Ok(Some(paths))) = pick.await {
                                    if let Some(path) = paths.first() {
                                        let path = path.display().to_string();
                                        let _ = handle.update(&mut cx, move |root, window, cx| {
                                            if let Ok(view) = root.downcast::<HubApp>() {
                                                view.update(cx, |hub, cx| {
                                                    hub.workspace = Some(path.clone());
                                                    hub.reset_session_surface(window, cx);
                                                    hub.refresh_readiness(cx);
                                                    cx.notify();
                                                });
                                            }
                                        });
                                    }
                                }
                            }
                        })
                        .detach();
                    }),
            )
    }

    /// Subscribe the session search input so a query change re-renders the
    /// filtered list (AGENTS.md §1.2: subscribe_in, inner cx).
    pub fn subscribe_sessions(&self, window: &mut Window, cx: &mut Context<Self>) {
        let _subscription = cx.subscribe_in(
            &self.session_query,
            window,
            |_view, _state, event, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        );
    }

    /// Sessions panel: heading + search + New session + list.
    fn sessions_panel(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let filtered = self.filtered_sessions(cx);
        let total = self.sessions.len();
        let query = self.session_query.read(cx).value().to_string();
        let querying = !query.trim().is_empty();
        let active_id = self.active_session_id.clone();
        let error = self.session_error.clone();
        v_flex()
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors.border)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight(650.))
                            .text_color(cx.theme().colors.muted_foreground)
                            .child("Sessions"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(if querying {
                                format!("{}/{}", filtered.len(), total)
                            } else {
                                total.to_string()
                            }),
                    ),
            )
            .child(Input::new(&self.session_query).small())
            .child(
                Button::new("new-session")
                    .secondary()
                    .small()
                    .label("New session")
                    .disabled(self.busy)
                    .on_click(move |_, window, app| {
                        let handle = window.window_handle();
                        let _ = handle.update(app, move |root, window, cx| {
                            if let Ok(view) = root.downcast::<HubApp>() {
                                view.update(cx, |hub, cx| {
                                    hub.reset_session_surface(window, cx);
                                    cx.notify();
                                });
                            }
                        });
                    }),
            )
            .child(
                div()
                    .flex_col()
                    .gap_1()
                    .children(if let Some(error) = error {
                        vec![div()
                            .text_xs()
                            .text_color(cx.theme().colors.danger)
                            .child(error)
                            .into_any_element()]
                    } else if filtered.is_empty() {
                        vec![div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(if querying {
                                "No sessions match that search."
                            } else {
                                "Hub runs will appear here."
                            })
                            .into_any_element()]
                    } else {
                        filtered
                            .iter()
                            .map(|session| session_item(session, &active_id, cx).into_any_element())
                            .collect::<Vec<_>>()
                    }),
            )
    }

    fn ticket_note(&self, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_1()
            .px_2()
            .py_2()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight(600.))
                    .child("Durable engine branches"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("Alva · Claude · Codex · reversible handoff"),
            )
    }
}

/// One clickable session row with active highlight.
fn session_item(
    session: &HubSessionSummary,
    active_id: &Option<String>,
    cx: &Context<HubApp>,
) -> impl IntoElement {
    let pid = session.product_session_id.clone();
    let name = session
        .workspace
        .split('/')
        .next_back()
        .unwrap_or(&session.workspace)
        .to_string();
    let prompt = session
        .last_prompt
        .clone()
        .unwrap_or_else(|| "Untitled session".to_string());
    let result = session.last_result.clone();
    let meta = format!(
        "{} · {} · {}",
        session.engine_id,
        session.status,
        format_relative_time(session.updated_at, now_ms())
    );
    let is_active = active_id.as_deref() == Some(pid.as_str());
    let muted = cx.theme().colors.muted_foreground;
    let mut row = div()
        .id(SharedString::from(format!("session-{pid}")))
        .v_flex()
        .gap_1()
        .px_1()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .child(div().text_sm().font_weight(FontWeight(600.)).child(name))
        .child(div().text_xs().text_color(muted).child(prompt));
    if let Some(result) = result {
        row = row.child(div().text_xs().text_color(muted).child(result));
    }
    row = row.child(div().text_xs().text_color(muted).child(meta));
    row.on_click(move |_, window, cx| {
        let handle = window.window_handle();
        let _ = handle.update(cx, |root, window, cx| {
            if let Ok(view) = root.downcast::<HubApp>() {
                view.update(cx, |hub, cx| {
                    let _ = hub.load_session(&pid, window, cx);
                    cx.notify();
                });
            }
        });
    })
    .when(is_active, |this| this.bg(cx.theme().colors.button))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
