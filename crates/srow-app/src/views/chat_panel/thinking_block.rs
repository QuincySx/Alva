// INPUT:  gpui (div, px, IntoElement, ParentElement, Styled, FontWeight, Hsla),
//         gpui::prelude::FluentBuilder, crate::theme::Theme
// OUTPUT: pub fn render_thinking
// POS:    Stateless render helper for collapsible reasoning/thinking blocks.
//         Expanded state is owned by the parent; click handling is added by the parent.

use gpui::{div, px, FontWeight, Hsla, IntoElement, ParentElement, Styled};

use crate::theme::Theme;

/// Renders a thinking/reasoning block.
///
/// `expanded` state is owned by the parent. The parent wraps the returned element
/// in a clickable container to toggle expansion.
pub fn render_thinking(
    text: &str,
    is_streaming: bool,
    expanded: bool,
    theme: &Theme,
) -> impl IntoElement {
    let border_color = Hsla::from(theme.border).opacity(0.5);
    let text_muted = theme.text_muted;

    let header_label = if is_streaming {
        "Thinking..."
    } else {
        "Thought"
    };

    let mut block = div()
        .w_full()
        .max_w(px(600.))
        .rounded_lg()
        .border_1()
        .border_color(border_color)
        .bg(theme.background)
        .overflow_hidden();

    // -- Header --
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
                .text_color(text_muted)
                .child("\u{1F4AD}"), // 💭
        )
        .child(
            div()
                .flex_1()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(text_muted)
                .child(header_label.to_string()),
        )
        .child(
            div()
                .text_xs()
                .text_color(text_muted)
                .child(if expanded { "\u{25BC}" } else { "\u{25B6}" }), // ▼ / ▶
        );

    block = block.child(header);

    // -- Expanded content --
    if expanded {
        block = block.child(
            div()
                .px_3()
                .pb_2()
                .border_t_1()
                .border_color(border_color)
                .child(
                    div()
                        .mt_2()
                        .text_xs()
                        .italic()
                        .text_color(text_muted)
                        .max_h(px(300.))
                        .overflow_hidden()
                        .child(text.to_string()),
                ),
        );
    }

    block
}
