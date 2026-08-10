// INPUT:  HubApp.session_detail + readiness
// OUTPUT: right-hand Details drawer — memory, branches, attempts, handoffs
//         (with expandable envelopes) and engine diagnostics.
// POS:    1:1 GPUI port of the React details drawer in App.tsx.

use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    input::Input,
    scroll::ScrollableElement,
    ActiveTheme, *,
};

use alva_hub_gpui::HandoffKind;

use super::app::HubApp;
use super::readiness_badge;

impl HubApp {
    /// Right-hand drawer shown while `details_open` and a session is loaded.
    pub fn details_drawer(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(detail) = self.session_detail.clone() else {
            return div().into_any_element();
        };
        let memory = detail.memory.clone();
        let branches = detail.branches.clone();
        let turns = detail.turns.clone();
        let handoffs = detail.handoffs.clone();
        let readiness = self.readiness.clone();
        v_flex()
            .id("details-drawer")
            .w(px(340.))
            .h_full()
            .flex_shrink_0()
            .border_l_1()
            .border_color(cx.theme().colors.border)
            .bg(cx.theme().colors.sidebar)
            .overflow_y_scrollbar()
            .child(
                v_flex()
                    .gap_1_5()
                    .p_3()
                    .child(self.drawer_memory(&memory, window, cx))
                    .child(self.drawer_branches(&branches, cx))
                    .child(self.drawer_attempts(&turns, cx))
                    .child(self.drawer_handoffs(&handoffs, cx))
                    .child(self.drawer_diagnostics(&readiness, cx)),
            )
            .into_any_element()
    }

    fn drawer_memory(
        &self,
        memory: &[String],
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        let draft = self.memory_draft.read(cx).value().to_string();
        let can_save = !draft.trim().is_empty();
        v_flex()
            .gap_1_5()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight(650.))
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("Memory"),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(
                        "Corrections every engine in this session carries across fallbacks and handoffs.",
                    ),
            )
            .children(if memory.is_empty() {
                vec![div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("No memory yet. Teach the session once; every engine keeps it.")
                    .into_any_element()]
            } else {
                memory
                    .iter()
                    .map(|entry| {
                        div()
                            .text_xs()
                            .child(entry.clone())
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            })
            .child(
                h_flex()
                    .gap_1()
                    .child(Input::new(&self.memory_draft).small())
                    .child(
                        Button::new("remember")
                            .secondary()
                            .small()
                            .label("Remember")
                            .disabled(!can_save)
                            .on_click(move |_, window, app| {
                                if weak.upgrade().is_some() {
                                    let handle = window.window_handle();
                                    let _ = handle.update(app, |root, window, cx| {
                                        if let Ok(view) = root.downcast::<HubApp>() {
                                            view.update(cx, |hub, cx| {
                                                hub.add_memory(window, cx);
                                            });
                                        }
                                    });
                                }
                            }),
                    ),
            )
    }

    fn drawer_branches(
        &self,
        branches: &[alva_hub_gpui::StoredBranch],
        cx: &Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap_1_5()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight(650.))
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("Branches"),
            )
            .children(if branches.is_empty() {
                vec![div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("No branches yet.")
                    .into_any_element()]
            } else {
                branches
                    .iter()
                    .map(|branch| {
                        let native = if branch.engine_session_id.is_some() {
                            " · native session kept"
                        } else {
                            ""
                        };
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight(600.))
                                    .child(format!("{} · {}", branch.engine_id, branch.status)),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(format!(
                                        "{} · {}{native}",
                                        branch.model_id, branch.provider_id
                                    )),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            })
    }

    fn drawer_attempts(
        &self,
        turns: &[alva_hub_gpui::StoredTurn],
        cx: &Context<Self>,
    ) -> impl IntoElement {
        v_flex()
            .gap_1_5()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight(650.))
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("Attempts"),
            )
            .children(if turns.iter().all(|turn| turn.attempts.is_empty()) {
                vec![div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("No attempts yet.")
                    .into_any_element()]
            } else {
                turns
                    .iter()
                    .flat_map(|turn| turn.attempts.iter())
                    .map(|attempt| {
                        let mut row = v_flex().gap_1().child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight(600.))
                                .child(format!("{} · {}", attempt.engine_id, attempt.status)),
                        );
                        if let Some(reason) = &attempt.failure_reason {
                            row = row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(reason.clone()),
                            );
                        }
                        if let Some(usage) = &attempt.usage {
                            row = row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(format!(
                                        "{}in/{}out · {}ms",
                                        usage.input_tokens, usage.output_tokens, usage.duration_ms
                                    )),
                            );
                        }
                        if !attempt.side_effects.is_empty() {
                            let paths = attempt
                                .side_effects
                                .iter()
                                .filter_map(|evidence| evidence.path.clone())
                                .collect::<Vec<_>>();
                            let line = if paths.is_empty() {
                                format!("{} tool call(s)", attempt.side_effects.len())
                            } else {
                                format!(
                                    "{} tool call(s) · {}",
                                    attempt.side_effects.len(),
                                    paths.join(", ")
                                )
                            };
                            row = row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(line),
                            );
                        }
                        row.into_any_element()
                    })
                    .collect::<Vec<_>>()
            })
    }

    fn drawer_handoffs(
        &self,
        handoffs: &[alva_hub_gpui::HandoffPackage],
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let weak = cx.entity().downgrade();
        v_flex()
            .gap_1_5()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight(650.))
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("Handoffs"),
            )
            .children(if handoffs.is_empty() {
                vec![div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("No handoffs yet.")
                    .into_any_element()]
            } else {
                handoffs
                    .iter()
                    .map(|handoff| {
                        let weak = weak.clone();
                        let handoff_id = handoff.handoff_id.clone();
                        let kind = match handoff.kind {
                            HandoffKind::Full => "full",
                            HandoffKind::Delta => "delta",
                        };
                        let delivered = if handoff.used_at.is_some() {
                            "delivered"
                        } else {
                            "pending"
                        };
                        let envelope = if handoff.session_memory.is_empty()
                            && handoff.available_skills.is_empty()
                            && handoff.available_mcp.is_empty()
                        {
                            String::new()
                        } else {
                            format!(
                                "envelope: {} memory · {} skills · {} MCP",
                                handoff.session_memory.len(),
                                handoff.available_skills.len(),
                                handoff.available_mcp.len()
                            )
                        };
                        let open = self
                            .open_drawer_handoffs
                            .get(&handoff_id)
                            .copied()
                            .unwrap_or(false);
                        let mut row = v_flex()
                            .gap_1()
                            .child(div().text_xs().font_weight(FontWeight(600.)).child(format!(
                                "{kind} · {} → {} · {delivered}",
                                handoff.source.engine_id, handoff.target.engine_id
                            )))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(handoff.reason.clone()),
                            );
                        if let Some(snapshot) = &handoff.team_snapshot {
                            row = row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(snapshot.clone()),
                            );
                        }
                        if !envelope.is_empty() {
                            row = row.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().colors.muted_foreground)
                                    .child(envelope),
                            );
                        }
                        let handoff_id_button = handoff_id.clone();
                        row = row.child(
                            Button::new(SharedString::from(format!("envelope-{handoff_id}")))
                                .ghost()
                                .xsmall()
                                .label(if open {
                                    "Hide envelope"
                                } else {
                                    "Show envelope"
                                })
                                .on_click(move |_, window, app| {
                                    if weak.upgrade().is_some() {
                                        let handle = window.window_handle();
                                        let _ = handle.update(app, |root, _window, cx| {
                                            if let Ok(view) = root.downcast::<HubApp>() {
                                                view.update(cx, |hub, cx| {
                                                    let open = hub
                                                        .open_drawer_handoffs
                                                        .get(&handoff_id_button)
                                                        .copied()
                                                        .unwrap_or(false);
                                                    hub.open_drawer_handoffs
                                                        .insert(handoff_id_button.clone(), !open);
                                                    cx.notify();
                                                });
                                            }
                                        });
                                    }
                                }),
                        );
                        if open {
                            let mut envelope = v_flex().gap_1();
                            if !handoff.session_memory.is_empty() {
                                envelope = envelope
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight(600.))
                                            .child("Memory carried"),
                                    )
                                    .children(
                                        handoff
                                            .session_memory
                                            .iter()
                                            .map(|entry| div().text_xs().child(entry.clone())),
                                    );
                            }
                            if !handoff.available_skills.is_empty() {
                                envelope = envelope
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight(600.))
                                            .child("Skills pointed"),
                                    )
                                    .children(handoff.available_skills.iter().map(|skill| {
                                        div().text_xs().child(format!(
                                            "{} — {} {}",
                                            skill.name, skill.description, skill.path
                                        ))
                                    }));
                            }
                            if !handoff.available_mcp.is_empty() {
                                envelope = envelope
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_weight(FontWeight(600.))
                                            .child("MCP servers"),
                                    )
                                    .children(handoff.available_mcp.iter().map(|server| {
                                        div().text_xs().child(format!(
                                            "{} {}",
                                            server.display_name, server.transport
                                        ))
                                    }));
                            }
                            if !handoff.transcript.is_empty() {
                                envelope = envelope
                                    .child(div().text_xs().font_weight(FontWeight(600.)).child(
                                        format!(
                                            "Filtered transcript ({} turns)",
                                            handoff.transcript.len()
                                        ),
                                    ))
                                    .children(handoff.transcript.iter().map(|turn| {
                                        let line = match &turn.result {
                                            Some(result) => {
                                                format!("» {} — {result}", turn.prompt)
                                            }
                                            None => format!("» {}", turn.prompt),
                                        };
                                        div().text_xs().child(line)
                                    }));
                            } else {
                                envelope = envelope.child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().colors.muted_foreground)
                                        .child("Transcript not shared (default)."),
                                );
                            }
                            row = row.child(envelope);
                        }
                        row.into_any_element()
                    })
                    .collect::<Vec<_>>()
            })
    }

    fn drawer_diagnostics(
        &self,
        readiness: &[alva_hub_gpui::EngineReadiness],
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme().clone();
        v_flex()
            .gap_1_5()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight(650.))
                    .text_color(theme.colors.muted_foreground)
                    .child("Engine diagnostics"),
            )
            .children(if readiness.is_empty() {
                vec![div().into_any_element()]
            } else {
                readiness
                    .iter()
                    .map(|engine| {
                        let reasons = engine
                            .reasons
                            .iter()
                            .map(super::describe_readiness_reason)
                            .collect::<Vec<_>>();
                        v_flex()
                            .gap_1()
                            .child(div().text_xs().font_weight(FontWeight(600.)).child(format!(
                                "{} · {}",
                                engine.display_name,
                                readiness_badge(match engine.status {
                                    alva_hub_gpui::ReadinessStatus::Healthy => "healthy",
                                    alva_hub_gpui::ReadinessStatus::Missing => "missing",
                                    alva_hub_gpui::ReadinessStatus::Incompatible => {
                                        "incompatible"
                                    }
                                    alva_hub_gpui::ReadinessStatus::Unauthenticated => {
                                        "unauthenticated"
                                    }
                                    alva_hub_gpui::ReadinessStatus::Unhealthy => "unhealthy",
                                })
                            )))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.colors.muted_foreground)
                                    .child(format!(
                                        "{} · stream {}",
                                        engine
                                            .version
                                            .clone()
                                            .unwrap_or_else(|| "version unknown".to_string()),
                                        match engine.stream_health {
                                            alva_hub_gpui::StreamHealth::Ok => "ok",
                                            alva_hub_gpui::StreamHealth::Untested => "untested",
                                            alva_hub_gpui::StreamHealth::Failing => "failing",
                                        }
                                    )),
                            )
                            .children(
                                reasons
                                    .iter()
                                    .map(|reason| div().text_xs().child(reason.clone())),
                            )
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            })
    }
}
