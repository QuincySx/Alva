// INPUT:  ratatui (Frame, Rect, Paragraph, Block, Borders), super::theme
// OUTPUT: Attachment, AttachmentStrip
// POS:    A horizontal strip of pending file/image attachments shown
//         above the chat input. Each entry is a chip with type icon +
//         filename + tap-to-remove (callers handle the remove via
//         keyboard / index since terminals don't have real clicks).

use std::path::PathBuf;

use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use super::super::theme::Theme;

#[derive(Debug, Clone)]
pub struct Attachment {
    pub path: PathBuf,
    pub kind: AttachmentKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Image,
    File,
}

impl Attachment {
    pub fn auto(path: PathBuf) -> Self {
        let kind = match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => AttachmentKind::Image,
            _ => AttachmentKind::File,
        };
        Self { path, kind }
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().and_then(|s| s.to_str()).unwrap_or("?").to_string()
    }
}

#[derive(Default)]
pub struct AttachmentStrip {
    items: Vec<Attachment>,
    /// Cursor for keyboard removal (Backspace removes the selected one).
    selected: usize,
}

impl AttachmentStrip {
    pub fn new() -> Self { Self::default() }

    pub fn push(&mut self, a: Attachment) { self.items.push(a); }
    pub fn clear(&mut self) { self.items.clear(); self.selected = 0; }
    pub fn is_empty(&self) -> bool { self.items.is_empty() }
    pub fn items(&self) -> &[Attachment] { &self.items }

    pub fn select_next(&mut self) {
        if !self.items.is_empty() { self.selected = (self.selected + 1) % self.items.len(); }
    }
    pub fn select_prev(&mut self) {
        if !self.items.is_empty() {
            self.selected = if self.selected == 0 { self.items.len() - 1 } else { self.selected - 1 };
        }
    }
    /// Remove the selected attachment. Returns the removed entry.
    pub fn remove_selected(&mut self) -> Option<Attachment> {
        if self.items.is_empty() { return None; }
        let i = self.selected.min(self.items.len() - 1);
        let removed = self.items.remove(i);
        if self.selected >= self.items.len() {
            self.selected = self.items.len().saturating_sub(1);
        }
        Some(removed)
    }

    /// Drain attachments — use when sending the message.
    pub fn drain(&mut self) -> Vec<Attachment> {
        self.selected = 0;
        std::mem::take(&mut self.items)
    }

    pub fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        if self.items.is_empty() || area.height == 0 { return; }

        let mut spans: Vec<Span> = Vec::with_capacity(self.items.len() * 4);
        for (i, a) in self.items.iter().enumerate() {
            let icon = match a.kind {
                AttachmentKind::Image => "🖼 ",
                AttachmentKind::File  => "📄 ",
            };
            let label = format!(" {}{} ", icon, a.file_name());
            let style = if i == self.selected {
                theme.selection.add_modifier(Modifier::REVERSED)
            } else {
                theme.tool_name
            };
            spans.push(Span::styled(label, style));
            spans.push(Span::raw(" "));
        }

        let line = Line::from(spans);
        let para = Paragraph::new(line).block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(theme.border)
                .title(format!(" Attachments · {} ", self.items.len())),
        );
        frame.render_widget(para, area);
    }
}
