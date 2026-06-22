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
        self.path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string()
    }
}

#[derive(Default)]
pub struct AttachmentStrip {
    items: Vec<Attachment>,
    /// Cursor for keyboard removal (Backspace removes the selected one).
    selected: usize,
}

impl AttachmentStrip {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, a: Attachment) {
        self.items.push(a);
    }
    pub fn clear(&mut self) {
        self.items.clear();
        self.selected = 0;
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn items(&self) -> &[Attachment] {
        &self.items
    }

    pub fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        }
    }
    pub fn select_prev(&mut self) {
        if !self.items.is_empty() {
            self.selected = if self.selected == 0 {
                self.items.len() - 1
            } else {
                self.selected - 1
            };
        }
    }
    /// Remove the selected attachment. Returns the removed entry.
    pub fn remove_selected(&mut self) -> Option<Attachment> {
        if self.items.is_empty() {
            return None;
        }
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
        if self.items.is_empty() || area.height == 0 {
            return;
        }

        let mut spans: Vec<Span> = Vec::with_capacity(self.items.len() * 4);
        for (i, a) in self.items.iter().enumerate() {
            let icon = match a.kind {
                AttachmentKind::Image => "🖼 ",
                AttachmentKind::File => "📄 ",
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

#[cfg(test)]
mod tests {
    //! Unit tests for Attachment + AttachmentStrip. Render is skipped
    //! (pure ratatui passthrough with no branching); we cover the
    //! extension→kind mapping and the cursor state machine instead.
    use super::*;

    fn att(p: &str) -> Attachment {
        Attachment::auto(PathBuf::from(p))
    }

    // ─── Attachment::auto + file_name ──────────────────────────────────

    #[test]
    fn auto_detects_image_extensions() {
        for ext in ["png", "jpg", "jpeg", "gif", "webp", "bmp"] {
            let a = att(&format!("photo.{ext}"));
            assert_eq!(
                a.kind,
                AttachmentKind::Image,
                "lowercase .{ext} must classify as Image"
            );
        }
    }

    #[test]
    fn auto_falls_back_to_file_for_other_extensions_and_no_extension() {
        // Each of these must classify as File
        for path in [
            "notes.txt",
            "readme.md",
            "lib.rs",
            "Makefile",
            "no_extension",
        ] {
            let a = att(path);
            assert_eq!(a.kind, AttachmentKind::File, "{path} must classify as File");
        }
    }

    #[test]
    fn auto_is_case_sensitive_for_extensions() {
        // Pin current behavior: the match is lowercase-literal-only, so
        // `PNG`/`JPG`/etc currently classify as File. If a future PR
        // normalises to ASCII-lowercase before matching, this test will
        // break loudly so the change is deliberate.
        let a = att("PHOTO.PNG");
        assert_eq!(
            a.kind,
            AttachmentKind::File,
            "uppercase .PNG currently classifies as File (case-sensitive match)"
        );
    }

    #[test]
    fn file_name_returns_question_mark_for_pathological_paths() {
        // PathBuf::from("") has no file_name() — guard against panic
        let a = att("");
        assert_eq!(a.file_name(), "?", "empty path must yield '?' placeholder");

        // Trailing-slash dir-style path also has no file_name in std
        let dir = Attachment::auto(PathBuf::from("/some/dir/"));
        // file_name() of "/some/dir/" is "dir" per std::path semantics —
        // pin actual behavior so it's documented
        assert_eq!(dir.file_name(), "dir");
    }

    // ─── AttachmentStrip cursor state machine ──────────────────────────

    #[test]
    fn strip_starts_empty_and_push_appends_in_order() {
        let mut s = AttachmentStrip::new();
        assert!(s.is_empty());
        assert_eq!(s.items().len(), 0);

        s.push(att("a.png"));
        s.push(att("b.txt"));
        assert!(!s.is_empty());
        assert_eq!(s.items().len(), 2);
        assert_eq!(s.items()[0].file_name(), "a.png");
        assert_eq!(s.items()[1].file_name(), "b.txt");
    }

    #[test]
    fn select_next_advances_and_wraps_around() {
        let mut s = AttachmentStrip::new();
        s.push(att("a.png"));
        s.push(att("b.png"));
        s.push(att("c.png"));

        s.select_next();
        s.select_next();
        // selected = 2 (last)
        s.select_next();
        // Wrap to 0
        assert_eq!(s.selected, 0, "select_next at last must wrap to 0");
    }

    #[test]
    fn select_prev_decrements_and_wraps_without_underflow() {
        let mut s = AttachmentStrip::new();
        s.push(att("a.png"));
        s.push(att("b.png"));
        s.push(att("c.png"));

        // selected starts at 0 — prev must wrap to len-1, not underflow
        s.select_prev();
        assert_eq!(s.selected, 2, "select_prev at 0 must wrap to len-1");

        s.select_prev();
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn select_methods_are_no_op_on_empty_strip() {
        let mut s = AttachmentStrip::new();
        // Both must not panic on % 0 / underflow on 0-1
        s.select_next();
        s.select_prev();
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn remove_selected_returns_removed_entry_and_clamps_cursor() {
        let mut s = AttachmentStrip::new();
        s.push(att("a.png"));
        s.push(att("b.png"));
        s.push(att("c.png"));
        s.select_next();
        s.select_next(); // selected = 2 (last)

        let removed = s.remove_selected().expect("remove must return Some");
        assert_eq!(removed.file_name(), "c.png");
        assert_eq!(s.items().len(), 2);
        // After removing last item, selected must clamp to new last (1)
        // — otherwise next render would index OOB
        assert_eq!(s.selected, 1, "cursor must clamp to new last index");

        // Remove middle — selected (1) stays in range, points to what was [2]
        let removed = s.remove_selected().expect("remove must return Some");
        assert_eq!(removed.file_name(), "b.png");
        assert_eq!(s.items().len(), 1);
        assert_eq!(s.selected, 0, "cursor must clamp again");
    }

    #[test]
    fn remove_selected_on_empty_returns_none_without_panic() {
        let mut s = AttachmentStrip::new();
        assert!(s.remove_selected().is_none());
        // Cursor stays at 0 after popping last
        s.push(att("a.png"));
        let _ = s.remove_selected();
        assert!(s.is_empty());
        assert_eq!(
            s.selected, 0,
            "after removing only item, selected must be 0"
        );
        // Removing again is still safe
        assert!(s.remove_selected().is_none());
    }

    #[test]
    fn drain_empties_buffer_and_resets_cursor() {
        let mut s = AttachmentStrip::new();
        s.push(att("a.png"));
        s.push(att("b.png"));
        s.select_next(); // selected = 1

        let drained = s.drain();
        assert_eq!(drained.len(), 2, "drain must return all items");
        assert!(s.is_empty(), "drain must empty the strip");
        assert_eq!(s.selected, 0, "drain must reset cursor to 0");
    }

    #[test]
    fn clear_empties_buffer_and_resets_cursor() {
        let mut s = AttachmentStrip::new();
        s.push(att("a.png"));
        s.push(att("b.png"));
        s.select_next();

        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.selected, 0, "clear must reset cursor to 0");
    }
}
