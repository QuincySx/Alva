// INPUT:  gpui (div, px, IntoElement, ParentElement, Styled, FontWeight, Hsla, FluentBuilder), crate::theme::Theme
// OUTPUT: pub fn render_completed_agent, pub fn render_running_agent
// POS:    Stateless render helpers for agent blocks in the message stream and running agents zone.

use gpui::{div, px, FontWeight, Hsla, IntoElement, ParentElement, Styled};
use gpui::prelude::FluentBuilder;

use crate::theme::Theme;

/// Renders a completed agent block in the message stream.
///
/// The entire block is clickable; the parent wraps with `on_click` to navigate
/// to the agent detail panel.
pub fn render_completed_agent(
    agent_name: &str,
    summary: &str,
    success: bool,
    theme: &Theme,
) -> impl IntoElement {
    let (bg_tint, border_tint, status_icon) = if success {
        (
            Hsla::from(theme.success).opacity(0.0625),
            Hsla::from(theme.success).opacity(0.25),
            "\u{2713}", // ✓
        )
    } else {
        (
            Hsla::from(theme.error).opacity(0.0625),
            Hsla::from(theme.error).opacity(0.25),
            "\u{2717}", // ✗
        )
    };

    let status_color = if success {
        Hsla::from(theme.success)
    } else {
        Hsla::from(theme.error)
    };

    div()
        .w_full()
        .max_w(px(600.))
        .rounded_lg()
        .border_1()
        .border_color(border_tint)
        .bg(bg_tint)
        .cursor_pointer()
        .overflow_hidden()
        // Header
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.text)
                        .child("\u{1F916}"), // 🤖
                )
                .child(
                    div()
                        .flex_1()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(agent_name.to_string()),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(status_color)
                        .child(status_icon.to_string()),
                ),
        )
        // Summary body
        .child(
            div()
                .px_3()
                .pb_2()
                .text_xs()
                .text_color(theme.text_muted)
                .child(summary.to_string()),
        )
        // Footer
        .child(
            div()
                .flex()
                .flex_row_reverse()
                .px_3()
                .pb_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.accent)
                        .child("Details \u{2192}"), // →
                ),
        )
}

/// Renders a running agent block (for the Running Agents Zone).
///
/// The entire block is clickable; the parent wraps with `on_click`.
pub fn render_running_agent(
    agent_name: &str,
    progress: &str,
    current_skill: Option<&str>,
    theme: &Theme,
) -> impl IntoElement {
    let accent_color = Hsla::from(theme.accent);

    div()
        .w_full()
        .max_w(px(600.))
        .rounded_lg()
        .border_1()
        .border_color(accent_color.opacity(0.3))
        .bg(accent_color.opacity(0.0625))
        .cursor_pointer()
        .overflow_hidden()
        // Main row: spinner + name + progress
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .child(
                    div()
                        .text_sm()
                        .text_color(theme.accent)
                        .child("\u{27F3}"), // ⟳
                )
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text)
                        .child(agent_name.to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(progress.to_string()),
                ),
        )
        // Current skill (optional)
        .when(current_skill.is_some(), |el| {
            let skill_name = current_skill.unwrap_or_default().to_string();
            el.child(
                div()
                    .px_3()
                    .pb_2()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child(format!("skill: {}", skill_name)),
            )
        })
}
