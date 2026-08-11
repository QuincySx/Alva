// INPUT:  HubApp state (sessions, workspace, store)
// OUTPUT: gpui-component Sidebar with workspace/session navigation and
//         Sheet entry points for readiness, fallback and team operations.
// POS:    Hub navigation rail; operational forms stay available without a dashboard-style shell.

use gpui::*;
use gpui_component::{
    avatar::Avatar,
    button::{Button, ButtonVariants as _},
    group_box::{GroupBox, GroupBoxVariants as _},
    input::{Input, InputEvent},
    sidebar::{
        Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
        SidebarMenuItem,
    },
    tag::Tag,
    ActiveTheme, *,
};

use alva_hub_gpui::HubSessionSummary;

use super::app::{with_window, HubApp};
use super::format_relative_time;

impl HubApp {
    pub fn sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                .map(|session| session_menu_item(session, &active_id))
                .collect::<Vec<_>>()
        };
        let weak_new = cx.entity().downgrade();
        let weak_readiness = cx.entity().downgrade();
        let weak_fallback = cx.entity().downgrade();
        let weak_team = cx.entity().downgrade();

        Sidebar::new("hub-sidebar")
            .w(px(292.))
            .collapsible(SidebarCollapsible::None)
            .header(
                SidebarHeader::new().child(
                    v_flex()
                        .w_full()
                        .gap_3()
                        .child(self.brand_block(cx))
                        .child(self.workspace_panel(window, cx))
                        .child(Input::new(&self.session_query).small())
                        .child(
                            Button::new("new-session")
                                .secondary()
                                .small()
                                .icon(IconName::Plus)
                                .label("New session")
                                .tooltip("Start a fresh Hub session")
                                .disabled(self.busy)
                                .on_click(move |_, window, app| {
                                    if let Some(entity) = weak_new.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.reset_session_surface(window, cx);
                                            cx.notify();
                                        });
                                    }
                                }),
                        ),
                ),
            )
            .children([
                SidebarGroup::new("Sessions").child(SidebarMenu::new().children(session_items)),
                SidebarGroup::new("Operations").child(
                    SidebarMenu::new()
                        .child(
                            SidebarMenuItem::new("Engine readiness")
                                .icon(IconName::Info)
                                .on_click(move |_, window, app| {
                                    if let Some(entity) = weak_readiness.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.open_readiness_sheet(window, cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            SidebarMenuItem::new("Fallback policy")
                                .icon(IconName::ArrowRight)
                                .on_click(move |_, window, app| {
                                    if let Some(entity) = weak_fallback.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.open_fallback_sheet(window, cx);
                                        });
                                    }
                                }),
                        )
                        .child(
                            SidebarMenuItem::new("Team workers")
                                .icon(IconName::User)
                                .on_click(move |_, window, app| {
                                    if let Some(entity) = weak_team.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.open_team_sheet(window, cx);
                                        });
                                    }
                                }),
                        ),
                ),
            ])
            .footer(SidebarFooter::new().child(self.ticket_note(cx)))
    }

    fn open_readiness_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn open_fallback_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn open_team_sheet(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
        GroupBox::new()
            .outline()
            .title("Workspace")
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_sm().font_weight(FontWeight(600.)).child(name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(path),
                    ),
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

fn session_menu_item(session: &HubSessionSummary, active_id: &Option<String>) -> SidebarMenuItem {
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
    let engine = session.engine_id.clone();
    let status = session.status.clone();
    let updated = format_relative_time(session.updated_at, now_ms());
    let is_active = active_id.as_deref() == Some(pid.as_str());
    SidebarMenuItem::new(format!("{name} — {prompt}"))
        .icon(IconName::Inbox)
        .active(is_active)
        .suffix(move |_, _| {
            h_flex()
                .gap_1()
                .child(
                    Tag::secondary()
                        .small()
                        .rounded_full()
                        .child(format!("{engine} · {status}")),
                )
                .child(div().text_xs().child(updated.clone()))
        })
        .on_click(move |_, window, cx| {
            let handle = window.window_handle();
            let pid = pid.clone();
            with_window(cx, handle, move |hub, window, cx| {
                let _ = hub.load_session(&pid, window, cx);
                cx.notify();
            });
        })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
