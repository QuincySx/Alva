//! User input prompt widget.
//!
//! Renders a mode-aware input line with a prefix that changes based on the
//! active input mode, plus a footer with model name and token count.

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use super::theme::Theme;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Active editing / command mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    /// Default conversational input.
    Normal,
    /// Slash-command mode (`/`).
    Command,
    /// Shell pass-through (`!`).
    Shell,
    /// Vim-style command (`:`) — reserved.
    Vim,
}

impl InputMode {
    /// Single-character prefix shown before the cursor.
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Normal => "> ",
            Self::Command => "/ ",
            Self::Shell => "! ",
            Self::Vim => ": ",
        }
    }

    /// Short label for the status bar.
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Command => "COMMAND",
            Self::Shell => "SHELL",
            Self::Vim => "VIM",
        }
    }
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// Prompt input area with mode prefix, editable text, and status footer.
pub struct PromptInputWidget<'a> {
    /// Current input buffer contents.
    input: &'a str,
    /// Cursor column position within `input`.
    cursor_col: usize,
    /// Active input mode.
    mode: InputMode,
    /// Model identifier shown in the footer (e.g. "gpt-4o").
    model_name: &'a str,
    /// Token usage counter displayed in the footer.
    token_count: u32,
    theme: &'a Theme,
}

impl<'a> PromptInputWidget<'a> {
    pub fn new(input: &'a str, theme: &'a Theme) -> Self {
        Self {
            input,
            cursor_col: input.len(),
            mode: InputMode::Normal,
            model_name: "",
            token_count: 0,
            theme,
        }
    }

    pub fn cursor(mut self, col: usize) -> Self {
        self.cursor_col = col;
        self
    }

    pub fn mode(mut self, mode: InputMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn model_name(mut self, name: &'a str) -> Self {
        self.model_name = name;
        self
    }

    pub fn token_count(mut self, count: u32) -> Self {
        self.token_count = count;
        self
    }
}

impl<'a> Widget for PromptInputWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Split: input row(s) on top, 1-line footer on bottom.
        let chunks = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

        let input_area = chunks[0];
        let footer_area = chunks[1];

        // -- Input line --
        let prefix = self.mode.prefix();
        let input_line = Line::from(vec![
            Span::styled(prefix, self.theme.prompt),
            Span::styled(self.input.to_owned(), self.theme.text),
        ]);

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(self.theme.border);

        let paragraph = Paragraph::new(input_line).block(block);
        paragraph.render(input_area, buf);

        // -- Footer (model + tokens) --
        let footer = Line::from(vec![
            Span::styled(
                format!(" {} ", self.mode.label()),
                self.theme.status_bar,
            ),
            Span::styled(
                format!(" {} ", self.model_name),
                self.theme.text_dim,
            ),
            Span::styled(
                format!(" {}T ", self.token_count),
                self.theme.text_dim,
            ),
        ]);
        let footer_paragraph = Paragraph::new(footer);
        footer_paragraph.render(footer_area, buf);
    }
}
