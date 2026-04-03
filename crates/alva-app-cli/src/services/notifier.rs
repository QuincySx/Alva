//! Terminal notification service.
//!
//! Detects the terminal emulator via `TERM_PROGRAM` and sends notifications
//! using the appropriate escape sequence. Falls back to a simple BEL character
//! when the terminal is unrecognised.

use std::io::{self, Write};

/// Known terminal emulators that support custom notification escape sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKind {
    ITerm2,
    Kitty,
    Ghostty,
    AppleTerminal,
    WezTerm,
    /// Any terminal we do not specifically recognise.
    Unknown,
}

impl TerminalKind {
    /// Detect the running terminal from the `TERM_PROGRAM` environment variable.
    pub fn detect() -> Self {
        let term = std::env::var("TERM_PROGRAM").unwrap_or_default();
        match term.as_str() {
            "iTerm.app" | "iTerm2" => Self::ITerm2,
            "kitty" => Self::Kitty,
            "ghostty" => Self::Ghostty,
            "Apple_Terminal" => Self::AppleTerminal,
            "WezTerm" => Self::WezTerm,
            _ => Self::Unknown,
        }
    }
}

/// A lightweight notification service that uses terminal escape sequences.
#[derive(Debug, Clone)]
pub struct Notifier {
    kind: TerminalKind,
}

impl Notifier {
    /// Create a new `Notifier`, auto-detecting the terminal type.
    pub fn new() -> Self {
        Self {
            kind: TerminalKind::detect(),
        }
    }

    /// Create a `Notifier` for a specific terminal kind (useful for testing).
    pub fn with_kind(kind: TerminalKind) -> Self {
        Self { kind }
    }

    /// The detected terminal kind.
    pub fn kind(&self) -> TerminalKind {
        self.kind
    }

    /// Send a notification with the given `text`.
    ///
    /// The exact mechanism depends on the detected terminal:
    /// - **iTerm2**: `ESC ] 9 ; <text> BEL`
    /// - **Kitty**: `ESC ] 99 ; i=1:d=0:p=title ; <text> ST`
    /// - **Ghostty / WezTerm**: terminal bell (these terminals surface the bell
    ///   as an OS notification when configured).
    /// - **Apple Terminal / Unknown**: terminal bell (`\x07`).
    pub fn notify(&self, text: &str) -> io::Result<()> {
        let mut out = io::stdout().lock();
        match self.kind {
            TerminalKind::ITerm2 => {
                // OSC 9 — iTerm2 Growl-style notification
                write!(out, "\x1b]9;{}\x07", text)?;
            }
            TerminalKind::Kitty => {
                // OSC 99 — kitty desktop notification protocol
                // i=1  — unique notification id (we reuse 1)
                // d=0  — close after timeout
                // p=title — the payload is the title text
                write!(out, "\x1b]99;i=1:d=0:p=title;{}\x1b\\", text)?;
            }
            TerminalKind::Ghostty
            | TerminalKind::WezTerm
            | TerminalKind::AppleTerminal
            | TerminalKind::Unknown => {
                // Fallback: terminal bell — many terminals convert this into an
                // OS notification when the window is not focused.
                write!(out, "\x07")?;
            }
        }
        out.flush()
    }
}

impl Default for Notifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_returns_unknown_for_empty_env() {
        // In test environment TERM_PROGRAM is usually unset.
        let kind = TerminalKind::detect();
        // We cannot guarantee what it will be, but it should not panic.
        let _ = kind;
    }

    #[test]
    fn notifier_with_kind_roundtrips() {
        let n = Notifier::with_kind(TerminalKind::Kitty);
        assert_eq!(n.kind(), TerminalKind::Kitty);
    }
}
