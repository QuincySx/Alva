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
