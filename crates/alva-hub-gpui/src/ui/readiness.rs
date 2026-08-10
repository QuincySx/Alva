// INPUT:  HubApp.readiness / repair_plan / repair_logs
// OUTPUT: sidebar "Engines" panel — readiness cards with badges, reasons,
//         one-click repair actions and the official-guide link.
// POS:    1:1 GPUI port of the React readiness panel in App.tsx.

use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    ActiveTheme, *,
};

use alva_hub_gpui::EngineReadiness;

use super::app::HubApp;
use super::{describe_readiness_reason, readiness_badge};

impl HubApp {
    /// Engines dashboard: one card per engine + refresh + repair entry.
    pub fn readiness_panel(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let busy = self.readiness_busy;
        let weak = cx.entity().downgrade();
        let readiness = self.readiness.clone();
        let repair_plan = self.repair_plan.clone();
        let repair_logs = self.repair_logs.clone();
        let repairing = self.repairing_engine.clone();
        let theme = cx.theme().clone();
        v_flex()
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(theme.colors.border)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight(650.))
                            .text_color(theme.colors.muted_foreground)
                            .child("Engines"),
                    )
                    .child(
                        Button::new("readiness-refresh")
                            .ghost()
                            .xsmall()
                            .label(if busy { "Checking…" } else { "Refresh" })
                            .disabled(busy)
                            .on_click(move |_, _, app| {
                                if let Some(entity) = weak.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.refresh_readiness(cx);
                                    });
                                }
                            }),
                    ),
            )
            .child(div().flex_col().gap_2().children(if readiness.is_empty() {
                vec![div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("Engine health will appear here.")
                    .into_any_element()]
            } else {
                readiness
                    .iter()
                    .map(|engine| {
                        let plan = repair_plan
                            .iter()
                            .filter(|action| action.engine_id == engine.engine_id)
                            .cloned()
                            .collect::<Vec<_>>();
                        let log = repair_logs.get(&engine.engine_id).cloned();
                        readiness_card(engine, &plan, log, repairing.as_deref(), cx)
                            .into_any_element()
                    })
                    .collect::<Vec<_>>()
            }))
    }
}

fn status_str(status: &alva_hub_gpui::ReadinessStatus) -> &'static str {
    use alva_hub_gpui::ReadinessStatus::*;
    match status {
        Healthy => "healthy",
        Missing => "missing",
        Incompatible => "incompatible",
        Unauthenticated => "unauthenticated",
        Unhealthy => "unhealthy",
    }
}

fn stream_str(health: &alva_hub_gpui::StreamHealth) -> &'static str {
    use alva_hub_gpui::StreamHealth::*;
    match health {
        Ok => "ok",
        Untested => "untested",
        Failing => "failing",
    }
}

/// One engine's readiness card.
fn readiness_card(
    engine: &EngineReadiness,
    plan: &[alva_hub_gpui::RepairAction],
    log: Option<String>,
    repairing: Option<&str>,
    cx: &Context<HubApp>,
) -> impl IntoElement {
    let badge_color = match engine.status {
        alva_hub_gpui::ReadinessStatus::Healthy => cx.theme().colors.success,
        alva_hub_gpui::ReadinessStatus::Missing => cx.theme().colors.muted_foreground,
        alva_hub_gpui::ReadinessStatus::Incompatible => cx.theme().colors.warning,
        alva_hub_gpui::ReadinessStatus::Unauthenticated => cx.theme().colors.warning,
        alva_hub_gpui::ReadinessStatus::Unhealthy => cx.theme().colors.danger,
    };
    let version_line = match (&engine.version, engine.minimum_version.as_str()) {
        (Some(version), minimum) => format!("v{version} · requires ≥ {minimum}"),
        (None, minimum) => format!("requires ≥ {minimum}"),
    };
    let auth_line = match engine.authenticated {
        Some(true) => "signed in",
        Some(false) => "not signed in",
        None => "sign-in unknown",
    };
    let mut card = div()
        .id(SharedString::from(format!(
            "readiness-{}",
            engine.engine_id
        )))
        .v_flex()
        .gap_1()
        .child(
            h_flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight(600.))
                        .child(engine.display_name.clone()),
                )
                .child(
                    div()
                        .px_1()
                        .py_1()
                        .rounded_full()
                        .text_xs()
                        .text_color(badge_color)
                        .child(readiness_badge(status_str(&engine.status))),
                ),
        )
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child(format!(
                    "{version_line} · {auth_line} · stream {}",
                    stream_str(&engine.stream_health)
                )),
        );
    if !engine.capabilities.is_empty() {
        card = card.child(
            div()
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child(engine.capabilities.join(" · ")),
        );
    }
    for reason in &engine.reasons {
        card = card.child(
            div()
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child(describe_readiness_reason(reason)),
        );
    }
    for action in plan {
        let is_repairing = repairing == Some(engine.engine_id.as_str());
        let weak = cx.entity().downgrade();
        let engine_id = engine.engine_id.clone();
        let kind = action.kind;
        let title = action.title.clone();
        let mut row = div().v_flex().gap_1();
        if action.one_click {
            row = row.child(
                Button::new(SharedString::from(format!(
                    "repair-{}-{:?}",
                    engine.engine_id, action.kind
                )))
                .small()
                .label(if is_repairing {
                    "Working…".to_string()
                } else {
                    title
                })
                .disabled(repairing.is_some())
                .on_click(move |_, _, app| {
                    if let Some(entity) = weak.upgrade() {
                        entity.update(app, |hub, cx| {
                            hub.run_repair(engine_id.clone(), kind, cx);
                        });
                    }
                }),
            );
        } else {
            row = row.child(div().text_xs().child(action.title.clone()));
            if let Some(command) = &action.command {
                row = row.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().colors.muted_foreground)
                        .font_family("ui-monospace")
                        .child(command.clone()),
                );
            }
        }
        if let Some(url) = &action.url {
            let url = url.clone();
            row = row.child(
                Button::new(SharedString::from(format!("guide-{}", engine.engine_id)))
                    .link()
                    .xsmall()
                    .label("Official guide")
                    .on_click(move |_, _, app| {
                        app.open_url(&url);
                    }),
            );
        }
        card = card.child(row);
    }
    if let Some(log) = log {
        card = card.child(
            div()
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child(log),
        );
    }
    card
}
