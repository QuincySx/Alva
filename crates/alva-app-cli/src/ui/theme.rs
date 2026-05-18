//! Terminal UI theme with dark/light mode support.
//!
//! Defines a [`Theme`] struct containing [`Style`] fields for every visual
//! element in the TUI — text roles, tool status indicators, borders,
//! code-block syntax highlighting, and status bar.

use ratatui::style::{Color, Modifier, Style};

/// Complete visual theme for the terminal UI.
#[derive(Debug, Clone)]
pub struct Theme {
    // -- text roles --
    pub text: Style,
    pub text_dim: Style,
    pub text_bold: Style,

    // -- message roles --
    pub user_text: Style,
    pub assistant_text: Style,
    pub system_text: Style,
    pub error_text: Style,

    // -- tool indicators --
    pub tool_name: Style,
    pub tool_running: Style,
    pub tool_success: Style,
    pub tool_error: Style,

    // -- chrome --
    pub border: Style,
    pub border_focused: Style,
    pub selection: Style,
    pub prompt: Style,
    pub status_bar: Style,

    // -- code highlighting --
    pub code_block_bg: Style,
    pub code_keyword: Style,
    pub code_string: Style,
    pub code_comment: Style,
}

/// Which colour scheme to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Dark,
    Light,
}

impl Theme {
    /// Build a theme for the requested mode.
    pub fn new(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self::dark(),
            ThemeMode::Light => Self::light(),
        }
    }

    /// Dark theme — light text on dark background.
    pub fn dark() -> Self {
        Self {
            // text
            text: Style::default().fg(Color::White),
            text_dim: Style::default().fg(Color::DarkGray),
            text_bold: Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),

            // message roles
            user_text: Style::default().fg(Color::Cyan),
            assistant_text: Style::default().fg(Color::Green),
            system_text: Style::default().fg(Color::Yellow),
            error_text: Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),

            // tool
            tool_name: Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            tool_running: Style::default().fg(Color::Yellow),
            tool_success: Style::default().fg(Color::Green),
            tool_error: Style::default().fg(Color::Red),

            // chrome
            border: Style::default().fg(Color::DarkGray),
            border_focused: Style::default().fg(Color::Cyan),
            selection: Style::default()
                .bg(Color::DarkGray)
                .fg(Color::White),
            prompt: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            status_bar: Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray),

            // code
            code_block_bg: Style::default().bg(Color::Rgb(30, 30, 46)),
            code_keyword: Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            code_string: Style::default().fg(Color::Green),
            code_comment: Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        }
    }

    /// Light theme — dark text on light background.
    pub fn light() -> Self {
        Self {
            // text
            text: Style::default().fg(Color::Black),
            text_dim: Style::default().fg(Color::Gray),
            text_bold: Style::default()
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),

            // message roles
            user_text: Style::default().fg(Color::Blue),
            assistant_text: Style::default().fg(Color::Rgb(0, 128, 0)),
            system_text: Style::default().fg(Color::Rgb(180, 120, 0)),
            error_text: Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),

            // tool
            tool_name: Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            tool_running: Style::default().fg(Color::Rgb(180, 120, 0)),
            tool_success: Style::default().fg(Color::Rgb(0, 128, 0)),
            tool_error: Style::default().fg(Color::Red),

            // chrome
            border: Style::default().fg(Color::Gray),
            border_focused: Style::default().fg(Color::Blue),
            selection: Style::default()
                .bg(Color::LightYellow)
                .fg(Color::Black),
            prompt: Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            status_bar: Style::default()
                .fg(Color::Black)
                .bg(Color::Gray),

            // code
            code_block_bg: Style::default().bg(Color::Rgb(240, 240, 240)),
            code_keyword: Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
            code_string: Style::default().fg(Color::Rgb(0, 128, 0)),
            code_comment: Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::ITALIC),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    //! Tests for Theme contracts that callers depend on:
    //!   * `Theme::default()` returns the dark variant (changing it
    //!     would silently change the whole CLI's default look)
    //!   * `Theme::new(mode)` routes to the right variant
    //!   * UX affordances: error_text in BOTH themes is BOLD so users
    //!     can spot it
    //!   * light/dark are visibly DIFFERENT (anything that accidentally
    //!     makes one a copy of the other defeats theme switching)
    //!   * tool status colors are mutually distinct (Running / Success
    //!     / Error must not collide)
    use super::*;

    // -- Default + mode routing -------------------------------------------

    #[test]
    fn default_returns_dark_variant() {
        // Pin: Default = dark. Tauri Inspector and other callers rely
        // on this implicit default.
        let d = Theme::default();
        let dark = Theme::dark();
        assert_eq!(d.text.fg, dark.text.fg, "default text fg must match dark");
        assert_eq!(d.text_dim.fg, dark.text_dim.fg);
    }

    #[test]
    fn new_dark_returns_dark_variant() {
        let t = Theme::new(ThemeMode::Dark);
        let dark = Theme::dark();
        assert_eq!(t.user_text.fg, dark.user_text.fg);
        assert_eq!(t.assistant_text.fg, dark.assistant_text.fg);
    }

    #[test]
    fn new_light_returns_light_variant() {
        let t = Theme::new(ThemeMode::Light);
        let light = Theme::light();
        assert_eq!(t.user_text.fg, light.user_text.fg);
        assert_eq!(t.assistant_text.fg, light.assistant_text.fg);
    }

    // -- UX accessibility pins --------------------------------------------

    #[test]
    fn error_text_is_bold_in_both_themes() {
        // Pin: errors must stand out visually. If a refactor drops
        // the BOLD modifier, users may miss error messages on a
        // chatty stream.
        assert!(
            Theme::dark().error_text.add_modifier.contains(Modifier::BOLD),
            "dark.error_text must be BOLD"
        );
        assert!(
            Theme::light().error_text.add_modifier.contains(Modifier::BOLD),
            "light.error_text must be BOLD"
        );
    }

    #[test]
    fn tool_name_is_bold_in_both_themes() {
        // Same affordance for tool invocations — bold so users see
        // "the agent ran X".
        assert!(Theme::dark().tool_name.add_modifier.contains(Modifier::BOLD));
        assert!(Theme::light().tool_name.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn prompt_is_bold_in_both_themes() {
        assert!(Theme::dark().prompt.add_modifier.contains(Modifier::BOLD));
        assert!(Theme::light().prompt.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn code_comment_is_italic_in_both_themes() {
        assert!(Theme::dark().code_comment.add_modifier.contains(Modifier::ITALIC));
        assert!(Theme::light().code_comment.add_modifier.contains(Modifier::ITALIC));
    }

    // -- Light vs dark distinctness ---------------------------------------

    #[test]
    fn light_and_dark_differ_in_default_text_color() {
        // Pin: dark = White text, light = Black text. If a future
        // refactor accidentally sets them the same, theme switching
        // becomes a no-op (silent UX regression).
        assert_ne!(
            Theme::dark().text.fg,
            Theme::light().text.fg,
            "dark.text.fg and light.text.fg must differ"
        );
    }

    #[test]
    fn light_and_dark_differ_in_user_text_color() {
        // Pin per-role distinctness — the user_text accent
        // (Cyan vs Blue here) must differ across modes.
        assert_ne!(
            Theme::dark().user_text.fg,
            Theme::light().user_text.fg,
        );
    }

    // -- Tool status palette distinctness ---------------------------------

    #[test]
    fn tool_status_colors_are_mutually_distinct_in_dark() {
        // Running / Success / Error MUST have different foreground
        // colors in each theme — otherwise a user can't tell from
        // color alone which state a tool is in.
        let t = Theme::dark();
        assert_ne!(t.tool_running.fg, t.tool_success.fg);
        assert_ne!(t.tool_running.fg, t.tool_error.fg);
        assert_ne!(t.tool_success.fg, t.tool_error.fg);
    }

    #[test]
    fn tool_status_colors_are_mutually_distinct_in_light() {
        let t = Theme::light();
        assert_ne!(t.tool_running.fg, t.tool_success.fg);
        assert_ne!(t.tool_running.fg, t.tool_error.fg);
        assert_ne!(t.tool_success.fg, t.tool_error.fg);
    }
}
