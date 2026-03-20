use gpui::{prelude::*, Context, Entity, FontWeight, Render, Window, div};

use crate::models::WorkspaceModel;
use crate::theme::Theme;
use super::sidebar_tree::SidebarTree;

pub struct SidePanel {
    sidebar_tree: Entity<SidebarTree>,
    workspace_model: Entity<WorkspaceModel>,
}

impl SidePanel {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        cx: &mut Context<Self>,
    ) -> Self {
        let wm = workspace_model.clone();
        let sidebar_tree = cx.new(|cx| SidebarTree::new(wm, cx));

        Self {
            sidebar_tree,
            workspace_model,
        }
    }
}

impl Render for SidePanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window, cx);
        let workspace_model = self.workspace_model.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.surface)
            .child(
                // "+ New" button at top
                div()
                    .id("new-session-btn")
                    .flex()
                    .items_center()
                    .justify_center()
                    .px_3()
                    .py_2()
                    .mx_2()
                    .mt_2()
                    .mb_1()
                    .rounded_md()
                    .cursor_pointer()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.accent)
                    .border_1()
                    .border_color(theme.accent)
                    .hover(move |style| style.bg(theme.surface_hover))
                    .child("+ New")
                    .on_click(move |_, _, cx| {
                        // Placeholder: create a new global session
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
                    }),
            )
            .child(
                // Tree list
                div()
                    .flex_1()
                    .py_1()
                    .child(self.sidebar_tree.clone()),
            )
    }
}
