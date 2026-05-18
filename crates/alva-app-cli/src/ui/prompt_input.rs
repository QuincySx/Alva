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

#[cfg(test)]
mod tests {
    //! Tests for InputMode (prefix/label routing) + PromptInputWidget
    //! render (mode prefix, input text, footer with mode/model/tokens).
    //! Buffer pattern from ui::spinner (L107) / ui::message_list (L108).
    use super::*;

    fn theme() -> Theme {
        Theme::default()
    }

    fn render_to_string(widget: PromptInputWidget<'_>, width: u16, height: u16) -> String {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);
        let mut s = String::new();
        for y in 0..height {
            for x in 0..width {
                s.push_str(buf[(x, y)].symbol());
            }
            s.push('\n');
        }
        s
    }

    // -- InputMode::prefix per variant ------------------------------------

    #[test]
    fn prefix_normal_is_angle_bracket() {
        assert_eq!(InputMode::Normal.prefix(), "> ");
    }

    #[test]
    fn prefix_command_is_slash() {
        assert_eq!(InputMode::Command.prefix(), "/ ");
    }

    #[test]
    fn prefix_shell_is_bang() {
        assert_eq!(InputMode::Shell.prefix(), "! ");
    }

    #[test]
    fn prefix_vim_is_colon() {
        assert_eq!(InputMode::Vim.prefix(), ": ");
    }

    #[test]
    fn prefix_variants_are_mutually_distinct() {
        // Pin: users distinguish modes ONLY by this prefix; collapsing
        // any two to the same string makes mode-switching invisible.
        use std::collections::HashSet;
        let prefixes: HashSet<_> = [
            InputMode::Normal.prefix(),
            InputMode::Command.prefix(),
            InputMode::Shell.prefix(),
            InputMode::Vim.prefix(),
        ]
        .into_iter()
        .collect();
        assert_eq!(prefixes.len(), 4, "all 4 prefixes must be unique");
    }

    // -- InputMode::label per variant -------------------------------------

    #[test]
    fn label_strings_match_each_mode() {
        assert_eq!(InputMode::Normal.label(), "NORMAL");
        assert_eq!(InputMode::Command.label(), "COMMAND");
        assert_eq!(InputMode::Shell.label(), "SHELL");
        assert_eq!(InputMode::Vim.label(), "VIM");
    }

    // -- PromptInputWidget builders ---------------------------------------

    #[test]
    fn new_defaults_cursor_to_input_len_and_mode_to_normal() {
        // Builder default contract: cursor lands at end of input, mode
        // Normal, no model name, 0 tokens. Render reflects all of these.
        let theme = theme();
        let s = render_to_string(PromptInputWidget::new("hi", &theme), 40, 4);
        assert!(s.contains("> "), "default mode must render '> ' prefix: {s}");
        assert!(s.contains("hi"), "input text must appear: {s}");
        assert!(s.contains("NORMAL"), "default mode label must be NORMAL: {s}");
        // Default model_name is "" — still shows the trailing space chrome
        // around it but no specific name to check.
        assert!(s.contains("0T"), "default token_count=0 must show '0T': {s}");
    }

    // -- Render: mode prefix + input text ---------------------------------

    #[test]
    fn render_uses_command_prefix_when_mode_command() {
        let theme = theme();
        let s = render_to_string(
            PromptInputWidget::new("help", &theme).mode(InputMode::Command),
            40,
            4,
        );
        // Slash prefix at the input line.
        assert!(s.contains("/ help"), "command-mode line must show '/ help': {s}");
        assert!(s.contains("COMMAND"), "footer must show COMMAND label: {s}");
    }

    #[test]
    fn render_uses_shell_prefix_when_mode_shell() {
        let theme = theme();
        let s = render_to_string(
            PromptInputWidget::new("ls", &theme).mode(InputMode::Shell),
            40,
            4,
        );
        assert!(s.contains("! ls"));
        assert!(s.contains("SHELL"));
    }

    #[test]
    fn render_uses_vim_prefix_when_mode_vim() {
        let theme = theme();
        let s = render_to_string(
            PromptInputWidget::new("q", &theme).mode(InputMode::Vim),
            40,
            4,
        );
        assert!(s.contains(": q"));
        assert!(s.contains("VIM"));
    }

    // -- Render: footer (model + tokens) ----------------------------------

    #[test]
    fn render_footer_includes_model_name() {
        let theme = theme();
        let s = render_to_string(
            PromptInputWidget::new("", &theme).model_name("claude-sonnet-4-5"),
            60,
            4,
        );
        assert!(
            s.contains("claude-sonnet-4-5"),
            "footer must include model name: {s}"
        );
    }

    #[test]
    fn render_footer_includes_token_count_with_t_suffix() {
        // Pin: footer renders "<N>T" — the "T" suffix tells the user
        // the number is tokens (not messages / lines). Dropping the
        // suffix makes the number ambiguous.
        let theme = theme();
        let s = render_to_string(
            PromptInputWidget::new("", &theme).token_count(12345),
            60,
            4,
        );
        assert!(s.contains("12345T"), "footer must show '12345T': {s}");
    }

    #[test]
    fn render_footer_includes_all_three_segments_at_once() {
        // Compose all three: mode label + model + tokens. Pin that
        // the three Spans don't accidentally collapse (e.g. format
        // refactor losing one).
        let theme = theme();
        let s = render_to_string(
            PromptInputWidget::new("", &theme)
                .mode(InputMode::Command)
                .model_name("gpt-4o")
                .token_count(42),
            70,
            4,
        );
        assert!(s.contains("COMMAND"));
        assert!(s.contains("gpt-4o"));
        assert!(s.contains("42T"));
    }
}
