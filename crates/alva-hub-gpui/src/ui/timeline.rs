// INPUT:  HubApp.timeline (projected events), store, permission router
// OUTPUT: main column — topbar, timeline cards, composer with model/provider
//         line and run button.
// POS:    1:1 GPUI port of the React timeline + composer in App.tsx.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::Input,
    scroll::ScrollableElement,
    ActiveTheme, Icon, IconName, *,
};

use alva_hub_gpui::{EngineTarget, HandoffKind, TimelineStatus};

use super::app::{status_color, HubApp};
use super::{FallbackControls, ProjectedEvent};

impl HubApp {
    pub fn main_column(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .size_full()
            .min_w_0()
            .child(self.topbar(window, cx))
            .child(self.timeline_area(cx))
            .child(self.composer(window, cx))
    }

    fn timeline_area(&self, cx: &Context<Self>) -> impl IntoElement {
        let empty = self.timeline.is_empty();
        let session_count = self.sessions.len();
        div()
            .flex_1()
            .size_full()
            .v_flex()
            .overflow_y_scrollbar()
            .px_4()
            .py_3()
            .gap_2()
            .child(if empty {
                self.empty_state(session_count, cx).into_any_element()
            } else {
                self.timeline_cards(cx).into_any_element()
            })
    }

    fn empty_state(&self, session_count: usize, cx: &Context<Self>) -> impl IntoElement {
        v_flex()
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
                    .font_weight(FontWeight(600.))
                    .child("Start one continuous project session"),
            )
            .child(
                div()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(format!(
                        "{session_count} persisted sessions · Hub keeps each engine branch in private local storage."
                    )),
            )
    }

    fn timeline_cards(&self, cx: &Context<Self>) -> impl IntoElement {
        let cards: Vec<gpui::AnyElement> = self
            .timeline
            .iter()
            .enumerate()
            .map(|(i, event)| self.timeline_card(i, event, cx).into_any_element())
            .collect();
        div().v_flex().gap_2().children(cards)
    }

    fn timeline_card(
        &self,
        index: usize,
        event: &ProjectedEvent,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let (status, color) = status_color(&event.status, cx);
        let message = event.message.clone().unwrap_or_else(|| {
            if event.status == TimelineStatus::Running {
                "Executing in the selected workspace…".to_string()
            } else {
                "Run finished.".to_string()
            }
        });
        let mut card = div()
            .id(SharedString::from(format!("tl-{index}")))
            .v_flex()
            .gap_1()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(cx.theme().colors.background)
            .border_1()
            .border_color(color)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight(600.))
                            .child(event.engine_id.clone()),
                    )
                    .child(
                        div()
                            .px_1()
                            .py_1()
                            .rounded_full()
                            .text_xs()
                            .text_color(color)
                            .child(status),
                    ),
            )
            .child(div().text_sm().child(message));
        if event.status == TimelineStatus::Interrupted {
            card = card.child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(
                        "The previous runtime is gone. Continue safely from the composer below.",
                    ),
            );
        }
        if let Some(request) = &event.permission_request {
            if let Some(resource) = &request.resource {
                card = card.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().colors.muted_foreground)
                        .child(format!("Resource: {resource}")),
                );
            }
            card = card.child(self.permission_card(event, request, cx));
        }
        if let Some(fallback) = &event.fallback {
            card = card.child(self.fallback_controls(index, event, fallback, cx));
        }
        card
    }

    fn permission_card(
        &self,
        event: &ProjectedEvent,
        request: &alva_hub_gpui::PermissionRequestInfo,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let pid = event.product_session_id.clone();
        let weak = cx.entity().downgrade();
        let weak2 = cx.entity().downgrade();
        let description = request.description.clone();
        let request_id = request.request_id.clone();
        let request_id2 = request.request_id.clone();
        let pid2 = pid.clone();
        let mut card = v_flex()
            .gap_1()
            .px_2()
            .py_2()
            .rounded_md()
            .bg(cx.theme().colors.button)
            .child(if let Some(description) = description {
                div().text_xs().child(description).into_any_element()
            } else {
                div().into_any_element()
            });
        card = card.child(
            h_flex()
                .gap_2()
                .child(
                    Button::new(SharedString::from(format!("allow-{request_id}")))
                        .small()
                        .label("Allow")
                        .on_click(move |_, _, app| {
                            if let Some(entity) = weak.upgrade() {
                                entity.update(app, |hub, cx| {
                                    hub.answer_permission(&pid, &request_id, true, cx);
                                });
                            }
                        }),
                )
                .child(
                    Button::new(SharedString::from(format!("deny-{request_id2}")))
                        .small()
                        .label("Deny")
                        .on_click(move |_, _, app| {
                            if let Some(entity) = weak2.upgrade() {
                                entity.update(app, |hub, cx| {
                                    hub.answer_permission(&pid2, &request_id2, false, cx);
                                });
                            }
                        }),
                ),
        );
        card
    }

    fn fallback_controls(
        &self,
        index: usize,
        event: &ProjectedEvent,
        fallback: &FallbackControls,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let handoff_open = self.open_handoffs.get(&index).copied().unwrap_or(false);
        let handoff = fallback.handoff.clone();
        let weak = cx.entity().downgrade();
        let weak2 = cx.entity().downgrade();
        let weak3 = cx.entity().downgrade();
        let source = fallback.source_engine_id.clone();
        let source2 = fallback.source_engine_id.clone();
        let pid = event.product_session_id.clone();
        let pid2 = event.product_session_id.clone();
        let attempt_is_live = self.busy || self.session_status_is_running();
        let engine = self.selected_engine(cx);
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(SharedString::from(format!("stop-{index}")))
                            .small()
                            .label("Stop")
                            .disabled(!attempt_is_live)
                            .on_click(move |_, _, app| {
                                if let Some(entity) = weak.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.stop_attempt(&pid, cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("switchback-{index}")))
                            .small()
                            .label(format!("Switch back to {source}"))
                            .disabled(engine == source)
                            .on_click(move |_, window, app| {
                                if weak2.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let _ = handle.update(app, |root, window, cx| {
                                        if let Ok(view) = root.downcast::<HubApp>() {
                                            view.update(cx, |hub, cx| {
                                                hub.select_engine(&source2, window, cx);
                                            });
                                        }
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new(SharedString::from(format!("handoff-{index}")))
                            .ghost()
                            .small()
                            .label(if handoff_open {
                                "Hide handoff"
                            } else {
                                "View handoff"
                            })
                            .disabled(handoff.is_none())
                            .on_click(move |_, window, app| {
                                if weak3.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let _ = handle.update(app, |root, window, cx| {
                                        if let Ok(view) = root.downcast::<HubApp>() {
                                            view.update(cx, |hub, cx| {
                                                hub.toggle_handoff(index, &pid2, window, cx);
                                            });
                                        }
                                    });
                                }
                            }),
                    ),
            )
            .when(handoff_open, |this| this.child(handoff_view(&handoff, cx)))
    }

    /// Is the open session's store status "running" (survives restart)?
    fn session_status_is_running(&self) -> bool {
        self.session_detail
            .as_ref()
            .map(|detail| detail.session.status == "running")
            .unwrap_or(false)
    }

    fn composer(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_id = self
            .pending_engine
            .clone()
            .unwrap_or_else(|| self.selected_engine(cx));
        let selected_id = selected_id.as_str();
        let target = self
            .fallback_policy
            .targets
            .iter()
            .find(|target| target.engine_id == selected_id)
            .cloned()
            .or_else(|| EngineTarget::for_engine(selected_id).ok());
        let summary_line = target
            .as_ref()
            .map(|target| {
                format!(
                    "Running {selected_id} on {} · {}",
                    target.model_id, target.provider_id
                )
            })
            .unwrap_or_default();
        let busy = self.busy;
        let workspace_set = self.workspace.is_some();
        let engine_display = self.selected_engine(cx);
        let model_id = self
            .session_detail
            .as_ref()
            .map(|detail| detail.session.model_id.clone())
            .unwrap_or_default();
        let profile_label = super::app::profile_label_of(self.selected_profile(cx).as_str());
        let fallback_summary = if self.fallback_policy.targets.is_empty() {
            "none".to_string()
        } else {
            self.fallback_policy
                .targets
                .iter()
                .map(|target| {
                    if target.automatic_use_authorized {
                        target.engine_id.clone()
                    } else {
                        format!("{} (unauthorized)", target.engine_id)
                    }
                })
                .collect::<Vec<_>>()
                .join(" → ")
        };
        let weak = cx.entity().downgrade();
        v_flex()
            .gap_2()
            .px_4()
            .py_3()
            .border_t_1()
            .border_color(cx.theme().colors.border)
            .child(Input::new(&self.prompt))
            .when(!summary_line.is_empty(), |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().colors.muted_foreground)
                        .child(summary_line),
                )
            })
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(format!(
                                "{}{}{} · {} · fallback: {}",
                                if workspace_set {
                                    ""
                                } else {
                                    "Select a workspace first · "
                                },
                                engine_display,
                                if model_id.is_empty() {
                                    String::new()
                                } else {
                                    format!(" · {model_id}")
                                },
                                profile_label,
                                fallback_summary
                            )),
                    )
                    .child(
                        Button::new("run")
                            .primary()
                            .label(if busy {
                                "Running…".to_string()
                            } else {
                                format!("Run with {engine_display}")
                            })
                            .disabled(busy || !workspace_set)
                            .on_click(move |_, window, app| {
                                if weak.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let _ = handle.update(app, move |root, window, cx| {
                                        if let Ok(view) = root.downcast::<HubApp>() {
                                            view.update(cx, |hub, cx| {
                                                hub.run(window, cx);
                                            });
                                        }
                                    });
                                }
                            }),
                    ),
            )
    }
}

/// Full handoff envelope: kind, delivered/pending, reason, goal, work,
/// files, summary, skills, MCP and filtered transcript.
fn handoff_view(
    handoff: &Option<alva_hub_gpui::HandoffPackage>,
    cx: &Context<HubApp>,
) -> impl IntoElement {
    let Some(handoff) = handoff else {
        return div().into_any_element();
    };
    let kind = match handoff.kind {
        HandoffKind::Full => "full",
        HandoffKind::Delta => "delta",
    };
    let delivered = if handoff.used_at.is_some() {
        "delivered"
    } else {
        "pending"
    };
    let mut view = div()
        .id(SharedString::from(format!(
            "handoff-{}",
            handoff.handoff_id
        )))
        .v_flex()
        .gap_1()
        .mt_1()
        .px_2()
        .py_2()
        .rounded_md()
        .bg(cx.theme().colors.button)
        .child(div().text_xs().font_weight(FontWeight(600.)).child(format!(
            "{kind} handoff · {} → {} · {delivered}",
            handoff.source.engine_id, handoff.target.engine_id
        )))
        .child(div().text_xs().child(format!("Reason: {}", handoff.reason)));
    if let Some(goal) = &handoff.user_goal {
        view = view.child(div().text_xs().child(format!("Goal: {goal}")));
    }
    if let Some(instruction) = &handoff.latest_instruction {
        view = view.child(
            div()
                .text_xs()
                .child(format!("Latest instruction: {instruction}")),
        );
    }
    for work in &handoff.completed_work {
        view = view.child(
            div()
                .text_xs()
                .child(format!("✓ {} — {}", work.prompt, work.result)),
        );
    }
    for work in &handoff.pending_work {
        view = view.child(
            div()
                .text_xs()
                .child(format!("⏳ {} — {}", work.prompt, work.reason)),
        );
    }
    if !handoff.changed_files.is_empty() {
        view = view.child(
            div()
                .text_xs()
                .child(format!("Files: {}", handoff.changed_files.join(", "))),
        );
    }
    if !handoff.workspace_summary.is_empty() {
        view = view.child(div().text_xs().child(handoff.workspace_summary.clone()));
    }
    if !handoff.available_skills.is_empty() {
        let mut skills = div().v_flex().gap_1().child(
            div()
                .text_xs()
                .font_weight(FontWeight(600.))
                .child("Workspace skills"),
        );
        for skill in &handoff.available_skills {
            skills = skills.child(div().text_xs().child(format!(
                "{} — {} {}",
                skill.name, skill.description, skill.path
            )));
        }
        view = view.child(skills);
    }
    if !handoff.available_mcp.is_empty() {
        let mut mcp = div().v_flex().gap_1().child(
            div()
                .text_xs()
                .font_weight(FontWeight(600.))
                .child("MCP servers"),
        );
        for server in &handoff.available_mcp {
            mcp = mcp.child(
                div()
                    .text_xs()
                    .child(format!("{} {}", server.display_name, server.transport)),
            );
        }
        view = view.child(mcp);
    }
    if !handoff.transcript.is_empty() {
        let mut transcript =
            div()
                .v_flex()
                .gap_1()
                .child(div().text_xs().font_weight(FontWeight(600.)).child(format!(
                    "Filtered transcript ({} turns)",
                    handoff.transcript.len()
                )));
        for turn in &handoff.transcript {
            let line = match &turn.result {
                Some(result) => format!("» {} — {result}", turn.prompt),
                None => format!("» {}", turn.prompt),
            };
            transcript = transcript.child(div().text_xs().child(line));
        }
        view = view.child(transcript);
    } else {
        view = view.child(
            div()
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child("Transcript not shared (default)."),
        );
    }
    view.into_any_element()
}
