// INPUT:  gpui (div, px, IntoElement, ParentElement, Styled, FontWeight, Hsla, FluentBuilder), crate::theme::Theme, super::markdown
// OUTPUT: pub fn render_user_message, pub fn render_assistant_message, pub fn render_system_message
// POS:    Stateless render helpers for user, assistant, and system message bubbles.

use gpui::{div, px, FontWeight, Hsla, IntoElement, ParentElement, Styled};
use gpui::prelude::FluentBuilder;

use crate::theme::Theme;
use super::markdown::render_markdown;

/// Right-aligned bubble with accent background for user messages.
pub fn render_user_message(text: &str, is_streaming: bool, theme: &Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_row_reverse()
        .w_full()
        .child(
            div()
                .max_w(px(600.))
                .px_4()
                .py_2()
                .rounded_lg()
                .bg(theme.accent)
                .text_color(theme.selected_text)
                .text_sm()
                .child(
                    div()
                        .text_xs()
                        .mb_1()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(Hsla::from(theme.selected_text).opacity(0.8))
                        .child("You"),
                )
                .child(text.to_string())
                .when(is_streaming, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(Hsla::from(theme.selected_text).opacity(0.6))
                            .child(" ..."),
                    )
                }),
        )
}

/// Left-aligned bubble with Markdown rendering for assistant messages.
pub fn render_assistant_message(text: &str, is_streaming: bool, theme: &Theme) -> impl IntoElement {
    let text_muted = theme.text_muted;

    div()
        .w_full()
        .child(
            div()
                .max_w(px(600.))
                .px_4()
                .py_2()
                .rounded_lg()
                .bg(theme.surface)
                .text_color(theme.text)
                .text_sm()
                .child(
                    div()
                        .text_xs()
                        .mb_1()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(text_muted)
                        .child("Assistant"),
                )
                .child(render_markdown(text, theme))
                .when(is_streaming, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(text_muted)
                            .child(" ..."),
                    )
                }),
        )
}

/// Centered muted text for system messages, no bubble background.
pub fn render_system_message(text: &str, theme: &Theme) -> impl IntoElement {
    let error_color = Hsla::from(theme.error);

    div()
        .w_full()
        .flex()
        .justify_center()
        .child(
            div()
                .max_w(px(600.))
                .px_4()
                .py_2()
                .rounded_lg()
                .bg(error_color.opacity(0.125))
                .text_color(error_color)
                .border_1()
                .border_color(error_color.opacity(0.25))
                .text_sm()
                .child(
                    div()
                        .text_xs()
                        .mb_1()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(error_color)
                        .child("System"),
                )
                .child(text.to_string()),
        )
}
