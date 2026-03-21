// INPUT:  gpui, gpui_component (Button, ButtonVariants), crate::models::WorkspaceModel, crate::theme, sidebar_tree::SidebarTree
// OUTPUT: pub struct SidePanel
// POS:    Left sidebar view containing a "New Session" button (gpui-component) and the SidebarTree workspace/session list.
use gpui::{prelude::*, Context, Entity, Render, Window, div};

use gpui_component::button::Button;

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
                    .mx_2()
                    .mt_2()
                    .mb_1()
                    .child(
                        Button::new("new-session-btn")
                            .label("+ New")
                            .outline()
                            .on_click(move |_, _, cx| {
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
                            })
                    )
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
