//! Permission dialog widget.
//!
//! Centred modal that asks the user to approve or deny a tool invocation
//! before it executes (bash command, file edit, web fetch, etc.).

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

use super::theme::Theme;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Category of permission being requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionType {
    Bash,
    FileEdit,
    FileWrite,
    WebFetch,
    FileRead,
}

impl PermissionType {
    /// Human-readable label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Bash => "Bash Command",
            Self::FileEdit => "File Edit",
            Self::FileWrite => "File Write",
            Self::WebFetch => "Web Fetch",
            Self::FileRead => "File Read",
        }
    }

    /// Icon for the permission type.
    pub fn icon(self) -> &'static str {
        match self {
            Self::Bash => "\u{f0c8e}", // terminal icon (fallback: $)
            Self::FileEdit => "\u{270f}",
            Self::FileWrite => "\u{1f4be}",
            Self::WebFetch => "\u{1f310}",
            Self::FileRead => "\u{1f4c4}",
        }
    }
}

// ---------------------------------------------------------------------------
// Widget
// ---------------------------------------------------------------------------

/// Modal dialog requesting permission for a tool action.
pub struct PermissionDialogWidget<'a> {
    permission_type: PermissionType,
    /// Primary detail (e.g. the command string, file path).
    detail: &'a str,
    /// Optional secondary context (e.g. diff preview).
    context: Option<&'a str>,
    theme: &'a Theme,
}

impl<'a> PermissionDialogWidget<'a> {
    pub fn new(
        permission_type: PermissionType,
        detail: &'a str,
        theme: &'a Theme,
    ) -> Self {
        Self {
            permission_type,
            detail,
            context: None,
            theme,
        }
    }

    pub fn context(mut self, ctx: &'a str) -> Self {
        self.context = Some(ctx);
        self
    }

    /// Compute a centred rectangle within `area`.
    fn centered_rect(area: Rect, width_pct: u16, height_pct: u16) -> Rect {
        let vert = Layout::vertical([
            Constraint::Percentage((100 - height_pct) / 2),
            Constraint::Percentage(height_pct),
            Constraint::Percentage((100 - height_pct) / 2),
        ])
        .split(area);

        let horiz = Layout::horizontal([
            Constraint::Percentage((100 - width_pct) / 2),
            Constraint::Percentage(width_pct),
            Constraint::Percentage((100 - width_pct) / 2),
        ])
        .split(vert[1]);

        horiz[1]
    }
}

impl<'a> Widget for PermissionDialogWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let dialog_area = Self::centered_rect(area, 60, 40);

        // Clear the area behind the dialog.
        Clear.render(dialog_area, buf);

        // -- Build content lines --
        let mut lines: Vec<Line<'_>> = Vec::new();

        // Title line
        lines.push(Line::from(vec![
            Span::raw(self.permission_type.icon()),
            Span::raw(" "),
            Span::styled(
                self.permission_type.label(),
                self.theme
                    .text_bold
                    .add_modifier(Modifier::UNDERLINED),
            ),
        ]));
        lines.push(Line::default());

        // Detail
        for dl in self.detail.lines() {
            lines.push(Line::styled(dl.to_owned(), self.theme.text));
        }

        // Optional context
        if let Some(ctx) = self.context {
            lines.push(Line::default());
            for cl in ctx.lines() {
                lines.push(Line::styled(cl.to_owned(), self.theme.text_dim));
            }
        }

        // Separator & options
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::styled("[y]", self.theme.tool_success),
            Span::raw(" Allow once  "),
            Span::styled("[a]", self.theme.tool_success),
            Span::raw(" Allow always  "),
            Span::styled("[d]", self.theme.tool_error),
            Span::raw(" Deny  "),
            Span::styled("[n]", self.theme.tool_error),
            Span::raw(" Deny always"),
        ]));

        let text = Text::from(lines);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(self.theme.border_focused)
            .title(" Permission Required ")
            .title_alignment(Alignment::Center);

        let paragraph = Paragraph::new(text)
            .block(block)
            .wrap(Wrap { trim: false })
            .alignment(Alignment::Left);

        paragraph.render(dialog_area, buf);
    }
}
