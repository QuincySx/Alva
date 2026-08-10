// INPUT:  HubApp.team_run
// OUTPUT: sidebar "Team" panel — lead/worker cards, task queues, worktree
//         adoption controls, autonomy and archived visibility.
// POS:    1:1 GPUI port of the React team panel in App.tsx.
//         NOTE: implemented in a later batch; placeholder here.

use gpui::*;
use gpui_component::*;

use super::app::HubApp;

impl HubApp {
    pub fn team_panel(&self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                            .child("Team"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().colors.muted_foreground)
                            .child(
                                self.team_run
                                    .as_ref()
                                    .map(|run| format!("{} tasks", run.tasks.len()))
                                    .unwrap_or_default(),
                            ),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().colors.muted_foreground)
                    .child(if self.active_session_id.is_some() {
                        "Team runs arrive in a later batch."
                    } else {
                        "Open a session to lead a team run."
                    }),
            )
    }
}
