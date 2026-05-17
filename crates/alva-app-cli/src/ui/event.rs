//! Terminal event handler for the TUI application.
//!
//! Provides a thin abstraction over crossterm events, adding a periodic
//! [`TerminalEvent::Tick`] variant that drives animations (spinner, etc.).

use crossterm::event::{self, Event, KeyEvent, MouseEvent};
use std::time::Duration;

/// Terminal events consumed by the TUI application loop.
#[derive(Debug)]
pub enum TerminalEvent {
    /// A key was pressed.
    Key(KeyEvent),
    /// A mouse event occurred (click, scroll, drag, move).
    Mouse(MouseEvent),
    /// The terminal was resized.
    Resize(u16, u16),
    /// A bracketed-paste arrived as a single chunk (Cmd/Ctrl+V).
    Paste(String),
    /// A tick fired — used for spinner animation and periodic redraws.
    Tick,
}

/// Poll for the next terminal event, returning [`TerminalEvent::Tick`] if no
/// real event arrives within `tick_rate`.
///
/// Returns `None` only on unrecoverable poll/read errors (callers may treat
/// this as a signal to exit).
pub fn poll_event(tick_rate: Duration) -> Option<TerminalEvent> {
    if event::poll(tick_rate).ok()? {
        match event::read().ok()? {
            Event::Key(key) => Some(TerminalEvent::Key(key)),
            Event::Mouse(mouse) => Some(TerminalEvent::Mouse(mouse)),
            Event::Resize(w, h) => Some(TerminalEvent::Resize(w, h)),
            Event::Paste(s) => Some(TerminalEvent::Paste(s)),
            _ => None,
        }
    } else {
        Some(TerminalEvent::Tick)
    }
}
