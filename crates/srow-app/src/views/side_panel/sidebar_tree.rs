// INPUT:  gpui, crate::models (SidebarItem, WorkspaceModel), crate::theme
// OUTPUT: pub struct SidebarTree
// POS:    Scrollable tree view rendering workspace folders (expand/collapse) and global sessions with selection highlighting.
use gpui::{prelude::*, Context, Entity, Render, Window, div};

use crate::models::{SidebarItem, WorkspaceModel};
use crate::theme::Theme;

pub struct SidebarTree {
    pub workspace_model: Entity<WorkspaceModel>,
}

impl SidebarTree {
    pub fn new(workspace_model: Entity<WorkspaceModel>, cx: &mut Context<Self>) -> Self {
        cx.subscribe(&workspace_model, |_this, _model, _event, cx| {
            cx.notify();
        })
        .detach();

        Self { workspace_model }
    }
}

impl Render for SidebarTree {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let model = self.workspace_model.read(cx);
        let selected_id = model.selected_session_id.clone();
        let items = model.sidebar_items.clone();

        let accent = theme.accent;
        let surface_hover = theme.surface_hover;
        let text_color = theme.text;
        let text_muted = theme.text_muted;

        let mut container = div().id("sidebar-tree").flex().flex_col().w_full().overflow_y_scroll();

        for item in items {
            match item {
                SidebarItem::GlobalSession(session) => {
                    let is_selected = selected_id.as_ref() == Some(&session.id);
                    let session_id = session.id.clone();
                    let workspace_model = self.workspace_model.clone();

                    container = container.child(
                        div()
                            .id(gpui::ElementId::Name(
                                format!("gs-{}", session.id).into(),
                            ))
                            .px_3()
                            .py_1p5()
                            .mx_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_sm()
                            .when(is_selected, |el| {
                                el.bg(accent).text_color(gpui::white())
                            })
                            .when(!is_selected, |el| {
                                el.text_color(text_color)
                                    .hover(move |style| style.bg(surface_hover))
                            })
                            .child(session.name.clone())
                            .on_click(move |_, _, cx| {
                                tracing::info!(session_id = %session_id, "action_dispatch: select_session");
                                workspace_model.update(cx, |model, cx| {
                                    model.select_session(session_id.clone(), cx);
                                });
                            }),
                    );
                }
                SidebarItem::Workspace(ws) => {
                    let ws_id = ws.id.clone();
                    let workspace_model_toggle = self.workspace_model.clone();
                    let expanded = ws.expanded;
                    let arrow = if expanded { "▼" } else { "▶" };

                    // Workspace header row
                    container = container.child(
                        div()
                            .id(gpui::ElementId::Name(
                                format!("ws-{}", ws.id).into(),
                            ))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_1()
                            .px_3()
                            .py_1p5()
                            .mx_1()
                            .rounded_md()
                            .cursor_pointer()
                            .text_sm()
                            .text_color(text_color)
                            .hover(move |style| style.bg(surface_hover))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(text_muted)
                                    .child(arrow),
                            )
                            .child(format!("\u{1F4C1} {}", ws.name))
                            .on_click(move |_, _, cx| {
                                workspace_model_toggle.update(cx, |model, cx| {
                                    model.toggle_workspace(ws_id.clone(), cx);
                                });
                            }),
                    );

                    // Child sessions (only if expanded)
                    if expanded {
                        for session in &ws.sessions {
                            let is_selected =
                                selected_id.as_ref() == Some(&session.id);
                            let session_id = session.id.clone();
                            let workspace_model_select =
                                self.workspace_model.clone();

                            container = container.child(
                                div()
                                    .id(gpui::ElementId::Name(
                                        format!("s-{}", session.id).into(),
                                    ))
                                    .pl_6()
                                    .pr_3()
                                    .py_1()
                                    .mx_1()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .text_sm()
                                    .when(is_selected, |el| {
                                        el.bg(accent)
                                            .text_color(gpui::white())
                                    })
                                    .when(!is_selected, |el| {
                                        el.text_color(text_color).hover(
                                            move |style| {
                                                style.bg(surface_hover)
                                            },
                                        )
                                    })
                                    .child(format!("  {}", session.name))
                                    .on_click(move |_, _, cx| {
                                        workspace_model_select.update(
                                            cx,
                                            |model, cx| {
                                                model.select_session(
                                                    session_id.clone(),
                                                    cx,
                                                );
                                            },
                                        );
                                    }),
                            );
                        }
                    }
                }
            }
        }

        container
    }
}
