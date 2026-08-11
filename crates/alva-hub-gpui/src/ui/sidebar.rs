// INPUT:  HubApp state (sessions, workspace, store)
// OUTPUT: gpui-component Sidebar with workspace/session navigation and
//         Sheet entry points for readiness, fallback and team operations.
// POS:    Hub navigation rail; operational forms stay available without a dashboard-style shell.

use gpui::*;
use gpui_component::{
    avatar::Avatar,
    button::{Button, ButtonVariants as _},
    input::{Input, InputEvent},
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
        SidebarMenuItem,
    },
    ActiveTheme, *,
};

use super::app::PrimarySurface;
use super::app::{with_window, HubApp};
use super::components::HubSessionItem;

impl HubApp {
    pub fn sidebar(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let filtered = self.filtered_sessions(cx);
        let active_id = self.active_session_id.clone();
        let querying = !self.session_query.read(cx).value().trim().is_empty();
        let session_items = if let Some(error) = self.session_error.clone() {
            vec![SidebarMenuItem::new(error).disable(true)]
        } else if filtered.is_empty() {
            vec![SidebarMenuItem::new(if querying {
                "No sessions match that search."
            } else {
                "Hub runs will appear here."
            })
            .disable(true)]
        } else {
            filtered
                .iter()
                .cloned()
                .map(|session| {
                    HubSessionItem::new(session, active_id.as_deref()).into_sidebar_item()
                })
                .collect::<Vec<_>>()
        };
        let weak_new = cx.entity().downgrade();
        let weak_chat = cx.entity().downgrade();
        let weak_automation = cx.entity().downgrade();
        let weak_plugins = cx.entity().downgrade();
        let weak_workspace = cx.entity().downgrade();
        let weak_settings = cx.entity().downgrade();
        let weak_readiness = cx.entity().downgrade();
        let weak_fallback = cx.entity().downgrade();
        let weak_team = cx.entity().downgrade();
        let workspace_name = self
            .workspace
            .as_deref()
            .and_then(|workspace| workspace.split('/').next_back())
            .unwrap_or("Choose a project")
            .to_string();

        Sidebar::new("hub-sidebar")
            .w(px(248.))
            .collapsible(SidebarCollapsible::None)
            .header(
                SidebarHeader::new().child(
                    v_flex()
                        .w_full()
                        .gap_2()
                        .child(self.brand_block(cx))
                        .child(Input::new(&self.session_query).small()),
                ),
            )
            .children([
                SidebarGroup::new("Navigate").child(
                    SidebarMenu::new()
                        .child(
                            SidebarMenuItem::new("New conversation")
                                .icon(IconName::Plus)
                                .on_click(move |_, window, app| {
                                    if let Some(entity) = weak_new.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.reset_session_surface(window, cx);
                                            cx.notify();
                                        });
                                    }
                                }),
                        )
                        .child(
                            SidebarMenuItem::new("Chat")
                                .icon(IconName::Inbox)
                                .active(self.primary_surface == PrimarySurface::Chat)
                                .on_click(move |_, _, app| {
                                    if let Some(entity) = weak_chat.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.set_primary_surface(PrimarySurface::Chat, cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            SidebarMenuItem::new("Automation")
                                .icon(IconName::Info)
                                .active(self.primary_surface == PrimarySurface::Automation)
                                .on_click(move |_, _, app| {
                                    if let Some(entity) = weak_automation.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.set_primary_surface(PrimarySurface::Automation, cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            SidebarMenuItem::new("Plugins & skills")
                                .icon(IconName::Bot)
                                .active(self.primary_surface == PrimarySurface::Plugins)
                                .on_click(move |_, _, app| {
                                    if let Some(entity) = weak_plugins.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.set_primary_surface(PrimarySurface::Plugins, cx);
                                        });
                                    }
                                }),
                        ),
                ),
                SidebarGroup::new("Project").child(
                    SidebarMenu::new().child(
                        SidebarMenuItem::new(workspace_name)
                            .icon(IconName::Inbox)
                            .on_click(move |_, window, app| {
                                if let Some(entity) = weak_workspace.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.choose_workspace(window, cx);
                                    });
                                }
                            }),
                    ),
                ),
                SidebarGroup::new("Conversations")
                    .child(SidebarMenu::new().children(session_items)),
            ])
            .footer(
                SidebarFooter::new().child(
                    v_flex()
                        .gap_2()
                        .child(
                            Button::new("settings")
                                .ghost()
                                .small()
                                .w_full()
                                .icon(IconName::Info)
                                .label("Settings")
                                .selected(self.primary_surface == PrimarySurface::Settings)
                                .on_click(move |_, _, app| {
                                    if let Some(entity) = weak_settings.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.set_primary_surface(PrimarySurface::Settings, cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            h_flex()
                                .gap_1()
                                .child(
                                    Button::new("readiness-shortcut")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::Info)
                                        .tooltip("Engine readiness")
                                        .on_click(move |_, window, app| {
                                            if let Some(entity) = weak_readiness.upgrade() {
                                                entity.update(app, |hub, cx| {
                                                    hub.open_readiness_sheet(window, cx);
                                                });
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("fallback-shortcut")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::ArrowRight)
                                        .tooltip("Fallback policy")
                                        .on_click(move |_, window, app| {
                                            if let Some(entity) = weak_fallback.upgrade() {
                                                entity.update(app, |hub, cx| {
                                                    hub.open_fallback_sheet(window, cx);
                                                });
                                            }
                                        }),
                                )
                                .child(
                                    Button::new("team-shortcut")
                                        .ghost()
                                        .xsmall()
                                        .icon(IconName::User)
                                        .tooltip("Team workers")
                                        .on_click(move |_, window, app| {
                                            if let Some(entity) = weak_team.upgrade() {
                                                entity.update(app, |hub, cx| {
                                                    hub.open_team_sheet(window, cx);
                                                });
                                            }
                                        }),
                                )
                                .child(self.ticket_note(cx)),
                        ),
                ),
            )
    }

    pub fn open_readiness_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        window.open_sheet(cx, move |sheet, window, cx| {
            let Some(entity) = weak.upgrade() else {
                return sheet;
            };
            entity.update(cx, |hub, cx| {
                sheet
                    .size(px(460.))
                    .title("Engine readiness")
                    .child(hub.readiness_panel(window, cx))
            })
        });
    }

    pub fn open_fallback_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.collapsed.insert("fallback".to_string(), false);
        let weak = cx.entity().downgrade();
        window.open_sheet(cx, move |sheet, window, cx| {
            let Some(entity) = weak.upgrade() else {
                return sheet;
            };
            entity.update(cx, |hub, cx| {
                sheet
                    .size(px(480.))
                    .title("Fallback policy")
                    .child(hub.fallback_panel(window, cx))
            })
        });
    }

    pub fn open_team_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.collapsed.insert("team".to_string(), false);
        let weak = cx.entity().downgrade();
        window.open_sheet(cx, move |sheet, window, cx| {
            let Some(entity) = weak.upgrade() else {
                return sheet;
            };
            entity.update(cx, |hub, cx| {
                sheet
                    .size(px(520.))
                    .title("Team workers")
                    .child(hub.team_panel(window, cx))
            })
        });
    }

    /// Brand mark + title + tagline.
    fn brand_block(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .items_center()
            .child(Avatar::new().name("ALVA Hub").placeholder(IconName::Bot))
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

    pub fn choose_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let pick = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose a local project".into()),
        });
        let handle = window.window_handle();
        cx.spawn(move |_hub, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                if let Ok(Ok(Some(paths))) = pick.await {
                    if let Some(path) = paths.first() {
                        let path = path.display().to_string();
                        with_window(&mut cx, handle, move |hub, window, cx| {
                            hub.workspace = Some(path.clone());
                            hub.reset_session_surface(window, cx);
                            if let Ok(policy) = hub.store.global_fallback_policy() {
                                hub.fallback_policy = policy;
                            }
                            hub.sync_policy_inputs(window, cx);
                            hub.refresh_readiness(cx);
                            cx.notify();
                        });
                    }
                }
            }
        })
        .detach();
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

    fn ticket_note(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
            .flex_1()
            .text_right()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child("Alva · Claude · Codex")
    }
}
