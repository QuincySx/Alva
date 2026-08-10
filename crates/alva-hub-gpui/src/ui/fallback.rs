// INPUT:  HubApp.fallback_policy
// OUTPUT: sidebar "Fallback chain" panel — engine catalog toggles, editable
//         model/provider per target, authorize checkbox, ordering and save.
// POS:    1:1 GPUI port of the React fallback panel in App.tsx.
//         NOTE: implemented in a later batch; placeholder here.

use gpui::*;
use gpui_component::*;

use super::app::HubApp;

impl HubApp {
    pub fn fallback_panel(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(cx.theme().colors.border)
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight(650.))
                            .text_color(cx.theme().colors.muted_foreground)
                            .child("Fallback chain"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(self.fallback_policy.targets.len().to_string()),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child("Only checked targets may run automatically."),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(if self.fallback_policy.targets.is_empty() {
                        "Default chain: none configured.".to_string()
                    } else {
                        self.fallback_policy
                            .targets
                            .iter()
                            .map(|target| target.engine_id.clone())
                            .collect::<Vec<_>>()
                            .join(" → ")
                    }),
            )
    }
}
