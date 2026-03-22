// INPUT:  gpui, crate::theme
// OUTPUT: pub fn render_tool_call (placeholder)
// POS:    Stateless render helper for tool call blocks.
//         Implementation commented out — depends on deleted ToolState from ui_message.
//         TODO: Rebuild on agent-core tool types.

use gpui::{div, IntoElement, ParentElement};

use crate::theme::Theme;

/// Renders a tool call block (placeholder during migration).
pub fn render_tool_call(
    tool_name: &str,
    _theme: &Theme,
) -> impl IntoElement {
    div()
        .child(format!("Tool: {} (TODO: rebuild)", tool_name))
}
