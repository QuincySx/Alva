// INPUT:  gpui + gpui-component
// OUTPUT: ALVA Hub Desktop Rust-native shell (P1: main column + composer)
// POS:    GPUI migration path — same L7 product seam as alva-hub-desktop.
//         Follows crates/alva-hub-gpui/AGENTS.md: Entity<State> for
//         stateful components, theme colors only, no hardcoded hex.

use alva_hub_gpui::{
    build_engine, run_persisted, HubSessionSummary, HubStore, RunRequest, TimelineEvent,
    TimelineStatus,
};
use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::{Input, InputState},
    searchable_list::SearchableVec,
    select::{Select, SelectState},
    sidebar::{Sidebar, SidebarFooter, SidebarHeader, SidebarMenu, SidebarMenuItem},
    ActiveTheme, Icon, IconName, *,
};

pub struct HubApp {
    store: Arc<HubStore>,
    workspace: Option<String>,
    engine: Entity<SelectState<SearchableVec<&'static str>>>,
    permission: Entity<SelectState<SearchableVec<&'static str>>>,
    prompt: Entity<InputState>,
    /// Timeline events projected by the last run (batch 2).
    timeline: Vec<TimelineEvent>,
}

impl HubApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // P3: backend store opened directly (dev db, own instance lock so it
        // coexists with the Tauri build during migration).
        let store_path = std::env::var("ALVA_HUB_DB").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            format!("{home}/Library/Application Support/com.alva.hub-gpui/alva-hub.db")
        });
        let store = Arc::new(HubStore::open(&store_path).expect("open Hub store"));
        Self {
            store,
            workspace: None,
            timeline: Vec::new(),
            engine: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(vec!["alva", "claude", "codex"]),
                    Some(IndexPath::new(0)),
                    window,
                    cx,
                )
            }),
            permission: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(vec!["interactive", "workspace_write", "unrestricted"]),
                    Some(IndexPath::new(1)),
                    window,
                    cx,
                )
            }),
            prompt: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Ask alva to inspect, change, or explain this project…")
            }),
        }
    }

    fn sidebar_menu(
        sessions: &[HubSessionSummary],
        workspace: Option<&str>,
        cx: &Context<Self>,
    ) -> SidebarMenu {
        let session_items: Vec<SidebarMenuItem> = if sessions.is_empty() {
            vec![SidebarMenuItem::new("(no sessions yet)")]
        } else {
            sessions
                .iter()
                .map(|s| {
                    let label = format!(
                        "{} · {} · {}",
                        s.workspace.split('/').next_back().unwrap_or(&s.workspace),
                        s.engine_id,
                        s.status
                    );
                    SidebarMenuItem::new(label)
                })
                .collect()
        };
        let weak = cx.entity().downgrade();
        let workspace_label = workspace
            .map(|w| w.split('/').next_back().unwrap_or(w).to_string())
            .unwrap_or_else(|| "Not selected".to_string());
        SidebarMenu::new().children([
            SidebarMenuItem::new(workspace_label)
                .icon(IconName::Folder)
                .on_click(move |_, _window, app| {
                    let weak = weak.clone();
                    let pick = app.prompt_for_paths(gpui::PathPromptOptions {
                        files: false,
                        directories: true,
                        multiple: false,
                        prompt: Some("Choose a local project".into()),
                    });
                    app.spawn(|cx: &mut AsyncApp| {
                        let mut cx = cx.clone();
                        async move {
                            if let Ok(Ok(Some(paths))) = pick.await {
                                if let Some(path) = paths.first() {
                                    if let Some(entity) = weak.upgrade() {
                                        cx.update_entity(&entity, |hub, cx| {
                                            hub.workspace = Some(path.display().to_string());
                                            cx.notify();
                                        });
                                    }
                                }
                            }
                        }
                    })
                    .detach();
                }),
            SidebarMenuItem::new("Sessions")
                .icon(IconName::SquareTerminal)
                .click_to_toggle(true)
                .default_open(true)
                .children(session_items),
            SidebarMenuItem::new("Engines")
                .icon(IconName::Cpu)
                .click_to_toggle(true)
                .children([
                    SidebarMenuItem::new("Alva · healthy"),
                    SidebarMenuItem::new("Claude · healthy"),
                    SidebarMenuItem::new("Codex · sign-in needed"),
                ]),
            SidebarMenuItem::new("Fallback chain")
                .icon(IconName::Github)
                .click_to_toggle(true)
                .children([
                    SidebarMenuItem::new("1. alva"),
                    SidebarMenuItem::new("2. claude"),
                    SidebarMenuItem::new("3. codex"),
                ]),
            SidebarMenuItem::new("Team")
                .icon(IconName::User)
                .click_to_toggle(true)
                .children([SidebarMenuItem::new("(open a session to lead a team run)")]),
        ])
    }

    fn sessions(&self) -> Vec<HubSessionSummary> {
        self.store.list_sessions().unwrap_or_default()
    }

    /// Batch 2: wire Run to the migrated backend run_persisted.
    fn run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.clone() else {
            window.push_notification("Choose a workspace before running an engine.", cx);
            return;
        };
        let prompt = self.prompt.read(cx).value().to_string();
        if prompt.trim().is_empty() {
            return;
        }
        let engine_id = self
            .engine
            .read(cx)
            .selected_value()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "alva".to_string());
        let store = Arc::clone(&self.store);
        let history_dir = store.path().parent().map(PathBuf::from);
        let _ = window;
        // Foreground task per GPUI async docs: closure receives
        // (WeakEntity<Self>, &mut AsyncApp). The emit callback must be
        // Send, so events are collected locally and applied in one pass.
        cx.spawn(async move |this, cx: &mut AsyncApp| {
            let mut events = Vec::new();
            let outcome = run_persisted(
                &store,
                {
                    let ws_path = PathBuf::from(&workspace);
                    move |target, _pid, profile| {
                        let hist = history_dir.clone().unwrap_or_else(|| ws_path.clone());
                        build_engine(&target.engine_id, profile, &ws_path, &hist)
                    }
                },
                |_| None,
                RunRequest {
                    prompt: prompt.clone(),
                    workspace: PathBuf::from(&workspace),
                    engine_id: engine_id.clone(),
                    product_session_id: None,
                    permission_profile: None,
                },
                |event| events.push(event),
            )
            .await;
            if let Ok(outcome) = outcome {
                events.push(TimelineEvent {
                    status: if outcome.failed {
                        TimelineStatus::Failed
                    } else {
                        TimelineStatus::Completed
                    },
                    engine_id: engine_id.clone(),
                    product_session_id: outcome.product_session_id.clone(),
                    message: outcome.result.clone(),
                    permission_request: None,
                });
            }
            let _ = this.update(cx, |hub, cx| {
                hub.timeline.extend(events);
                cx.notify();
            });
        })
        .detach();
    }

    fn timeline_cards(&self, cx: &Context<Self>) -> impl IntoElement {
        let cards: Vec<gpui::AnyElement> = self
            .timeline
            .iter()
            .enumerate()
            .map(|(i, event)| {
                let status = match event.status {
                    TimelineStatus::Completed => "completed",
                    TimelineStatus::Failed => "failed",
                    TimelineStatus::Fallback => "fallback",
                    TimelineStatus::Interrupted => "interrupted",
                    TimelineStatus::Running => "running",
                    _ => "event",
                };
                let message = event
                    .message
                    .clone()
                    .unwrap_or_else(|| "Run finished.".into());
                div()
                    .id(SharedString::from(format!("tl-{i}")))
                    .v_flex()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(cx.theme().colors.background)
                    .border_1()
                    .border_color(cx.theme().colors.border)
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight(600.0))
                                    .child(event.engine_id.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(status),
                            ),
                    )
                    .child(div().text_sm().child(message))
                    .into_any()
            })
            .collect();
        div().v_flex().gap_2().children(cards)
    }

    fn sidebar(&self, cx: &Context<Self>) -> Sidebar<SidebarMenu> {
        Sidebar::new("hub-sidebar")
            .header(SidebarHeader::new().child("ALVA Hub"))
            .child(Self::sidebar_menu(
                &self.sessions(),
                self.workspace.as_deref(),
                cx,
            ))
            .footer(
                SidebarFooter::new().child(
                    div()
                        .v_flex()
                        .gap_1()
                        .child("One project. Every engine.")
                        .child("Durable branches · reversible handoff"),
                ),
            )
    }

    /// Top bar: engine + permission selectors, ready pill.
    fn topbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_3()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(label_small("Engine", cx))
                    .child(Select::new(&self.engine).small()),
            )
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(label_small("Permissions", cx))
                    .child(Select::new(&self.permission).small()),
            )
            .child(div().flex_1())
            .child(
                Button::new("details")
                    .ghost()
                    .label("Details")
                    .on_click(|_, _, _| { /* P5: open sheet */ }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(cx.theme().colors.button)
                    .child(div().size_2().rounded_full().bg(cx.theme().colors.success))
                    .child(div().text_sm().child("Ready")),
            )
    }

    /// Main column: empty state + composer.
    fn main_column(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .size_full()
            .child(self.topbar(cx))
            // Timeline — batch 2: real run events, else the empty state.
            .child(
                div()
                    .flex_1()
                    .size_full()
                    .v_flex()
                    .px_4()
                    .py_3()
                    .gap_2()
                    .when(self.timeline.is_empty(), |this| {
                        this.v_flex()
                            .flex_1()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .child(
                                div()
                                    .size_10()
                                    .rounded_lg()
                                    .bg(cx.theme().colors.button)
                                    .v_flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        Icon::new(IconName::ArrowRight)
                                            .text_color(cx.theme().colors.muted_foreground),
                                    ),
                            )
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight(600.0))
                                    .child("Start one continuous project session"),
                            )
                            .child(
                                div()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(format!(
                                        "{} persisted sessions · Hub keeps each engine branch in private local storage.",
                                        self.sessions().len()
                                    )),
                            )
                    })
                    .child(self.timeline_cards(cx))
            )
            // Composer
            .child(
                v_flex()
                    .gap_2()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(cx.theme().colors.border)
                    .child(Input::new(&self.prompt))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Button::new("run")
                                    .primary()
                                    .label("Run with alva")
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.run(window, cx);
                                    })),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child("fallback: alva → claude → codex"),
                            ),
                    ),
            )
    }
}

fn label_small(text: impl Into<SharedString>, cx: &Context<HubApp>) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(cx.theme().colors.muted_foreground)
        .child(text.into())
}

impl Render for HubApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .bg(cx.theme().colors.background)
            .child(self.sidebar(cx))
            .child(self.main_column(cx))
    }
}

fn main() {
    gpui_platform::application()
        .with_assets(gpui_component_assets::Assets)
        .run(move |cx| {
            gpui_component::init(cx);

            cx.spawn(async move |cx| {
                cx.open_window(WindowOptions::default(), |window, cx| {
                    let view = cx.new(|cx| HubApp::new(window, cx));
                    cx.new(|cx| Root::new(view, window, cx).bg(cx.theme().colors.background))
                })
                .expect("Failed to open window");
            })
            .detach();
        });
}
