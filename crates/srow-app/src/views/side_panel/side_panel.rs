use gpui::{prelude::*, Context, Entity, Render, Window, div};

use crate::models::WorkspaceModel;
use crate::theme::Theme;
use super::workspace_list::WorkspaceList;
use super::session_list::SessionList;

pub struct SidePanel {
    workspace_list: Entity<WorkspaceList>,
    session_list: Entity<SessionList>,
}

impl SidePanel {
    pub fn new(
        workspace_model: Entity<WorkspaceModel>,
        cx: &mut Context<Self>,
    ) -> Self {
        let wm1 = workspace_model.clone();
        let wm2 = workspace_model;

        let workspace_list = cx.new(|cx| WorkspaceList::new(wm1, cx));
        let session_list = cx.new(|cx| SessionList::new(wm2, cx));

        Self {
            workspace_list,
            session_list,
        }
    }
}

impl Render for SidePanel {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window);

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.surface)
            .py_2()
            .child(self.workspace_list.clone())
            .child(self.session_list.clone())
    }
}
