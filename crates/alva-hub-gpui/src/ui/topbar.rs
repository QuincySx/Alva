// INPUT:  HubApp engine/permission selection, readiness, eligibility
// OUTPUT: topbar — engine selector, permission profile selector, transcript
//         toggle, Details toggle and the status pill.
// POS:    1:1 GPUI port of the React topbar in App.tsx.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    select::Select,
    ActiveTheme, *,
};

use super::app::HubApp;

impl HubApp {
    pub fn topbar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_session = self.active_session_id.is_some();
        let selected = self.selected_engine(cx);
        let selected_id = self.pending_engine.as_deref().unwrap_or(selected.as_str());
        // Eligibility of the currently selected engine.
        let eligibility_detail = self
            .eligibility
            .iter()
            .find(|candidate| candidate.engine_id == selected_id)
            .map(|candidate| (candidate.eligible, candidate.detail.clone()));
        let details_open = self.details_open();
        let weak = cx.entity().downgrade();
        let weak2 = cx.entity().downgrade();
        let status = self.status_label();
        let status_color = super::app::status_color(
            self.timeline
                .last()
                .map(|e| &e.status)
                .unwrap_or(&alva_hub_gpui::TimelineStatus::Completed),
            cx,
        )
        .1;
        h_flex()
            .gap_3()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .bg(cx.theme().colors.sidebar)
            .child(
                v_flex()
                    .gap_1()
                    .child(self.label_small("Engine", cx))
                    .child(Select::new(&self.engine).small())
                    .when(self.pending_engine.is_some(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().colors.muted_foreground)
                                .child("Queued after the current run"),
                        )
                    }),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(self.label_small("Permissions", cx))
                    .child(Select::new(&self.permission).small().disabled(has_session))
                    .when(has_session, |this| {
                        this.child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    Checkbox::new("transcript-toggle")
                                        .label("Attach filtered full transcript to future handoffs")
                                        .checked(self.transcript_sharing)
                                        .on_click(move |checked, _, app| {
                                            if let Some(entity) = weak.upgrade() {
                                                entity.update(app, |hub, cx| {
                                                    hub.toggle_transcript_sharing(*checked, cx);
                                                });
                                            }
                                        }),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(cx.theme().colors.muted_foreground)
                                        .child("Fixed for this session"),
                                ),
                        )
                    })
                    .when(!has_session, |this| {
                        let detail = eligibility_detail.clone();
                        this.child(if let Some((eligible, detail)) = detail {
                            div()
                                .text_xs()
                                .when(!eligible, |d| d.text_color(cx.theme().colors.danger))
                                .when(eligible, |d| {
                                    d.text_color(cx.theme().colors.muted_foreground)
                                })
                                .child(detail)
                        } else {
                            div()
                        })
                    }),
            )
            .child(div().flex_1())
            .child(
                Button::new("details")
                    .ghost()
                    .label(if details_open {
                        "Hide details"
                    } else {
                        "Details"
                    })
                    .disabled(!has_session)
                    .on_click(move |_, _, app| {
                        if let Some(entity) = weak2.upgrade() {
                            entity.update(app, |hub, cx| {
                                hub.toggle_details(cx);
                            });
                        }
                    }),
            )
            .child(
                h_flex()
                    .gap_1_5()
                    .items_center()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(cx.theme().colors.button)
                    .child(div().size_2().rounded_full().bg(status_color))
                    .child(div().text_sm().child(status)),
            )
    }
}
