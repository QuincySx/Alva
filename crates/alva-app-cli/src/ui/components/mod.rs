// INPUT:  crossterm::Event, ratatui (Frame, Rect, widgets), super::theme
// OUTPUT: Component trait, ComponentAction, ModalFrame, Picker, TextField,
//         Toast, Tabs, layout helpers
// POS:    Reusable TUI widget library on top of ratatui. Anything in the
//         alva TUI that pops up, asks for input, or filters a list lives
//         here so future menus / settings / dialogs don't reimplement
//         layout + key routing each time.

//! # Component framework
//!
//! Three concepts:
//!
//! - **`Component`** — render + handle_event. The trait every UI piece implements.
//! - **`ComponentAction`** — what a component returns up to its parent
//!   (Dismiss, Submit, Changed, Bubble, None).
//! - **Composition** — a parent stores child Components as fields, calls
//!   their `render` from its own `render`, and routes events to them in
//!   `handle_event`. There's no global focus router.
//!
//! ## When to use which built-in
//!
//! | Need                              | Use                         |
//! |-----------------------------------|-----------------------------|
//! | Pop-up dialog with a border       | `ModalFrame`                |
//! | List the user picks from          | `Picker<T>`                 |
//! | Single-line text edit             | `TextField`                 |
//! | Toggle / yes-no                   | `Toggle`                    |
//! | Tab strip + body swap             | `Tabs`                      |
//! | Transient status message          | `Toast`                     |
//! | Center / anchor a popup rect      | `layout::*`                 |
//!
//! Compose them: `Form` is just a `Vec<TextField | Picker | Toggle>`,
//! `SettingsScreen` is `Tabs` over several `Form`s, etc.

use crossterm::event::Event;
use ratatui::layout::Rect;
use ratatui::Frame;

use super::theme::Theme;

pub mod modal;
pub mod picker;
pub mod text_field;
pub mod toast;
pub mod tabs;
pub mod toggle;
pub mod layout;
pub mod tree;
pub mod throbber;
pub mod popup;
pub mod image;
// Chat-screen building blocks (top-to-bottom): pinned header, conversation
// view (with collapsibles), attachment strip, multi-line chat input.
pub mod pinned_header;
pub mod conversation;
pub mod collapsible;
pub mod attachment_strip;
pub mod chat_input;

pub use modal::ModalFrame;
pub use picker::Picker;
pub use text_field::TextField;
pub use toast::{Toast, ToastKind, ToastStack};
pub use tabs::Tabs;
pub use toggle::Toggle;
pub use tree::Tree;
pub use throbber::ProgressThrobber;
pub use popup::ScrollablePopup;
pub use image::ImageView;
pub use pinned_header::PinnedHeader;
pub use conversation::{ConversationView, ConversationItem, MessageBubble, BubbleRole};
pub use collapsible::{CollapsibleBlock, CollapsibleKind};
pub use attachment_strip::{AttachmentStrip, Attachment, AttachmentKind};
pub use chat_input::{ChatInput, ChatInputAction};

/// What a component returns to its parent after handling one event.
/// `None` is "consumed, do nothing"; `Bubble(ev)` is "I didn't want this,
/// caller please handle it"; `Dismiss` / `Submit` are common terminal
/// states for modal-style components.
#[derive(Debug, Clone)]
pub enum ComponentAction {
    /// Event was handled (or ignored deliberately) — do nothing.
    None,
    /// Component asks to be closed without committing.
    Dismiss,
    /// Component is committing a value. The parent decides what to do.
    Submit(String),
    /// Selection / value changed but not yet committed (live preview).
    Changed,
    /// Component didn't claim the event — parent should keep routing.
    Bubble(Event),
}

/// Generic focusable UI piece. Implementors define how to draw themselves
/// into a `Rect` and how to react to one event. They do NOT own focus
/// state — the parent decides which component is "active" by routing
/// events to that one.
pub trait Component {
    fn render(&self, frame: &mut Frame<'_>, area: Rect, theme: &Theme);
    fn handle_event(&mut self, event: Event) -> ComponentAction;
}
