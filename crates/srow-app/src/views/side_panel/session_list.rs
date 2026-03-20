use gpui::{prelude::*, Context, Entity, FontWeight, Render, Window, div};

use crate::models::WorkspaceModel;
use crate::theme::Theme;

pub struct SessionList {
    pub workspace_model: Entity<WorkspaceModel>,
}

impl SessionList {
    pub fn new(workspace_model: Entity<WorkspaceModel>, cx: &mut Context<Self>) -> Self {
        cx.subscribe(&workspace_model, |_this, _model, _event, cx| {
            cx.notify();
        })
        .detach();

        Self { workspace_model }
    }
}

impl Render for SessionList {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window);
        let model = self.workspace_model.read(cx);
        let selected_id = model.selected_session_id.clone();
        let sessions = model.sessions.clone();

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
                    .mt_2()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(text_muted)
                    .child("SESSIONS"),
            )
            .children(sessions.into_iter().map(move |session| {
                let is_selected = selected_id.as_ref() == Some(&session.id);
                let session_id = session.id.clone();
                let workspace_model = self.workspace_model.clone();

                div()
                    .id(gpui::ElementId::Name(session.id.clone().into()))
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
                        workspace_model.update(cx, |model, cx| {
                            model.select_session(session_id.clone(), cx);
                        });
                    })
            }))
    }
}
