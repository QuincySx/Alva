// INPUT:  gpui, gpui_component (Button), crate::theme::Theme
// OUTPUT: pub fn render_management_buttons()
// POS:    Renders Agents and Skills management buttons for the sidebar.
use gpui::*;

use gpui_component::button::Button;

use crate::theme::Theme;

/// Renders the Agents and Skills management buttons.
/// These will open dialogs in Tasks 9 and 10; for now they log on click.
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
                .on_click(srow_debug::traced!("sidebar:agents_btn", move |_, _, _| {
                    tracing::info!("open agents dialog");
                })),
        )
        .child(
            Button::new("skills-btn")
                .label("Skills")
                .outline()
                .on_click(srow_debug::traced!("sidebar:skills_btn", move |_, _, _| {
                    tracing::info!("open skills dialog");
                })),
        )
}
