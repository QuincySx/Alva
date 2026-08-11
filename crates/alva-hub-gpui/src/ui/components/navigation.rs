// INPUT:  Hub workspace/session snapshots and weak HubApp handle
// OUTPUT: Project-context picker and durable session navigation items
// POS:    Navigation component layer built on gpui-component menus and sidebar

use gpui::{
    prelude::FluentBuilder as _, App, IntoElement, ParentElement as _, RenderOnce, SharedString,
    Styled, WeakEntity, Window,
};
use gpui_component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    sidebar::SidebarMenuItem,
    tag::Tag,
    IconName, Sizable as _,
};

use alva_hub_gpui::HubSessionSummary;

use crate::ui::{app::with_window, format_relative_time};

use super::super::app::HubApp;
use super::super::workspace::default_workspace_tools;

#[derive(IntoElement)]
pub struct HubProjectPicker {
    hub: WeakEntity<HubApp>,
    workspace: Option<SharedString>,
}

impl HubProjectPicker {
    pub fn new(hub: WeakEntity<HubApp>, workspace: Option<impl Into<SharedString>>) -> Self {
        Self {
            hub,
            workspace: workspace.map(Into::into),
        }
    }
}

impl RenderOnce for HubProjectPicker {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let pure_chat = self.hub.clone();
        let choose_project = self.hub.clone();
        let workspace = self.workspace.clone();
        Button::new("composer-project")
            .ghost()
            .small()
            .icon(IconName::Plus)
            .tooltip("Choose conversation context")
            .dropdown_menu(move |menu, _, _| {
                let pure_chat = pure_chat.clone();
                let choose_project = choose_project.clone();
                menu.item(PopupMenuItem::label("Conversation context"))
                    .item(
                        PopupMenuItem::new("Pure conversation")
                            .icon(IconName::Globe)
                            .checked(workspace.is_none())
                            .on_click(move |_, window, app| {
                                if let Some(entity) = pure_chat.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.workspace = None;
                                        hub.reset_session_surface(window, cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .when_some(workspace.clone(), |menu, workspace| {
                        menu.item(
                            PopupMenuItem::new(workspace)
                                .icon(IconName::FolderOpen)
                                .checked(true),
                        )
                    })
                    .separator()
                    .item(
                        PopupMenuItem::new("Choose another project…")
                            .icon(IconName::Folder)
                            .on_click(move |_, window, app| {
                                if let Some(entity) = choose_project.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.choose_workspace(window, cx);
                                    });
                                }
                            }),
                    )
            })
    }
}

#[derive(IntoElement)]
pub struct HubWorkbenchTabMenu {
    hub: WeakEntity<HubApp>,
}

impl HubWorkbenchTabMenu {
    pub fn new(hub: WeakEntity<HubApp>) -> Self {
        Self { hub }
    }
}

impl RenderOnce for HubWorkbenchTabMenu {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let hub = self.hub;
        Button::new("workbench-tools")
            .accessibility_id("hub-workbench-tools")
            .ghost()
            .small()
            .icon(IconName::PanelRightOpen)
            .tooltip("Open a session workspace tool")
            .dropdown_menu(move |mut menu, _, _| {
                menu = menu.item(PopupMenuItem::label("Session workspace"));
                for tool in default_workspace_tools() {
                    let hub = hub.clone();
                    menu = menu.item(PopupMenuItem::new(tool.label()).icon(tool.icon()).on_click(
                        move |_, window, app| {
                            if let Some(entity) = hub.upgrade() {
                                entity.update(app, |hub, cx| {
                                    hub.open_workspace_tool(tool, window, cx);
                                });
                            }
                        },
                    ));
                }
                menu
            })
    }
}

pub struct HubSessionItem {
    session: HubSessionSummary,
    active: bool,
}

impl HubSessionItem {
    pub fn new(session: HubSessionSummary, active_id: Option<&str>) -> Self {
        let active = active_id == Some(session.product_session_id.as_str());
        Self { session, active }
    }
}

impl HubSessionItem {
    pub fn into_sidebar_item(self) -> SidebarMenuItem {
        let session_id = self.session.product_session_id.clone();
        let prompt = self
            .session
            .last_prompt
            .clone()
            .unwrap_or_else(|| "Untitled session".to_string());
        let engine = self.session.engine_id.clone();
        let status = self.session.status.clone();
        let updated = format_relative_time(self.session.updated_at, now_ms());
        SidebarMenuItem::new(prompt)
            .icon(IconName::Inbox)
            .active(self.active)
            .suffix(move |_, _| {
                h_flex()
                    .gap_1()
                    .child(
                        Tag::secondary()
                            .small()
                            .rounded_full()
                            .child(format!("{engine} · {status}")),
                    )
                    .child(gpui::div().text_xs().child(updated.clone()))
            })
            .on_click(move |_, window, cx| {
                let handle = window.window_handle();
                let session_id = session_id.clone();
                with_window(cx, handle, move |hub, window, cx| {
                    let _ = hub.load_session(&session_id, window, cx);
                    cx.notify();
                });
            })
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_item_marks_matching_session_active() {
        let session = HubSessionSummary {
            product_session_id: "session-1".to_string(),
            project_id: "project-1".to_string(),
            engine_id: "alva".to_string(),
            engine_session_id: None,
            model_id: "model".to_string(),
            provider_id: "provider".to_string(),
            status: "active".to_string(),
            last_prompt: None,
            created_at: 0,
            updated_at: 0,
        };

        let item = HubSessionItem::new(session, Some("session-1"));
        assert!(item.active);
    }
}
