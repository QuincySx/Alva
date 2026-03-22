// INPUT:  gpui (div, px, IntoElement, ParentElement, Styled, FontWeight, Hsla),
//         gpui::prelude::FluentBuilder, srow_core::ui_message::parts::ToolState,
//         crate::theme::Theme
// OUTPUT: pub fn render_tool_call
// POS:    Stateless render helper for collapsible tool call blocks.
//         Collapse state is owned by the parent; click handling is added by the parent.

use gpui::{div, px, FontWeight, Hsla, IntoElement, ParentElement, Styled};
use gpui::prelude::FluentBuilder;
use srow_core::ui_message::ToolState;

use crate::theme::Theme;

/// Renders a tool call block.
///
/// `collapsed` state is owned by the parent. The parent wraps the returned element
/// in a clickable container to toggle the collapse state.
pub fn render_tool_call(
    tool_name: &str,
    input: &serde_json::Value,
    state: &ToolState,
    output: Option<&serde_json::Value>,
    error: Option<&str>,
    collapsed: bool,
    theme: &Theme,
) -> impl IntoElement {
    let is_running = matches!(
        state,
        ToolState::InputStreaming
            | ToolState::InputAvailable
            | ToolState::ApprovalRequested
            | ToolState::ApprovalResponded
    );
    let is_error = matches!(state, ToolState::OutputError | ToolState::OutputDenied);
    let is_success = matches!(state, ToolState::OutputAvailable);

    // Status indicator colors
    let (status_icon, status_text, status_color) = if is_running {
        let icon = match state {
            ToolState::ApprovalRequested => "?",
            _ => "\u{27F3}", // ⟳
        };
        let text = match state {
            ToolState::InputStreaming => "streaming input...",
            ToolState::InputAvailable => "ready",
            ToolState::ApprovalRequested => "approval needed",
            ToolState::ApprovalResponded => "approved",
            _ => "",
        };
        (icon, text, Hsla::from(theme.warning))
    } else if is_success {
        ("\u{2713}", "done", Hsla::from(theme.success)) // ✓
    } else {
        ("\u{2717}", if is_error { "error" } else { "denied" }, Hsla::from(theme.error)) // ✗
    };

    // Border color based on state
    let border_color = if is_running {
        Hsla::from(theme.warning).opacity(0.3)
    } else if is_success {
        Hsla::from(theme.success).opacity(0.3)
    } else {
        Hsla::from(theme.error).opacity(0.3)
    };

    let mut block = div()
        .w_full()
        .max_w(px(600.))
        .rounded_lg()
        .border_1()
        .border_color(border_color)
        .bg(theme.background)
        .overflow_hidden();

    // -- Header row --
    let header = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .cursor_pointer()
        .child(
            div()
                .text_sm()
                .text_color(status_color)
                .child(status_icon.to_string()),
        )
        .child(
            div()
                .flex_1()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child(format!("Tool: {}", tool_name)),
        )
        .child(
            div()
                .text_xs()
                .text_color(status_color)
                .child(status_text.to_string()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.text_muted)
                .child(if collapsed { "\u{25B6}" } else { "\u{25BC}" }), // ▶ / ▼
        );

    block = block.child(header);

    // -- Expanded content --
    if !collapsed {
        // Input section
        let input_str = serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string());
        let truncated_input = if input_str.len() > 500 {
            let end = input_str
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i <= 500)
                .last()
                .unwrap_or(0);
            format!("{}...", &input_str[..end])
        } else {
            input_str
        };

        let content = div()
            .px_3()
            .pb_2()
            .border_t_1()
            .border_color(Hsla::from(theme.border).opacity(0.5))
            .child(
                div()
                    .mt_2()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.text_muted)
                    .child("Input"),
            )
            .child(
                div()
                    .mt_1()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(theme.surface)
                    .text_xs()
                    .text_color(theme.text_muted)
                    .max_h(px(150.))
                    .overflow_hidden()
                    .child(truncated_input),
            )
            .when(output.is_some() || error.is_some(), |el| {
                let section_label = if is_error { "Error" } else { "Output" };
                let output_text = if let Some(err) = error {
                    err.to_string()
                } else if let Some(out) = output {
                    if let Some(s) = out.as_str() {
                        s.to_string()
                    } else {
                        serde_json::to_string_pretty(out).unwrap_or_else(|_| out.to_string())
                    }
                } else {
                    String::new()
                };

                let truncated_output = if output_text.len() > 500 {
                    let end = output_text
                        .char_indices()
                        .map(|(i, _)| i)
                        .take_while(|&i| i <= 500)
                        .last()
                        .unwrap_or(0);
                    format!("{}...", &output_text[..end])
                } else {
                    output_text
                };

                let text_color = if is_error {
                    Hsla::from(theme.error)
                } else {
                    Hsla::from(theme.text_muted)
                };

                el.child(
                    div()
                        .mt_2()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.text_muted)
                        .child(section_label.to_string()),
                )
                .child(
                    div()
                        .mt_1()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .bg(theme.surface)
                        .text_xs()
                        .text_color(text_color)
                        .max_h(px(200.))
                        .overflow_hidden()
                        .child(truncated_output),
                )
            });

        block = block.child(content);
    }

    block
}
