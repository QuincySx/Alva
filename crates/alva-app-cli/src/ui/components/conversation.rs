// INPUT:  ratatui (Frame, Rect, Block, Borders, Paragraph, Scrollbar),
//         crossterm::Event, super::{collapsible, theme}
// OUTPUT: ConversationItem, ConversationView
// POS:    The middle pane between PinnedHeader and ChatInput. Owns the
//         scrollable list of message bubbles + collapsible blocks. Manages
//         scroll offset, auto-stick-to-bottom, and which collapsible is
//         "focused" for keyboard expand/collapse.

use crossterm::event::{Event, KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};
use ratatui::Frame;

use super::super::theme::Theme;
use super::collapsible::CollapsibleBlock;

/// One row in the conversation. Plain message bubbles render as a single
/// styled paragraph; CollapsibleBlock items can expand into multi-row
/// detail. Each item is independently focusable for keyboard ops.
pub enum ConversationItem {
    Message(MessageBubble),
    Block(CollapsibleBlock),
}

pub struct MessageBubble {
    pub role: BubbleRole,
    pub text: String,
}

#[derive(Debug, Clone, Copy)]
pub enum BubbleRole { User, Assistant, System, Error }

impl MessageBubble {
    pub fn user(text: impl Into<String>) -> Self { Self { role: BubbleRole::User, text: text.into() } }
    pub fn assistant(text: impl Into<String>) -> Self { Self { role: BubbleRole::Assistant, text: text.into() } }
    pub fn system(text: impl Into<String>) -> Self { Self { role: BubbleRole::System, text: text.into() } }
    pub fn error(text: impl Into<String>) -> Self { Self { role: BubbleRole::Error, text: text.into() } }

    fn header(&self) -> &'static str {
        match self.role {
            BubbleRole::User      => "you",
            BubbleRole::Assistant => "alva",
            BubbleRole::System    => "system",
            BubbleRole::Error     => "error",
        }
    }
}

#[derive(Default)]
pub struct ConversationView {
    items: Vec<ConversationItem>,
    /// Top-line offset from the start of the rendered conversation, in rows.
    scroll: u16,
    /// Auto-stick to the bottom on new content (turned off while user
    /// scrolls up; turned back on when scroll reaches the bottom).
    auto_stick: bool,
    /// Currently-focused item index for keyboard expand/collapse.
    focused: Option<usize>,
}

impl ConversationView {
    pub fn new() -> Self {
        Self { items: Vec::new(), scroll: 0, auto_stick: true, focused: None }
    }

    pub fn push(&mut self, item: ConversationItem) {
        self.items.push(item);
        // Newly-pushed becomes focused only if nothing is.
        if self.focused.is_none() {
            self.focused = Some(self.items.len() - 1);
        }
    }

    pub fn items_mut(&mut self) -> &mut Vec<ConversationItem> { &mut self.items }
    pub fn items(&self) -> &[ConversationItem] { &self.items }

    pub fn focus_next(&mut self) {
        if self.items.is_empty() { return; }
        self.focused = Some(match self.focused {
            None => 0,
            Some(i) => (i + 1) % self.items.len(),
        });
    }
    pub fn focus_prev(&mut self) {
        if self.items.is_empty() { return; }
        self.focused = Some(match self.focused {
            None => self.items.len() - 1,
            Some(0) => self.items.len() - 1,
            Some(i) => i - 1,
        });
    }

    /// Toggle the focused collapsible (no-op for plain messages).
    pub fn toggle_focused(&mut self) {
        if let Some(i) = self.focused {
            if let Some(ConversationItem::Block(b)) = self.items.get_mut(i) {
                b.toggle();
            }
        }
    }

    pub fn scroll_up(&mut self, n: u16) {
        self.scroll = self.scroll.saturating_sub(n);
        self.auto_stick = false;
    }
    pub fn scroll_down(&mut self, n: u16, content_h: u16, view_h: u16) {
        let max = content_h.saturating_sub(view_h);
        self.scroll = (self.scroll + n).min(max);
        if self.scroll >= max { self.auto_stick = true; }
    }
    pub fn stick_to_bottom(&mut self) {
        self.auto_stick = true;
    }

    /// Total content height in rows given a render width.
    fn content_height(&self, width: u16) -> u16 {
        let mut h = 0u16;
        for item in &self.items {
            h = h.saturating_add(item_height(item, width));
            h = h.saturating_add(1); // 1-row spacer between items
        }
        h
    }

    pub fn render(&mut self, frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(theme.border)
            .title(" Conversation ");
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let total = self.content_height(inner.width);

        // Auto-stick: when content grows beyond the viewport, snap to bottom.
        if self.auto_stick {
            self.scroll = total.saturating_sub(inner.height);
        }

        // Render items top-down. Skip rows above scroll, stop at viewport bottom.
        let mut y = inner.y;
        let mut skip = self.scroll;
        let view_bottom = inner.y.saturating_add(inner.height);

        for (i, item) in self.items.iter().enumerate() {
            let h = item_height(item, inner.width);
            if skip >= h + 1 {
                skip -= h + 1;
                continue;
            }
            // Partially visible from top: clip via skip.
            let visible_h = (h.saturating_sub(skip)).min(view_bottom.saturating_sub(y));
            if visible_h == 0 { break; }
            let rect = Rect { x: inner.x, y, width: inner.width, height: visible_h };
            render_item(item, frame, rect, theme, self.focused == Some(i));
            y = y.saturating_add(visible_h).saturating_add(1);
            skip = 0;
            if y >= view_bottom { break; }
        }

        // Scrollbar on the right edge of the conversation area.
        if total > inner.height {
            let mut sb_state = ScrollbarState::new(total as usize)
                .position(self.scroll as usize)
                .viewport_content_length(inner.height as usize);
            let sb = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(theme.text_dim);
            frame.render_stateful_widget(sb, area, &mut sb_state);
        }
    }

    pub fn handle_event(&mut self, event: Event, view_h: u16) -> bool {
        let Event::Key(KeyEvent { code, .. }) = event else { return false; };
        match code {
            KeyCode::Up    => { self.focus_prev(); true }
            KeyCode::Down  => { self.focus_next(); true }
            KeyCode::Enter => { self.toggle_focused(); true }
            KeyCode::PageUp => { self.scroll_up(view_h); true }
            KeyCode::PageDown => {
                let total = self.content_height(80);
                self.scroll_down(view_h, total, view_h);
                true
            }
            KeyCode::Home => { self.scroll = 0; self.auto_stick = false; true }
            KeyCode::End  => { self.stick_to_bottom(); true }
            _ => false,
        }
    }
}

fn item_height(item: &ConversationItem, width: u16) -> u16 {
    match item {
        ConversationItem::Block(b) => b.height(width.saturating_sub(2)),
        ConversationItem::Message(m) => {
            // 1 header line + wrapped body lines. Cheap approx: text length / width.
            let body_w = width.saturating_sub(4).max(1);
            let lines = m.text.lines().fold(0u16, |acc, line| {
                let w = line.chars().count() as u16;
                acc.saturating_add(if w == 0 { 1 } else { w.div_ceil(body_w) })
            });
            1u16.saturating_add(lines)
        }
    }
}

fn render_item(
    item: &ConversationItem,
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    focused: bool,
) {
    match item {
        ConversationItem::Block(b) => {
            // We don't differentiate "focused collapsible" visually beyond the
            // border highlight — keep it cheap; could add a left-edge marker later.
            let _ = focused;
            b.render(frame, area, theme);
        }
        ConversationItem::Message(m) => {
            let (label, style) = match m.role {
                BubbleRole::User      => ("you",    theme.user_text),
                BubbleRole::Assistant => ("alva",   theme.assistant_text),
                BubbleRole::System    => ("system", theme.system_text),
                BubbleRole::Error     => ("error",  theme.error_text),
            };
            let mut header_style = style;
            if focused { header_style = header_style.add_modifier(Modifier::REVERSED); }

            let header = Line::from(Span::styled(format!(" {} ", label), header_style));
            let body = Text::raw(m.text.clone());

            let header_area = Rect { height: 1, ..area };
            frame.render_widget(Paragraph::new(header), header_area);

            let body_area = Rect {
                x: area.x + 2,
                y: area.y + 1,
                width: area.width.saturating_sub(2),
                height: area.height.saturating_sub(1),
            };
            let para = Paragraph::new(body).wrap(Wrap { trim: false });
            frame.render_widget(para, body_area);
        }
    }
}
