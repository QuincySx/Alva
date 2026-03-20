use gpui::{prelude::*, Context, Entity, FontWeight, Render, Window, div};

use crate::models::WorkspaceModel;
use crate::theme::Theme;

pub struct WorkspaceList {
    pub workspace_model: Entity<WorkspaceModel>,
}

impl WorkspaceList {
    pub fn new(workspace_model: Entity<WorkspaceModel>, cx: &mut Context<Self>) -> Self {
        cx.subscribe(&workspace_model, |_this, _model, _event, cx| {
            cx.notify();
        })
        .detach();

        Self { workspace_model }
    }
}

impl Render for WorkspaceList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window);
        let model = self.workspace_model.read(cx);
        let selected_id = model.selected_workspace_id.clone();
        let workspaces = model.workspaces.clone();

        let accent = theme.accent;
        let surface_hover = theme.surface_hover;
        let text_color = theme.text;
        let text_muted = theme.text_muted;

        div()
            .flex()
            .flex_col()
            .w_full()
            .child(
                div()
                    .px_3()
                    .py_1()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(text_muted)
                    .child("WORKSPACES"),
            )
            .children(workspaces.into_iter().map(move |ws| {
                let is_selected = selected_id.as_ref() == Some(&ws.id);
                let ws_id = ws.id.clone();
                let workspace_model = self.workspace_model.clone();

                div()
                    .id(gpui::ElementId::Name(ws.id.clone().into()))
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
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(ws.name.clone())
                            .child(
                                div()
                                    .text_xs()
                                    .when(is_selected, |el| el.text_color(gpui::white().opacity(0.7)))
                                    .when(!is_selected, |el| el.text_color(text_muted))
                                    .child(ws.path.clone()),
                            ),
                    )
                    .on_click(move |_, _, cx| {
                        workspace_model.update(cx, |model, cx| {
                            model.select_workspace(ws_id.clone(), cx);
                        });
                    })
            }))
    }
}
