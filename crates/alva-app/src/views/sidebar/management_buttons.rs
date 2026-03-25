// INPUT:  gpui, gpui_component (Button), crate::theme::Theme, crate::views::dialogs
// OUTPUT: pub fn render_management_buttons()
// POS:    Renders Agents and Skills management buttons for the sidebar, wired to open dialogs.
use gpui::*;

use gpui_component::button::Button;

use crate::theme::Theme;
use crate::views::dialogs;

/// Renders the Agents and Skills management buttons.
/// Clicking opens the corresponding dialog via gpui-component's Dialog API.
pub fn render_management_buttons(theme: &Theme) -> impl IntoElement {
    let text_muted = theme.text_muted;

    div()
        .flex()
        .flex_col()
        .gap_1()
        .px_2()
        .pb_1()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(text_muted)
                .pb(px(2.))
                .child("Manage"),
        )
        .child(
            Button::new("agents-btn")
                .label("Agents")
                .outline()
                .on_click(alva_app_debug::traced!("sidebar:agents_btn", move |_, window, cx| {
                    tracing::info!("open agents dialog");
                    dialogs::open_agents_dialog(window, cx);
                })),
        )
        .child(
            Button::new("skills-btn")
                .label("Skills")
                .outline()
                .on_click(alva_app_debug::traced!("sidebar:skills_btn", move |_, window, cx| {
                    tracing::info!("open skills dialog");
                    dialogs::open_skills_dialog(window, cx);
                })),
        )
}
