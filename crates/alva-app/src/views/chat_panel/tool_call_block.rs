// INPUT:  gpui, crate::theme
// OUTPUT: pub fn render_tool_call, pub fn render_tool_result
// POS:    Stateless render helpers for tool call and tool result blocks in the chat panel.

use gpui::{div, px, FontWeight, Hsla, IntoElement, ParentElement, Styled};
use gpui::prelude::FluentBuilder;

use crate::theme::Theme;

/// Renders a tool call block showing the tool name, input summary, and status.
pub fn render_tool_call(
    tool_name: &str,
    input_summary: Option<&str>,
    is_running: bool,
    theme: &Theme,
) -> impl IntoElement {
    let accent = Hsla::from(theme.accent);

    div()
        .w_full()
        .px_4()
        .py_2()
        .child(
            div()
                .max_w(px(600.))
                .px_3()
                .py_2()
                .rounded_md()
                .bg(accent.opacity(0.08))
                .border_1()
                .border_color(accent.opacity(0.2))
                .text_sm()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.accent)
                                .child(format!("⚙ {}", tool_name)),
                        )
                        .when(is_running, |el| {
                            el.child(
                                div()
                                    .text_xs()
                                    .text_color(theme.text_muted)
                                    .child("running..."),
                            )
                        }),
                )
                .when_some(input_summary, |el, summary| {
                    el.child(
                        div()
                            .mt_1()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .overflow_hidden()
                            .child(summary.to_string()),
                    )
                }),
        )
}

/// Renders a tool result block showing output or error.
pub fn render_tool_result(
    tool_name: &str,
    content: &str,
    is_error: bool,
    theme: &Theme,
) -> impl IntoElement {
    let color = if is_error {
        Hsla::from(theme.error)
    } else {
        Hsla::from(theme.success)
    };

    div()
        .w_full()
        .px_4()
        .py_1()
        .child(
            div()
                .max_w(px(600.))
                .px_3()
                .py_2()
                .rounded_md()
                .bg(color.opacity(0.06))
                .border_1()
                .border_color(color.opacity(0.15))
                .text_sm()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(color)
                        .child(format!(
                            "{} {}",
                            if is_error { "✗" } else { "✓" },
                            tool_name,
                        )),
                )
                .child(
                    div()
                        .mt_1()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .overflow_hidden()
                        .max_h(px(200.))
                        .child(truncate_content(content, 500)),
                ),
        )
}

/// Truncate long content with an ellipsis marker.
fn truncate_content(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        content.to_string()
    } else {
        format!("{}…", &content[..max_chars])
    }
}
