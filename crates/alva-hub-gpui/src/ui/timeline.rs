// INPUT:  HubApp.timeline (projected events), store, permission router
// OUTPUT: main column — topbar, timeline cards, composer with model/provider
//         line and run button.
// POS:    1:1 GPUI port of the React timeline + composer in App.tsx.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    alert::Alert,
    avatar::Avatar,
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    dialog::DialogButtonProps,
    group_box::GroupBoxVariant,
    input::Input,
    menu::{DropdownMenu as _, PopupMenuItem},
    scroll::ScrollableElement,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings},
    spinner::Spinner,
    tab::{Tab, TabBar},
    tag::{Tag, TagVariant},
    ActiveTheme, *,
};

use alva_hub_gpui::{
    AgentRuntime, AutomationRunStatus, ChatItemKind, FallbackControls, HandoffKind,
    ProjectedChatItem, ProjectedEvent, TimelineStatus,
};

use super::app::{status_color, with_window, HubApp};
use super::components::{
    HubActivityRow, HubAutomationForm, HubComposer, HubEmptyState, HubExtensionCard,
    HubExtensionDetail, HubModelPicker, HubProjectPicker, HubProviderForm, HubQuickStartCard,
    HubSurfaceHeader, HubTemplateCard,
};

impl HubApp {
    pub fn main_column(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .flex_1()
            .size_full()
            .min_w_0()
            .bg(cx.theme().colors.tiles)
            .child(self.topbar(window, cx))
            .child(if self.chat.is_empty() {
                self.new_conversation(window, cx).into_any_element()
            } else {
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.chat_area(cx))
                    .child(self.approval_area(cx))
                    .child(self.composer(window, cx))
                    .child(self.composer_readiness_warning(cx))
                    .into_any_element()
            })
    }

    fn new_conversation(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let session_count = self.sessions.len();
        v_flex()
            .flex_1()
            .min_h_0()
            .overflow_y_scrollbar()
            .px_5()
            .py_5()
            .child(
                v_flex()
                    .flex_1()
                    .w_full()
                    .max_w(px(820.))
                    .mx_auto()
                    .justify_center()
                    .gap_4()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_3xl()
                                            .font_weight(FontWeight(800.))
                                            .child("ALVA"),
                                    )
                                    .child(Tag::secondary().small().rounded_full().child("HUB")),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(format!(
                                        "One conversation. Every engine. · {session_count} stored locally"
                                    )),
                            ),
                    )
                    .child(self.approval_area(cx))
                    .child(self.composer(window, cx))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight(600.))
                            .text_color(cx.theme().colors.muted_foreground)
                            .child("Quick start"),
                    )
                    .child(self.quick_start_cards(cx)),
            )
    }

    fn quick_start_cards(&self, cx: &Context<Self>) -> impl IntoElement {
        let cards = [
            (
                "quick-explore",
                IconName::Search,
                "Explore this project",
                "Understand the codebase and its architecture.",
                "Explore this project and explain its architecture.",
            ),
            (
                "quick-build",
                IconName::Bot,
                "Build a feature",
                "Plan and implement a focused product change.",
                "Help me plan and build a new feature.",
            ),
            (
                "quick-review",
                IconName::Github,
                "Review changes",
                "Inspect the current diff and suggest improvements.",
                "Review the current changes and identify problems.",
            ),
            (
                "quick-fix",
                IconName::Info,
                "Fix a failure",
                "Diagnose an error and verify the correction.",
                "Diagnose and fix the current failure.",
            ),
        ];
        h_flex().w_full().flex_wrap().gap_3().children(cards.map(
            |(id, icon, title, description, prompt)| {
                let weak = cx.entity().downgrade();
                HubQuickStartCard::new(id, icon, title, description)
                    .on_click(move |_, window, app| {
                        if let Some(entity) = weak.upgrade() {
                            entity.update(app, |hub, cx| {
                                hub.prompt.update(cx, |input, cx| {
                                    input.set_value(prompt, window, cx);
                                });
                            });
                        }
                    })
                    .into_any_element()
            },
        ))
    }

    fn chat_area(&self, cx: &Context<Self>) -> impl IntoElement {
        let empty = self.chat.is_empty();
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
                self.chat_rows(cx).into_any_element()
            })
    }

    fn empty_state(&self, _: usize, _: &Context<Self>) -> impl IntoElement {
        HubEmptyState::new(
            IconName::Bot,
            "No messages yet",
            "Start with the composer below.",
        )
    }

    fn chat_rows(&self, cx: &Context<Self>) -> impl IntoElement {
        let cards: Vec<gpui::AnyElement> = self
            .chat
            .iter()
            .enumerate()
            .map(|(index, item)| self.chat_row(index, item, cx).into_any_element())
            .collect();
        v_flex()
            .w_full()
            .max_w(px(860.))
            .mx_auto()
            .gap_4()
            .py_3()
            .children(cards)
    }

    fn chat_row(
        &self,
        index: usize,
        item: &ProjectedChatItem,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        match item.kind {
            ChatItemKind::User => h_flex()
                .w_full()
                .justify_end()
                .child(
                    div()
                        .id(SharedString::from(format!("chat-user-{index}")))
                        .max_w(relative(0.78))
                        .px_4()
                        .py_3()
                        .rounded_xl()
                        .bg(cx.theme().colors.accent)
                        .text_color(cx.theme().colors.accent_foreground)
                        .child(item.message.clone()),
                )
                .into_any_element(),
            ChatItemKind::Assistant => {
                let status = item.status.as_ref().unwrap_or(&TimelineStatus::Completed);
                let (status_label, _) = status_color(status, cx);
                h_flex()
                    .w_full()
                    .items_start()
                    .gap_3()
                    .child(Avatar::new().name(item.engine_id.clone()))
                    .child(
                        v_flex()
                            .min_w_0()
                            .flex_1()
                            .gap_2()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().text_sm().font_weight(FontWeight(600.)).child(
                                        if item.engine_id.is_empty() {
                                            "Assistant".to_string()
                                        } else {
                                            item.engine_id.clone()
                                        },
                                    ))
                                    .child(
                                        Tag::new()
                                            .with_variant(super::timeline_tag_variant(status))
                                            .small()
                                            .rounded_full()
                                            .child(status_label),
                                    ),
                            )
                            .child(div().text_sm().child(item.message.clone())),
                    )
                    .into_any_element()
            }
            ChatItemKind::Activity => {
                let event = item
                    .timeline_index
                    .and_then(|timeline_index| self.timeline.get(timeline_index));
                event
                    .map(|event| self.timeline_card(index, event, cx).into_any_element())
                    .unwrap_or_else(|| {
                        div()
                            .text_sm()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(item.message.clone())
                            .into_any_element()
                    })
            }
        }
    }

    fn timeline_card(
        &self,
        index: usize,
        event: &ProjectedEvent,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let (status, _) = status_color(&event.status, cx);
        let message = event.message.clone().unwrap_or_else(|| {
            if event.status == TimelineStatus::Running {
                "Executing in the selected workspace…".to_string()
            } else {
                "Run finished.".to_string()
            }
        });
        let expanded = self.open_activities.contains(&index)
            || matches!(
                event.status,
                TimelineStatus::PermissionRequired | TimelineStatus::Fallback
            );
        let weak = cx.entity().downgrade();
        let summary = message.lines().next().unwrap_or("Activity").to_string();
        let mut card = HubActivityRow::new(
            event.engine_id.clone(),
            status,
            super::timeline_tag_variant(&event.status),
            summary,
        )
        .open(expanded)
        .trigger(
            Button::new(SharedString::from(format!("activity-toggle-{index}")))
                .ghost()
                .xsmall()
                .icon(if expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .tooltip(if expanded {
                    "Collapse activity"
                } else {
                    "Expand activity"
                })
                .on_click(move |_, _, app| {
                    if let Some(entity) = weak.upgrade() {
                        entity.update(app, |hub, cx| {
                            if !hub.open_activities.remove(&index) {
                                hub.open_activities.insert(index);
                            }
                            cx.notify();
                        });
                    }
                }),
        )
        .content(div().text_sm().child(message));
        if expanded && event.status == TimelineStatus::Interrupted {
            card = card.content(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(
                        "The previous runtime is gone. Continue safely from the composer below.",
                    ),
            );
        }
        if expanded {
            if let Some(request) = &event.permission_request {
                if let Some(resource) = &request.resource {
                    card = card.content(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(format!("Resource: {resource}")),
                    );
                }
                card = card.content(self.permission_card(event, request, cx));
            }
            if let Some(fallback) = &event.fallback {
                card = card.content(self.fallback_controls(index, event, fallback, cx));
            }
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
                                    let source2 = source2.clone();
                                    with_window(app, handle, move |hub, window, cx| {
                                        hub.select_engine(&source2, window, cx);
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
                                    let pid2 = pid2.clone();
                                    with_window(app, handle, move |hub, window, cx| {
                                        hub.toggle_handoff(index, &pid2, window, cx);
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

    /// Warning above the composer when the selected engine is not healthy.
    fn composer_readiness_warning(&self, cx: &Context<Self>) -> AnyElement {
        let selected_engine = self.selected_engine(cx);
        let selected = self
            .pending_engine
            .as_deref()
            .unwrap_or(selected_engine.as_str());
        let Some(health) = self
            .readiness
            .iter()
            .find(|report| report.engine_id == selected)
        else {
            return div().into_any_element();
        };
        if health.status == alva_hub_gpui::ReadinessStatus::Healthy {
            return div().into_any_element();
        }
        let reason = health
            .reasons
            .iter()
            .map(super::describe_readiness_reason)
            .next()
            .unwrap_or_else(|| "Open engine readiness for details.".to_string());
        let status = super::readiness_badge(match health.status {
            alva_hub_gpui::ReadinessStatus::Healthy => "healthy",
            alva_hub_gpui::ReadinessStatus::Missing => "missing",
            alva_hub_gpui::ReadinessStatus::Incompatible => "incompatible",
            alva_hub_gpui::ReadinessStatus::Unauthenticated => "unauthenticated",
            alva_hub_gpui::ReadinessStatus::Unhealthy => "unhealthy",
        });
        let message = format!("{} · {status} — {reason}", health.display_name);
        let alert = match health.status {
            alva_hub_gpui::ReadinessStatus::Unhealthy => {
                Alert::error("composer-readiness", message)
            }
            _ => Alert::warning("composer-readiness", message),
        };
        alert.xsmall().mx_4().into_any_element()
    }

    fn composer(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy;
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
        let weak_stop = cx.entity().downgrade();
        let stop_session = self.active_session_id.clone();
        let workspace_label = self
            .workspace
            .as_deref()
            .and_then(|workspace| workspace.split('/').next_back().map(ToString::to_string));
        let project_action = HubProjectPicker::new(cx.entity().downgrade(), workspace_label);
        let submit_action = if busy {
            Button::new("stop")
                .danger()
                .small()
                .icon(IconName::SquareTerminal)
                .tooltip("Stop the current attempt")
                .on_click(move |_, _, app| {
                    let Some(session_id) = stop_session.clone() else {
                        return;
                    };
                    if let Some(entity) = weak_stop.upgrade() {
                        entity.update(app, |hub, cx| {
                            hub.stop_attempt(&session_id, cx);
                        });
                    }
                })
                .into_any_element()
        } else {
            Button::new("run")
                .primary()
                .small()
                .icon(IconName::ArrowRight)
                .tooltip("Send message")
                .on_click(move |_, window, app| {
                    if weak.upgrade().is_some() {
                        let handle = window.window_handle();
                        with_window(app, handle, |hub, window, cx| {
                            hub.run(window, cx);
                        });
                    }
                })
                .into_any_element()
        };
        let metadata = format!(
            "{}{}{} · {} · fallback: {}",
            if self.workspace.is_some() {
                ""
            } else {
                "Pure conversation · "
            },
            engine_display,
            if model_id.is_empty() {
                String::new()
            } else {
                format!(" · {model_id}")
            },
            profile_label,
            fallback_summary
        );
        let runtime = AgentRuntime::parse(&engine_display).unwrap_or(AgentRuntime::Alva);
        let selected_route = self.selected_model_route(cx).ok();
        let model_picker = HubModelPicker::new(
            cx.entity().downgrade(),
            self.model_query.clone(),
            self.model_catalog.clone(),
            runtime,
            selected_route,
            self.model_query.read(cx).value().to_string(),
        );
        HubComposer::new(
            self.prompt.clone(),
            self.permission.clone(),
            model_picker,
            metadata,
        )
        .permission_disabled(self.active_session_id.is_some())
        .metadata_visible(self.active_session_id.is_some())
        .inset(self.active_session_id.is_some())
        .project_action(project_action)
        .submit_action(submit_action)
    }

    pub fn automation_surface(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let weak_empty = cx.entity().downgrade();
        let weak_nightly = cx.entity().downgrade();
        let weak_pr = cx.entity().downgrade();
        let weak_research = cx.entity().downgrade();
        let weak_docs = cx.entity().downgrade();
        let automation_count = self.automations.len();
        v_flex()
            .size_full()
            .child(HubSurfaceHeader::new(
                "Automation",
                format!(
                    "{automation_count} automation{} · runs while Hub is open",
                    if automation_count == 1 { "" } else { "s" }
                ),
            ))
            .child(
                v_flex()
                    .flex_1()
                    .overflow_y_scrollbar()
                    .px_6()
                    .py_6()
                    .gap_5()
                    .when(self.automations.is_empty(), |this| this.child(
                        HubEmptyState::new(
                            IconName::Info,
                            "No automations yet",
                            "Create a scheduled prompt for Alva, Claude, or Codex.",
                        )
                        .action(
                            Button::new("new-automation")
                                .primary()
                                .icon(IconName::Plus)
                                .label("New automation")
                                .on_click(move |_, window, app| {
                                    if let Some(entity) = weak_empty.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.open_automation_dialog(None, window, cx);
                                        });
                                    }
                                }),
                        ),
                    ))
                    .children(self.automations.iter().map(|automation| {
                        let enabled_id = automation.id.clone();
                        let run_id = automation.id.clone();
                        let delete_id = automation.id.clone();
                        let automation_name = automation.name.clone();
                        let weak_enabled = cx.entity().downgrade();
                        let weak_run = cx.entity().downgrade();
                        let weak_delete = cx.entity().downgrade();
                        let running = automation.last_status == AutomationRunStatus::Running;
                        let status = match automation.last_status {
                            AutomationRunStatus::Idle => "Scheduled",
                            AutomationRunStatus::Running => "Running",
                            AutomationRunStatus::Succeeded => "Succeeded",
                            AutomationRunStatus::Failed => "Failed",
                        };
                        let description = format!(
                            "{} · {} / {} · next {}",
                            automation.schedule.label(),
                            automation.target.engine_id,
                            automation.target.model_id,
                            super::format_future_time(automation.next_run_at_ms, chrono::Utc::now().timestamp_millis())
                        );
                        let action = h_flex()
                            .items_center()
                            .gap_1()
                            .child(
                                Checkbox::new(SharedString::from(format!("automation-enabled-{enabled_id}")))
                                    .checked(automation.enabled)
                                    .disabled(running)
                                    .on_click(move |enabled, window, app| {
                                        if let Some(hub) = weak_enabled.upgrade() {
                                            let result = hub.update(app, |hub, cx| {
                                                hub.set_automation_enabled(&enabled_id, *enabled, cx)
                                            });
                                            match result {
                                                Ok(()) => window.push_notification(
                                                    if *enabled {
                                                        "Automation enabled"
                                                    } else {
                                                        "Automation paused"
                                                    },
                                                    app,
                                                ),
                                                Err(error) => window.push_notification(error, app),
                                            }
                                        }
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(format!("run-automation-{run_id}")))
                                    .secondary()
                                    .xsmall()
                                    .label("Run now")
                                    .loading(running)
                                    .disabled(running)
                                    .on_click(move |_, window, app| {
                                        if let Some(hub) = weak_run.upgrade() {
                                            let result = hub.update(app, |hub, cx| {
                                                hub.run_automation_now(&run_id, cx)
                                            });
                                            match result {
                                                Ok(()) => window.push_notification("Automation started", app),
                                                Err(error) => window.push_notification(error, app),
                                            }
                                        }
                                    }),
                            )
                            .child(
                                Button::new(SharedString::from(format!("delete-automation-{delete_id}")))
                                    .danger()
                                    .xsmall()
                                    .icon(IconName::Close)
                                    .tooltip("Delete automation")
                                    .disabled(running)
                                    .on_click(move |_, window, app| {
                                        if let Some(hub) = weak_delete.upgrade() {
                                            let result = hub.update(app, |hub, cx| {
                                                hub.delete_automation(&delete_id, cx)
                                            });
                                            match result {
                                                Ok(()) => window.push_notification("Automation deleted", app),
                                                Err(error) => window.push_notification(error, app),
                                            }
                                        }
                                    }),
                            );
                        HubExtensionCard::new(
                            IconName::Calendar,
                            automation_name,
                            description,
                            status,
                        )
                        .action(action)
                    }))
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight(650.))
                            .child("Start from a template"),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_3()
                            .child(
                                HubTemplateCard::new(
                                    IconName::Asterisk,
                                    "Nightly self-healing tests",
                                    "Run tests in an isolated worktree and repair failures.",
                                    "02:00 · isolated worktree",
                                )
                                .on_click(move |_, window, app| {
                                    if let Some(entity) = weak_nightly.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.open_automation_dialog(
                                                Some((
                                                    "Nightly self-healing tests",
                                                    "Run the test suite in an isolated worktree and repair failures with the smallest safe change.",
                                                )),
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                }),
                            )
                            .child(
                                HubTemplateCard::new(
                                    IconName::Github,
                                    "PR gatekeeper",
                                    "Review new changes and report release risk.",
                                    "Weekdays · pull requests",
                                )
                                .on_click(move |_, window, app| {
                                    if let Some(entity) = weak_pr.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.open_automation_dialog(
                                                Some((
                                                    "PR gatekeeper",
                                                    "Review new pull request changes and report correctness, security, and release risks.",
                                                )),
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                }),
                            ),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_3()
                            .child(
                                HubTemplateCard::new(
                                    IconName::Search,
                                    "Research radar",
                                    "Collect important updates and produce a concise digest.",
                                    "Daily · web research",
                                )
                                .on_click(move |_, window, app| {
                                    if let Some(entity) = weak_research.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.open_automation_dialog(
                                                Some((
                                                    "Research radar",
                                                    "Collect important updates for this project and produce a concise daily digest.",
                                                )),
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                }),
                            )
                            .child(
                                HubTemplateCard::new(
                                    IconName::File,
                                    "Documentation pulse",
                                    "Find stale docs and propose focused improvements.",
                                    "Weekly · project context",
                                )
                                .on_click(move |_, window, app| {
                                    if let Some(entity) = weak_docs.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.open_automation_dialog(
                                                Some((
                                                    "Documentation pulse",
                                                    "Find stale project documentation and propose focused improvements.",
                                                )),
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                }),
                            ),
                    ),
            )
    }

    fn open_automation_dialog(
        &self,
        template: Option<(&'static str, &'static str)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (name, prompt) = template.unwrap_or(("", ""));
        self.automation_name
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.automation_prompt
            .update(cx, |input, cx| input.set_value(prompt, window, cx));
        let weak = cx.entity().downgrade();
        let form_name = self.automation_name.clone();
        let form_prompt = self.automation_prompt.clone();
        let form_schedule = self.automation_schedule.clone();
        window.open_dialog(cx, move |dialog, _, _| {
            let weak = weak.clone();
            dialog
                .width(px(560.))
                .title("New automation")
                .child(HubAutomationForm::new(
                    form_name.clone(),
                    form_prompt.clone(),
                    form_schedule.clone(),
                ))
                .button_props(
                    DialogButtonProps::default()
                        .show_cancel(true)
                        .ok_text("Create automation")
                        .on_ok(move |_, window, app| {
                            let Some(hub) = weak.upgrade() else {
                                return false;
                            };
                            match hub.update(app, |hub, cx| hub.create_automation_from_form(cx)) {
                                Ok(name) => {
                                    window.push_notification(
                                        format!("Automation scheduled: {name}"),
                                        app,
                                    );
                                    true
                                }
                                Err(error) => {
                                    window.push_notification(error, app);
                                    false
                                }
                            }
                        }),
                )
        });
    }

    pub(crate) fn open_provider_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.reset_provider_form(window, cx);
        let weak = cx.entity().downgrade();
        let form = HubProviderForm::new(
            weak.clone(),
            self.provider_name.clone(),
            self.provider_id.clone(),
            self.provider_base_url.clone(),
            self.provider_api_key.clone(),
            self.provider_model_id.clone(),
            self.provider_model_name.clone(),
            self.provider_context_window.clone(),
            self.provider_max_output.clone(),
            self.provider_protocol.clone(),
            self.provider_runtimes.clone(),
            self.provider_images,
            self.provider_tools,
        );
        window.open_dialog(cx, move |dialog, _, _| {
            let weak = weak.clone();
            dialog
                .width(px(620.))
                .title("Add model provider")
                .child(form.clone())
                .button_props(
                    DialogButtonProps::default()
                        .show_cancel(true)
                        .ok_text("Save provider")
                        .on_ok(move |_, window, app| {
                            let Some(hub) = weak.upgrade() else {
                                return false;
                            };
                            match hub.update(app, |hub, cx| hub.save_provider_form(window, cx)) {
                                Ok(name) => {
                                    window.push_notification(
                                        format!("Model provider saved: {name}"),
                                        app,
                                    );
                                    true
                                }
                                Err(error) => {
                                    window.push_notification(error, app);
                                    false
                                }
                            }
                        }),
                )
        });
    }

    pub fn plugins_surface(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.extension_tab == 1 {
            self.skills_catalog_surface(cx).into_any_element()
        } else {
            self.plugins_catalog_surface(cx).into_any_element()
        }
    }

    fn plugins_catalog_surface(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let show_inspector = self.extension_matches(cx, "alva inspector debug records recursive");
        let show_mcp = self.extension_matches(cx, "mcp tools model context protocol");
        let show_alva = self.extension_matches(cx, "alva engine native");
        let show_claude = self.extension_matches(cx, "claude code adapter");
        let show_codex = self.extension_matches(cx, "codex exec json adapter");
        let query = self.extension_query.read(cx).value().trim().to_lowercase();
        v_flex()
            .size_full()
            .child(self.extension_catalog_toolbar(cx))
            .child(
                v_flex()
                    .flex_1()
                    .overflow_y_scrollbar()
                    .w_full()
                    .max_w(px(980.))
                    .mx_auto()
                    .px_6()
                    .py_5()
                    .gap_5()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight(700.))
                                    .child("Plugins"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Tools and integrations available to Hub sessions."),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight(650.))
                            .child("Installed"),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_3()
                            .when(show_inspector, |row| row.child(
                                HubExtensionCard::new(
                                    IconName::Search,
                                    "ALVA Inspector",
                                    "Durable execution records and recursive agent inspection.",
                                    "Built in",
                                )
                                .action({
                                    let weak = cx.entity().downgrade();
                                    Button::new("open-inspector-plugin")
                                        .ghost()
                                        .xsmall()
                                        .label("Open")
                                        .on_click(move |_, window, app| {
                                            if let Some(entity) = weak.upgrade() {
                                                entity.update(app, |hub, cx| {
                                                    hub.open_workspace_tool(
                                                        super::workspace::WorkspaceTool::Inspector,
                                                        window,
                                                        cx,
                                                    );
                                                });
                                            }
                                        })
                                }),
                            ))
                            .when(show_mcp, |row| row.child(
                                HubExtensionCard::new(
                                    IconName::Frame,
                                    "MCP tools",
                                    "Connect external tools through the Model Context Protocol.",
                                    "Built in",
                                )
                                .action(self.extension_detail_button(
                                    "open-mcp-plugin",
                                    "Configure",
                                    IconName::Frame,
                                    "MCP tools",
                                    "Connect and configure external Model Context Protocol servers.",
                                    "Built in",
                                    &["Tool discovery", "Server lifecycle", "Permission routing"],
                                    cx,
                                )),
                            )),
                    )
                    .child(
                        h_flex().flex_wrap().gap_3().children(
                            self.plugins
                                .iter()
                                .filter(move |plugin| {
                                    query.is_empty()
                                        || format!(
                                            "{} {} {} {}",
                                            plugin.manifest.name,
                                            plugin
                                                .manifest
                                                .description
                                                .as_deref()
                                                .unwrap_or_default(),
                                            plugin.scope.label(),
                                            plugin.runtime_label(),
                                        )
                                        .to_lowercase()
                                        .contains(&query)
                                })
                                .map(|plugin| {
                                    let name = plugin.manifest.name.clone();
                                    let toggle_name = name.clone();
                                    let chat_name = name.clone();
                                    let weak_toggle = cx.entity().downgrade();
                                    let weak_chat = cx.entity().downgrade();
                                    let enabled = plugin.enabled;
                                    let description = plugin
                                        .manifest
                                        .description
                                        .clone()
                                        .unwrap_or_else(|| "ALVA subprocess plugin".into());
                                    let status = format!(
                                        "{} · {}{}",
                                        plugin.scope.label(),
                                        plugin.runtime_label(),
                                        if enabled { "" } else { " · Disabled" }
                                    );
                                    HubExtensionCard::new(
                                        IconName::Asterisk,
                                        name,
                                        description,
                                        status,
                                    )
                                    .action(
                                        h_flex()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                Checkbox::new(SharedString::from(format!(
                                                    "plugin-enabled-{toggle_name}"
                                                )))
                                                .checked(enabled)
                                                .on_click(move |enabled, window, app| {
                                                    if let Some(hub) = weak_toggle.upgrade() {
                                                        let result =
                                                            hub.update(app, |hub, cx| {
                                                                hub.set_plugin_enabled(
                                                                    &toggle_name,
                                                                    *enabled,
                                                                    cx,
                                                                )
                                                            });
                                                        match result {
                                                            Ok(()) => window.push_notification(
                                                                "Plugin setting saved; it applies to the next ALVA turn.",
                                                                app,
                                                            ),
                                                            Err(error) => {
                                                                window.push_notification(error, app)
                                                            }
                                                        }
                                                    }
                                                }),
                                            )
                                            .child(
                                                Button::new(SharedString::from(format!(
                                                    "chat-with-plugin-{chat_name}"
                                                )))
                                                .ghost()
                                                .xsmall()
                                                .label("Chat")
                                                .disabled(!enabled)
                                                .on_click(move |_, window, app| {
                                                    if let Some(hub) = weak_chat.upgrade() {
                                                        hub.update(app, |hub, cx| {
                                                            hub.chat_with_plugin(
                                                                &chat_name,
                                                                window,
                                                                cx,
                                                            )
                                                        });
                                                    }
                                                }),
                                            ),
                                    )
                                }),
                        ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight(650.))
                            .child("Available engines"),
                    )
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_3()
                            .when(show_alva, |row| row.child(
                                HubExtensionCard::new(
                                    IconName::Bot,
                                    "Alva",
                                    "Full native session, plugin, memory, and Inspector support.",
                                    "Connected",
                                )
                                .action(self.extension_detail_button(
                                    "open-alva-engine",
                                    "Details",
                                    IconName::Bot,
                                    "Alva",
                                    "The native ALVA engine with complete Hub session and Inspector support.",
                                    "Connected",
                                    &["Streaming", "Permissions", "Recursive Inspector"],
                                    cx,
                                )),
                            ))
                            .when(show_claude, |row| row.child(
                                HubExtensionCard::new(
                                    IconName::Bot,
                                    "Claude Code",
                                    "Use the installed Claude runtime through the Hub adapter.",
                                    "Adapter",
                                )
                                .action(self.extension_detail_button(
                                    "open-claude-engine",
                                    "Details",
                                    IconName::Bot,
                                    "Claude Code",
                                    "Claude Code adapter with capability-aware best-effort event projection.",
                                    "Adapter",
                                    &["Streaming", "Resume", "Best-effort inspection"],
                                    cx,
                                )),
                            )),
                    )
                    .child(
                        h_flex().flex_wrap().gap_3().when(show_codex, |row| row.child(
                            HubExtensionCard::new(
                                IconName::SquareTerminal,
                                "Codex",
                                "Use codex exec JSON events through the Hub adapter.",
                                "Adapter",
                            )
                            .action(self.extension_detail_button(
                                "open-codex-engine",
                                "Details",
                                IconName::SquareTerminal,
                                "Codex",
                                "Codex exec adapter with JSON event streaming and Hub session routing.",
                                "Adapter",
                                &["JSON events", "Resume", "Best-effort inspection"],
                                cx,
                            )),
                        )),
                    ),
            )
    }

    fn skills_catalog_surface(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.extension_query.read(cx).value().trim().to_lowercase();
        v_flex()
            .size_full()
            .child(self.extension_catalog_toolbar(cx))
            .child(
                v_flex()
                    .flex_1()
                    .overflow_y_scrollbar()
                    .w_full()
                    .max_w(px(980.))
                    .mx_auto()
                    .px_6()
                    .py_5()
                    .gap_5()
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight(700.))
                                    .child("Skills"),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Reusable instructions loaded for this workspace."),
                            ),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight(650.))
                            .child(format!(
                                "Discovered · {} skill{}",
                                self.skills.len(),
                                if self.skills.len() == 1 { "" } else { "s" }
                            )),
                    )
                    .when(self.skills_loading, |this| {
                        this.child(
                            h_flex()
                                .items_center()
                                .gap_2()
                                .child(Spinner::new().small())
                                .child("Loading skills…"),
                        )
                    })
                    .when(!self.skills_loading && self.skills.is_empty(), |this| {
                        this.child(HubEmptyState::new(
                            IconName::File,
                            "No skills discovered",
                            "Add SKILL.md packages under this project or your ALVA skill directories.",
                        ))
                    })
                    .child(
                        h_flex().flex_wrap().gap_3().children(
                            self.skills
                                .iter()
                                .filter(move |skill| {
                                    query.is_empty()
                                        || format!(
                                            "{} {}",
                                            skill.meta.name, skill.meta.description
                                        )
                                        .to_lowercase()
                                        .contains(&query)
                                })
                                .map(|skill| {
                                    let name = skill.meta.name.clone();
                                    let toggle_name = name.clone();
                                    let chat_name = name.clone();
                                    let weak_toggle = cx.entity().downgrade();
                                    let weak_chat = cx.entity().downgrade();
                                    let enabled = skill.enabled;
                                    HubExtensionCard::new(
                                        IconName::File,
                                        name,
                                        skill.meta.description.clone(),
                                        if enabled { "Enabled" } else { "Disabled" },
                                    )
                                    .action(
                                        h_flex()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                Checkbox::new(SharedString::from(format!(
                                                    "skill-enabled-{toggle_name}"
                                                )))
                                                .checked(enabled)
                                                .on_click(move |enabled, _, app| {
                                                    if let Some(hub) = weak_toggle.upgrade() {
                                                        hub.update(app, |hub, cx| {
                                                            hub.set_skill_enabled(
                                                                toggle_name.clone(),
                                                                *enabled,
                                                                cx,
                                                            )
                                                        });
                                                    }
                                                }),
                                            )
                                            .child(
                                                Button::new(SharedString::from(format!(
                                                    "chat-with-skill-{chat_name}"
                                                )))
                                                .ghost()
                                                .xsmall()
                                                .label("Chat")
                                                .disabled(!enabled)
                                                .on_click(move |_, window, app| {
                                                    if let Some(hub) = weak_chat.upgrade() {
                                                        hub.update(app, |hub, cx| {
                                                            hub.chat_with_skill(
                                                                &chat_name,
                                                                window,
                                                                cx,
                                                            )
                                                        });
                                                    }
                                                }),
                                            ),
                                    )
                                }),
                        ),
                    ),
            )
    }

    fn extension_tabs(&self, cx: &Context<Self>) -> TabBar {
        let weak = cx.entity().downgrade();
        TabBar::new("extension-tabs")
            .segmented()
            .small()
            .selected_index(self.extension_tab)
            .children([Tab::new().label("Plugins"), Tab::new().label("Skills")])
            .on_click(move |index, _, app| {
                if let Some(entity) = weak.upgrade() {
                    entity.update(app, |hub, cx| {
                        hub.extension_tab = *index;
                        cx.notify();
                    });
                }
            })
    }

    fn extension_catalog_toolbar(&self, cx: &Context<Self>) -> impl IntoElement {
        h_flex()
            .items_center()
            .gap_3()
            .px_6()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(self.extension_tabs(cx))
            .child(div().flex_1())
            .child(Input::new(&self.extension_query).small().w(px(260.)))
            .child(self.add_plugin_button(cx))
    }

    fn extension_matches(&self, cx: &Context<Self>, searchable: &str) -> bool {
        let query = self.extension_query.read(cx).value().trim().to_lowercase();
        query.is_empty() || searchable.contains(&query)
    }

    fn add_plugin_button(&self, cx: &Context<Self>) -> impl IntoElement {
        let weak_local = cx.entity().downgrade();
        let weak_mcp = cx.entity().downgrade();
        let weak_browse = cx.entity().downgrade();
        Button::new("add-plugin-header")
            .secondary()
            .small()
            .icon(IconName::Plus)
            .label("Add plugin")
            .dropdown_menu(move |menu, _, _| {
                let weak_local = weak_local.clone();
                let weak_mcp = weak_mcp.clone();
                let weak_browse = weak_browse.clone();
                menu.item(PopupMenuItem::label("Add to Hub"))
                    .item(
                        PopupMenuItem::new("Install from local folder…")
                            .icon(IconName::Folder)
                            .on_click(move |_, window, app| {
                                if let Some(entity) = weak_local.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.choose_plugin_folder(window, cx);
                                    });
                                }
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Connect MCP server…")
                            .icon(IconName::Frame)
                            .on_click(move |_, window, app| {
                                if let Some(entity) = weak_mcp.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.open_extension_sheet(
                                            IconName::Frame,
                                            "MCP tools",
                                            "Connect and configure external Model Context Protocol servers.",
                                            "Built in",
                                            &["Tool discovery", "Server lifecycle", "Permission routing"],
                                            window,
                                            cx,
                                        );
                                    });
                                }
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Browse integrations")
                            .icon(IconName::Search)
                            .on_click(move |_, window, app| {
                                if let Some(entity) = weak_browse.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.extension_tab = 0;
                                        hub.extension_query.update(cx, |state, cx| {
                                            state.set_value("", window, cx)
                                        });
                                        cx.notify();
                                    });
                                }
                            }),
                    )
            })
    }

    fn choose_plugin_folder(&self, window: &mut Window, cx: &mut Context<Self>) {
        let pick = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose a plugin folder".into()),
        });
        let handle = window.window_handle();
        cx.spawn(move |_hub, cx: &mut AsyncApp| {
            let mut cx = cx.clone();
            async move {
                if let Ok(Ok(Some(paths))) = pick.await {
                    if let Some(path) = paths.first() {
                        let path = path.clone();
                        with_window(&mut cx, handle, move |hub, window, cx| {
                            match hub.install_plugin_folder(&path, cx) {
                                Ok(name) => window.push_notification(
                                    format!(
                                        "Plugin installed: {name}. It will load on the next ALVA turn."
                                    ),
                                    cx,
                                ),
                                Err(error) => window.push_notification(error, cx),
                            }
                        });
                    }
                }
            }
        })
        .detach();
    }

    #[allow(clippy::too_many_arguments)]
    fn extension_detail_button(
        &self,
        id: &'static str,
        label: &'static str,
        icon: IconName,
        title: &'static str,
        description: &'static str,
        status: &'static str,
        capabilities: &'static [&'static str],
        cx: &Context<Self>,
    ) -> Button {
        let weak = cx.entity().downgrade();
        Button::new(id)
            .ghost()
            .xsmall()
            .label(label)
            .on_click(move |_, window, app| {
                if let Some(entity) = weak.upgrade() {
                    let icon = icon.clone();
                    entity.update(app, move |hub, cx| {
                        hub.open_extension_sheet(
                            icon,
                            title,
                            description,
                            status,
                            capabilities,
                            window,
                            cx,
                        );
                    });
                }
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn open_extension_sheet(
        &self,
        icon: IconName,
        title: &'static str,
        description: &'static str,
        status: &'static str,
        capabilities: &'static [&'static str],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.open_sheet(cx, move |sheet, _, _| {
            sheet.size(px(460.)).title(title).child(
                HubExtensionDetail::new(icon.clone(), title, description, status)
                    .capabilities(capabilities.iter().copied()),
            )
        });
    }

    pub fn settings_surface(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak_back = cx.entity().downgrade();
        let weak_inspector = cx.entity().downgrade();
        let weak_transcript = cx.entity().downgrade();
        let weak_readiness = cx.entity().downgrade();
        let weak_fallback = cx.entity().downgrade();
        let weak_team = cx.entity().downgrade();
        let transcript_sharing = self.transcript_sharing;
        let inspector_enabled = self.inspector_enabled;
        let has_session = self.active_session_id.is_some();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .px_5()
                    .py_3()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(
                        Button::new("close-settings")
                            .ghost()
                            .small()
                            .icon(IconName::ArrowLeft)
                            .tooltip("Back to Hub")
                            .on_click(move |_, window, app| {
                                if let Some(entity) = weak_back.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.close_settings(window, cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight(700.))
                            .child("Settings"),
                    ),
            )
            .child(
                Settings::new("hub-settings")
                    .sidebar_width(px(240.))
                    .with_group_variant(GroupBoxVariant::Outline)
                    .pages(vec![
                SettingPage::new("General")
                    .icon(IconName::Settings2)
                    .default_open(true)
                    .description("Configure the Hub shell and session behavior.")
                    .groups(vec![
                        SettingGroup::new().title("Appearance").items(vec![
                            static_setting("Theme", "Dark"),
                            static_setting("Interface font", "System UI · 14"),
                            static_setting("Code font", "Monospace · 14"),
                        ]),
                        SettingGroup::new().title("Session behavior").items(vec![
                            SettingItem::new(
                                "Share filtered transcript on handoff",
                                SettingField::switch(
                                    move |_| transcript_sharing,
                                    move |enabled, app| {
                                        if let Some(entity) = weak_transcript.upgrade() {
                                            entity.update(app, |hub, cx| {
                                                hub.toggle_transcript_sharing(enabled, cx);
                                            });
                                        }
                                    },
                                ),
                            )
                            .description("Off by default and scoped to the current Hub session.")
                            .disabled(!has_session),
                        ]),
                        SettingGroup::new()
                            .title("Engine orchestration")
                            .items(vec![
                                SettingItem::new(
                                    "Engine readiness",
                                    SettingField::render(move |options, _, _| {
                                        let weak_readiness = weak_readiness.clone();
                                        Button::new("settings-readiness")
                                            .secondary()
                                            .label("Open")
                                            .with_size(options.size)
                                            .on_click(move |_, window, app| {
                                                if let Some(entity) = weak_readiness.upgrade() {
                                                    entity.update(app, |hub, cx| {
                                                        hub.open_readiness_sheet(window, cx);
                                                    });
                                                }
                                            })
                                    }),
                                )
                                .description(
                                    "Installation, authentication, and compatibility status.",
                                ),
                                SettingItem::new(
                                    "Fallback policy",
                                    SettingField::render(move |options, _, _| {
                                        let weak_fallback = weak_fallback.clone();
                                        Button::new("settings-fallback")
                                            .secondary()
                                            .label("Open")
                                            .with_size(options.size)
                                            .on_click(move |_, window, app| {
                                                if let Some(entity) = weak_fallback.upgrade() {
                                                    entity.update(app, |hub, cx| {
                                                        hub.open_fallback_sheet(window, cx);
                                                    });
                                                }
                                            })
                                    }),
                                )
                                .description("Choose the ordered engine fallback chain."),
                                SettingItem::new(
                                    "Team workers",
                                    SettingField::render(move |options, _, _| {
                                        let weak_team = weak_team.clone();
                                        Button::new("settings-team")
                                            .secondary()
                                            .label("Open")
                                            .with_size(options.size)
                                            .on_click(move |_, window, app| {
                                                if let Some(entity) = weak_team.upgrade() {
                                                    entity.update(app, |hub, cx| {
                                                        hub.open_team_sheet(window, cx);
                                                    });
                                                }
                                            })
                                    }),
                                )
                                .description(
                                    "Configure recursive HubSession worker collaboration.",
                                ),
                            ]),
                    ]),
                placeholder_setting_page(
                    "Personalization",
                    IconName::User,
                    "Instructions, memory, and default agent behavior.",
                ),
                self.models_setting_page(cx),
                placeholder_setting_page(
                    "Usage",
                    IconName::ChartPie,
                    "Session usage, limits, and local execution history.",
                ),
                placeholder_setting_page(
                    "Keyboard shortcuts",
                    IconName::Frame,
                    "Navigation, composer, and workbench shortcuts.",
                ),
                placeholder_setting_page(
                    "Tools",
                    IconName::Settings2,
                    "Built-in tools, MCP services, and permissions.",
                ),
                placeholder_setting_page(
                    "Automation",
                    IconName::Calendar,
                    "Scheduling and background execution defaults.",
                ),
                SettingPage::new("Experiments")
                    .icon(IconName::Asterisk)
                    .description("Surfaces that are still evolving.")
                    .group(
                        SettingGroup::new().title("Debug tools").item(
                            SettingItem::new(
                                "Show Inspector",
                                SettingField::switch(
                                    move |_| inspector_enabled,
                                    move |enabled, app| {
                                        if let Some(entity) = weak_inspector.upgrade() {
                                            entity.update(app, |hub, cx| {
                                                hub.toggle_inspector(enabled, cx);
                                            });
                                        }
                                    },
                                ),
                            )
                            .description(
                                "ALVA sessions expose full records; other engines are best effort.",
                            ),
                        ),
                    ),
                placeholder_setting_page(
                    "About",
                    IconName::Info,
                    "Version, update channel, logs, and local storage.",
                ),
                    ]),
            )
    }

    fn models_setting_page(&self, cx: &mut Context<Self>) -> SettingPage {
        let weak_add = cx.entity().downgrade();
        let mut items = vec![SettingItem::new(
            "Add provider",
            SettingField::render(move |options, _, _| {
                let weak_add = weak_add.clone();
                Button::new("add-model-provider")
                    .primary()
                    .icon(IconName::Plus)
                    .label("Add provider")
                    .with_size(options.size)
                    .on_click(move |_, window, app| {
                        if let Some(hub) = weak_add.upgrade() {
                            hub.update(app, |hub, cx| hub.open_provider_dialog(window, cx));
                        }
                    })
            }),
        )
        .description(
            "Connect an API provider and declare exactly which agent runtimes can use it. Reuse a Provider ID to add another model.",
        )];
        for provider in self.model_catalog.providers() {
            let provider_id = provider.provider_id.clone();
            let title = provider.display_name.clone();
            let runtimes = provider
                .runtimes
                .iter()
                .map(|runtime| runtime.label())
                .collect::<Vec<_>>()
                .join(" · ");
            let model_count = self
                .model_catalog
                .models()
                .iter()
                .filter(|model| model.provider_id == provider.provider_id)
                .count();
            let capability_summary = self
                .model_catalog
                .models()
                .iter()
                .find(|model| model.provider_id == provider.provider_id)
                .map(|model| {
                    format!(
                        "{}k context · {}k output · {} · {}",
                        model.capabilities.context_window / 1_000,
                        model.capabilities.max_output_tokens / 1_000,
                        if model.capabilities.images {
                            "images"
                        } else {
                            "text only"
                        },
                        if model.capabilities.tools {
                            "tools"
                        } else {
                            "no tools"
                        }
                    )
                })
                .unwrap_or_else(|| "No model configured".into());
            let description = format!(
                "{} · {} · {model_count} model{} · {}",
                provider.protocol.label(),
                runtimes,
                if model_count == 1 { "" } else { "s" },
                capability_summary
            );
            if provider.built_in {
                let connected = provider.connected;
                items.push(
                    SettingItem::new(
                        title,
                        SettingField::render(move |options, _, _| {
                            Tag::new()
                                .with_variant(if connected {
                                    TagVariant::Success
                                } else {
                                    TagVariant::Warning
                                })
                                .small()
                                .rounded_full()
                                .with_size(options.size)
                                .child(if connected {
                                    "Connected"
                                } else {
                                    "Setup needed"
                                })
                        }),
                    )
                    .description(description),
                );
            } else {
                let weak_remove = cx.entity().downgrade();
                items.push(
                    SettingItem::new(
                        title,
                        SettingField::render(move |options, _, _| {
                            let provider_id = provider_id.clone();
                            let weak_remove = weak_remove.clone();
                            Button::new(SharedString::from(format!("remove-{provider_id}")))
                                .danger()
                                .label("Remove")
                                .with_size(options.size)
                                .on_click(move |_, window, app| {
                                    if let Some(hub) = weak_remove.upgrade() {
                                        let result = hub.update(app, |hub, cx| {
                                            hub.remove_custom_provider(&provider_id, window, cx)
                                        });
                                        match result {
                                            Ok(()) => {
                                                window.push_notification("Provider removed", app)
                                            }
                                            Err(error) => window.push_notification(error, app),
                                        }
                                    }
                                })
                        }),
                    )
                    .description(description),
                );
            }
            for model in self
                .model_catalog
                .models()
                .iter()
                .filter(|model| model.provider_id == provider.provider_id)
            {
                let title = format!("↳ {}", model.display_name);
                let reasoning = if model.capabilities.reasoning_efforts.is_empty() {
                    "fixed reasoning".to_string()
                } else {
                    format!(
                        "reasoning {}",
                        model
                            .capabilities
                            .reasoning_efforts
                            .iter()
                            .map(|effort| effort.label())
                            .collect::<Vec<_>>()
                            .join("/")
                    )
                };
                let description = format!(
                    "{} · {}k context · {}k output · {} · {} · {}",
                    model.model_id,
                    model.capabilities.context_window / 1_000,
                    model.capabilities.max_output_tokens / 1_000,
                    if model.capabilities.images {
                        "image input"
                    } else {
                        "text only"
                    },
                    if model.capabilities.tools {
                        "tools"
                    } else {
                        "no tools"
                    },
                    reasoning,
                );
                let enabled = model.enabled && provider.connected;
                items.push(
                    SettingItem::new(
                        title,
                        SettingField::render(move |options, _, _| {
                            Tag::new()
                                .with_variant(if enabled {
                                    TagVariant::Success
                                } else {
                                    TagVariant::Warning
                                })
                                .small()
                                .rounded_full()
                                .with_size(options.size)
                                .child(if enabled { "Available" } else { "Unavailable" })
                        }),
                    )
                    .description(description),
                );
            }
        }
        SettingPage::new("Models & engines")
            .icon(IconName::Bot)
            .description(
                "Subscription accounts, API providers, model capabilities, and compatible agents.",
            )
            .group(SettingGroup::new().title("Providers").items(items))
    }
}

fn static_setting(title: &'static str, value: &'static str) -> SettingItem {
    SettingItem::new(
        title,
        SettingField::render(move |options, _, _| {
            Tag::secondary()
                .small()
                .rounded_full()
                .with_size(options.size)
                .child(value)
        }),
    )
}

fn placeholder_setting_page(
    title: &'static str,
    icon: IconName,
    description: &'static str,
) -> SettingPage {
    SettingPage::new(title)
        .icon(icon)
        .description(description)
        .group(SettingGroup::new().item(static_setting("Status", "Component shell ready")))
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
