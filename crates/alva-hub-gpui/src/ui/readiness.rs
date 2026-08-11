// INPUT:  HubApp.readiness / repair_plan / repair_logs
// OUTPUT: sidebar "Engines" panel — readiness cards with badges, reasons,
//         one-click repair actions and the official-guide link.
// POS:    1:1 GPUI port of the React readiness panel in App.tsx.

use gpui::*;
use gpui_component::{
    accordion::Accordion,
    button::{Button, ButtonVariants as _},
    group_box::{GroupBox, GroupBoxVariants as _},
    skeleton::Skeleton,
    tag::Tag,
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
        let weak_refresh = cx.entity().downgrade();
        let readiness = self.readiness.clone();
        let repair_plan = self.repair_plan.clone();
        let repair_logs = self.repair_logs.clone();
        let repairing = self.repairing_engine.clone();
        let theme = cx.theme().clone();
        let content =
            if readiness.is_empty() && busy {
                v_flex()
                    .gap_2()
                    .child(Skeleton::new().h_3())
                    .child(Skeleton::new().secondary().h_3())
                    .child(Skeleton::new().secondary().h_3())
                    .into_any_element()
            } else if readiness.is_empty() {
                div()
                    .text_xs()
                    .text_color(theme.colors.muted_foreground)
                    .child("Engine health will appear here.")
                    .into_any_element()
            } else {
                let engine_ids = readiness
                    .iter()
                    .map(|engine| engine.engine_id.clone())
                    .collect::<Vec<_>>();
                let accordion =
                    readiness.iter().fold(
                        Accordion::new("readiness-engine-list")
                            .multiple(true)
                            .small(),
                        |accordion, engine| {
                            let plan = repair_plan
                                .iter()
                                .filter(|action| action.engine_id == engine.engine_id)
                                .cloned()
                                .collect::<Vec<_>>();
                            let log = repair_logs.get(&engine.engine_id).cloned();
                            let key = format!("readiness-{}", engine.engine_id);
                            let default_collapsed =
                                matches!(engine.status, alva_hub_gpui::ReadinessStatus::Healthy);
                            let open = !self
                                .collapsed
                                .get(&key)
                                .copied()
                                .unwrap_or(default_collapsed);
                            accordion.item(|item| {
                                item.open(open).title(readiness_title(engine)).child(
                                    readiness_details(engine, &plan, log, repairing.as_deref(), cx),
                                )
                            })
                        },
                    );
                let weak = cx.entity().downgrade();
                accordion
                    .on_toggle_click(move |open_items, _, app| {
                        if let Some(entity) = weak.upgrade() {
                            entity.update(app, |hub, cx| {
                                for (index, engine_id) in engine_ids.iter().enumerate() {
                                    hub.collapsed.insert(
                                        format!("readiness-{engine_id}"),
                                        !open_items.contains(&index),
                                    );
                                }
                                cx.notify();
                            });
                        }
                    })
                    .into_any_element()
            };
        GroupBox::new()
            .outline()
            .title(
                h_flex()
                    .w_full()
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
                            .label("Refresh")
                            .loading(busy)
                            .disabled(busy)
                            .on_click(move |_, _, app| {
                                if let Some(entity) = weak_refresh.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.refresh_readiness(cx);
                                    });
                                }
                            }),
                    ),
            )
            .child(content)
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

fn readiness_title(engine: &EngineReadiness) -> impl IntoElement {
    h_flex()
        .w_full()
        .items_center()
        .justify_between()
        .child(engine.display_name.clone())
        .child(
            Tag::new()
                .with_variant(super::readiness_tag_variant(&engine.status))
                .small()
                .rounded_full()
                .child(readiness_badge(status_str(&engine.status))),
        )
}

fn readiness_details(
    engine: &EngineReadiness,
    plan: &[alva_hub_gpui::RepairAction],
    log: Option<String>,
    repairing: Option<&str>,
    cx: &Context<HubApp>,
) -> impl IntoElement {
    let version_line = match (&engine.version, engine.minimum_version.as_str()) {
        (Some(version), minimum) => format!("v{version} · requires ≥ {minimum}"),
        (None, minimum) => format!("requires ≥ {minimum}"),
    };
    let auth_line = match engine.authenticated {
        Some(true) => "signed in",
        Some(false) => "not signed in",
        None => "sign-in unknown",
    };
    let mut content = v_flex().gap_1().child(
        div()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child(format!(
                "{version_line} · {auth_line} · stream {}",
                stream_str(&engine.stream_health)
            )),
    );
    if !engine.capabilities.is_empty() {
        content = content.child(
            div()
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child(engine.capabilities.join(" · ")),
        );
    }
    for reason in &engine.reasons {
        content = content.child(
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
                .label(title)
                .loading(is_repairing)
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
        content = content.child(row);
    }
    if let Some(log) = log {
        content = content.child(
            div()
                .text_xs()
                .text_color(cx.theme().colors.muted_foreground)
                .child(log),
        );
    }
    content
}
