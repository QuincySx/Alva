// INPUT:  HubApp.permission_router / approval_groups
// OUTPUT: merged approval cards — equivalent concurrent requests from one
//         scope answered together, with per-member detail never hidden.
// POS:    1:1 GPUI port of the React approval-group cards in App.tsx.

use std::sync::Arc;

use gpui::*;
use gpui_component::{button::Button, ActiveTheme, *};

use alva_hub_gpui::PermissionGroup;

use super::app::HubApp;

impl HubApp {
    /// Reload merged approval cards from the router.
    pub fn refresh_approval_groups(&mut self) {
        self.approval_groups = self.permission_router.groups();
    }

    /// Answer every member of one equivalence group exactly as displayed
    /// (React `decideApprovalGroup`), then audit each covered session.
    pub fn decide_approval_group(
        &self,
        group: PermissionGroup,
        allow: bool,
        cx: &mut Context<Self>,
    ) {
        let router = Arc::clone(&self.permission_router);
        let message = if allow {
            None
        } else {
            Some("Denied by the user".to_string())
        };
        let members = group.members.clone();
        let request_ids = members
            .iter()
            .map(|member| member.request_id.clone())
            .collect::<Vec<_>>();
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let outcome = router.respond_many(&members, allow, message).await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| match outcome {
                    Ok(outcome) => {
                        for member in &outcome.answered {
                            let summary = format!(
                                "{} {}",
                                if allow { "allowed" } else { "denied" },
                                member.request_id
                            );
                            let _ = hub
                                .store
                                .record_permission_decision(&member.product_session_id, &summary);
                        }
                        for request_id in request_ids {
                            hub.answered_permissions.insert(request_id, allow);
                        }
                        hub.refresh_approval_groups();
                        cx.notify();
                    }
                    Err(error) => hub.push_local_failure(&error, cx),
                });
            }
        })
        .detach();
    }

    /// Approval cards for groups with more than one member (singletons are
    /// answered inline on their timeline card).
    pub fn approval_area(&self, cx: &Context<Self>) -> impl IntoElement {
        let groups: Vec<PermissionGroup> = self
            .approval_groups
            .iter()
            .filter(|group| group.members.len() > 1)
            .cloned()
            .collect();
        let weak = cx.entity().downgrade();
        let cards: Vec<gpui::AnyElement> = groups
            .iter()
            .map(|group| {
                let group = group.clone();
                let weak = weak.clone();
                let member_count = group.members.len();
                let tool_name = group.tool_name.clone();
                let resource = group
                    .members
                    .first()
                    .and_then(|member| member.resource.clone());
                let group_key = group.group_key.clone();
                let mut card =
                    v_flex()
                        .gap_1_5()
                        .px_3()
                        .py_2()
                        .rounded_md()
                        .border_1()
                        .border_color(cx.theme().colors.warning)
                        .child(div().text_sm().font_weight(FontWeight(600.)).child(
                            match &resource {
                                Some(resource) => format!(
                                    "{member_count} workers want to run '{tool_name}' on {resource}"
                                ),
                                None => format!("{member_count} workers want to run '{tool_name}'"),
                            },
                        ))
                        // Aggregation must never hide who is asking for what.
                        .children(group.members.iter().map(|member| {
                            let line = match &member.resource {
                                Some(resource) => format!(
                                    "{} · {} · {}",
                                    member.requester, member.tool_name, resource
                                ),
                                None => format!("{} · {}", member.requester, member.tool_name),
                            };
                            div().text_xs().child(line)
                        }));
                let weak_allow = weak.clone();
                card = card.child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new(SharedString::from(format!("allow-all-{group_key}")))
                                .small()
                                .label(format!("Allow all {member_count}"))
                                .on_click({
                                    let group = group.clone();
                                    move |_, _, app| {
                                        if let Some(entity) = weak_allow.upgrade() {
                                            entity.update(app, |hub, cx| {
                                                hub.decide_approval_group(group.clone(), true, cx);
                                            });
                                        }
                                    }
                                }),
                        )
                        .child(
                            Button::new(SharedString::from(format!("deny-all-{group_key}")))
                                .small()
                                .label("Deny all")
                                .on_click({
                                    let group = group.clone();
                                    move |_, _, app| {
                                        if let Some(entity) = weak.clone().upgrade() {
                                            entity.update(app, |hub, cx| {
                                                hub.decide_approval_group(group.clone(), false, cx);
                                            });
                                        }
                                    }
                                }),
                        ),
                );
                card.into_any_element()
            })
            .collect();
        div().v_flex().gap_2().px_4().children(cards)
    }
}
