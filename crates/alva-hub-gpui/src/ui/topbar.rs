// INPUT:  HubApp engine/permission selection, readiness, eligibility
// OUTPUT: topbar — engine tabs, permission profile selector, transcript
//         toggle, Details Sheet trigger and status tag.
// POS:    Primary Hub controls built from gpui-component semantics.

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    checkbox::Checkbox,
    select::Select,
    switch::Switch,
    tab::{Tab, TabBar},
    tag::Tag,
    ActiveTheme, *,
};

use super::app::HubApp;

impl HubApp {
    pub fn topbar(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        const ENGINES: [&str; 3] = ["alva", "claude", "codex"];
        let has_session = self.active_session_id.is_some();
        let selected = self.selected_engine(cx);
        let selected_id = self.pending_engine.as_deref().unwrap_or(selected.as_str());
        let selected_index = ENGINES
            .iter()
            .position(|engine| *engine == selected_id)
            .unwrap_or(0);
        // Eligibility of the currently selected engine.
        let eligibility_detail = self
            .eligibility
            .iter()
            .find(|candidate| candidate.engine_id == selected_id)
            .map(|candidate| (candidate.eligible, candidate.detail.clone()));
        let weak_engine = cx.entity().downgrade();
        let weak_transcript = cx.entity().downgrade();
        let weak_details = cx.entity().downgrade();
        let weak_inspector = cx.entity().downgrade();
        let status = self.status_label();
        let timeline_status = self
            .timeline
            .last()
            .map(|event| &event.status)
            .unwrap_or(&alva_hub_gpui::TimelineStatus::Completed);
        v_flex()
            .gap_2()
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .bg(cx.theme().colors.sidebar)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(self.label_small("Engine", cx))
                    .child(
                        TabBar::new("engine-tabs")
                            .segmented()
                            .small()
                            .w(px(280.))
                            .selected_index(selected_index)
                            .children([
                                Tab::new().label("Alva"),
                                Tab::new().label("Claude"),
                                Tab::new().label("Codex"),
                            ])
                            .on_click(move |index, window, app| {
                                let Some(engine_id) = ENGINES.get(*index) else {
                                    return;
                                };
                                if let Some(entity) = weak_engine.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.select_engine(engine_id, window, cx);
                                    });
                                }
                            }),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("details")
                            .ghost()
                            .icon(IconName::PanelRightOpen)
                            .label("Details")
                            .tooltip("Open session memory, attempts and diagnostics")
                            .disabled(!has_session)
                            .on_click(move |_, window, app| {
                                if let Some(entity) = weak_details.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.open_details_sheet(window, cx);
                                    });
                                }
                            }),
                    )
                    .child(
                        Tag::new()
                            .with_variant(super::timeline_tag_variant(timeline_status))
                            .rounded_full()
                            .child(div().text_sm().child(status)),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(self.label_small("Permissions", cx))
                    .child(
                        Select::new(&self.permission)
                            .small()
                            .w(px(220.))
                            .disabled(has_session),
                    )
                    .when(self.pending_engine.is_some(), |this| {
                        this.child(
                            Tag::warning()
                                .small()
                                .rounded_full()
                                .child("Engine switch queued"),
                        )
                    })
                    .when(has_session, |this| {
                        this.child(
                            Checkbox::new("transcript-toggle")
                                .label("Attach filtered transcript to future handoffs")
                                .checked(self.transcript_sharing)
                                .on_click(move |checked, _, app| {
                                    if let Some(entity) = weak_transcript.upgrade() {
                                        entity.update(app, |hub, cx| {
                                            hub.toggle_transcript_sharing(*checked, cx);
                                        });
                                    }
                                }),
                        )
                    })
                    .child(
                        Switch::new("inspector-experiment")
                            .small()
                            .label("Inspector")
                            .tooltip("Show the experimental execution Inspector")
                            .checked(self.inspector_enabled)
                            .on_click(move |enabled, _, app| {
                                if let Some(entity) = weak_inspector.upgrade() {
                                    entity.update(app, |hub, cx| {
                                        hub.toggle_inspector(*enabled, cx);
                                    });
                                }
                            }),
                    )
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
    }
}
