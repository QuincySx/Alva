// INPUT:  HubApp engine/permission selection, readiness, eligibility
// OUTPUT: compact Cindy-style conversation title bar and tool-workspace controls
// POS:    Keeps configuration in the composer/settings instead of a dashboard header.

use gpui::*;
use gpui_component::{
    button::{Button, ButtonVariants as _},
    ActiveTheme, *,
};

use super::app::HubApp;
use super::components::HubWorkbenchTabMenu;

impl HubApp {
    pub fn topbar(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let has_session = self.active_session_id.is_some();
        if !has_session {
            return div().into_any_element();
        }
        let selected_id = self
            .pending_engine
            .clone()
            .unwrap_or_else(|| self.selected_engine(cx));
        let title = self
            .active_session_id
            .as_deref()
            .and_then(|active_id| {
                self.sessions
                    .iter()
                    .find(|session| session.product_session_id == active_id)
            })
            .and_then(|session| session.last_prompt.clone())
            .unwrap_or_else(|| "New conversation".to_string());
        let workspace = self
            .workspace
            .as_deref()
            .and_then(|workspace| workspace.split('/').next_back())
            .unwrap_or("No project");
        let weak_details = cx.entity().downgrade();
        h_flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(cx.theme().colors.border)
            .bg(cx.theme().colors.background)
            .child(
                v_flex()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .max_w(px(520.))
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_sm()
                            .font_weight(FontWeight(650.))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(format!("{workspace} · {selected_id}")),
                    ),
            )
            .child(div().flex_1())
            .child(
                Button::new("details")
                    .ghost()
                    .small()
                    .icon(IconName::Info)
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
            .child(HubWorkbenchTabMenu::new(cx.entity().downgrade()))
            .into_any_element()
    }
}
