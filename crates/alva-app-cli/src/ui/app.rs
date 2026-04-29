//! Full-screen TUI application built on ratatui.
//!
//! [`TuiApp`] manages terminal lifecycle, UI state, rendering, and keyboard
//! input. It is designed as an **alternative** to the basic REPL — the caller
//! chooses which mode to run.
//!
//! Layout:
//! ```text
//! +----------------------------------------------+
//! | Status Bar: Model | Session | Tokens         |
//! +----------------------------------------------+
//! |                                              |
//! |  Message List (scrollable)                   |
//! |  - User messages (cyan)                      |
//! |  - Assistant messages (white + markdown)     |
//! |  - Tool uses (bullet running, check done)    |
//! |  - System messages (yellow)                  |
//! |                                              |
//! +----------------------------------------------+
//! | > Input area                                 |
//! |   model-name | 1234t                         |
//! +----------------------------------------------+
//! ```

use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;
use ratatui::Terminal;

use alva_app_core::{AgentEvent, AgentMessage, AlvaPaths, BaseAgent, PermissionDecision};
use alva_host_native::middleware::ApprovalRequest;
use alva_kernel_abi::agent_session::EventQuery;
use alva_kernel_abi::AgentSession;
use alva_llm_provider::{OpenAIChatProvider, ProviderConfig};
use tokio::sync::mpsc;

use crate::checkpoint;
use crate::session::{JsonFileAgentSession, JsonFileSessionManager};

use super::components::{
    Attachment, AttachmentKind, AttachmentStrip, ChatInput, ChatInputAction, CollapsibleBlock,
    Component, ConversationItem, ConversationView, MessageBubble, Picker, PinnedHeader,
};
use super::event::{poll_event, TerminalEvent};
use super::permission_dialog::{PermissionDialogWidget, PermissionType};
use super::typeahead::Typeahead;
use super::spinner::{SpinnerWidget, SPINNER_FRAMES};
use super::theme::{Theme, ThemeMode};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Target tick rate (~30 fps).
const TICK_RATE: Duration = Duration::from_millis(33);

/// Tips shown while the spinner is active.
const SPINNER_TIPS: &[&str] = &[
    "Ctrl+C to interrupt",
    "Use /help for commands",
    "/plan toggles read-only mode",
    "!cmd runs a shell command",
    "/model <id> to switch models",
];

// ---------------------------------------------------------------------------
// Pending approval state
// ---------------------------------------------------------------------------

/// Tracks a pending permission dialog overlay.
struct PendingApproval {
    request: ApprovalRequest,
}

// ---------------------------------------------------------------------------
// Slash command catalog
// ---------------------------------------------------------------------------

/// Hardcoded list of slash commands the typeahead offers. Mirrors what the
/// reedline path advertises (CommandRegistry builtins + REPL inline-handled
/// extras). Out-of-band: if you wire CommandRegistry in here later, replace
/// this with `registry.list().into_iter().map(|(n,_)| n.to_string()).collect()`.
fn default_slash_commands() -> Vec<String> {
    [
        // Registry builtins (commands/registry.rs)
        "clear", "compact", "new", "help", "exit", "cost", "status", "doctor",
        "config", "model", "theme", "permissions", "plan", "fast", "vim",
        "commit", "review", "export",
        // Inline-handled in repl.rs (and equivalents in TUI)
        "quit", "resume", "fork", "rewind", "sessions", "setup", "auto", "locks",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

// ---------------------------------------------------------------------------
// TuiApp
// ---------------------------------------------------------------------------

/// Full-screen terminal UI application state.
///
/// All conversation rendering goes through the reusable component framework
/// in [`super::components`]:
/// - `conversation` is a [`ConversationView`] of [`MessageBubble`] +
///   [`CollapsibleBlock`] items (tool calls / thinking blocks fold up).
/// - `chat_input` is a multi-line [`ChatInput`] that emits SlashTrigger /
///   AtTrigger so the parent can pop a palette while the user types.
/// - `pinned_question` keeps the user's most recent prompt visible at the
///   top so streaming output can't push it off-screen.
/// - `attachments` holds pending file/image chips above the input.
pub struct TuiApp {
    // -- conversation --
    conversation: ConversationView,
    /// User's most recent prompt — rendered in the PinnedHeader stripe.
    pinned_question: Option<String>,
    attachments: AttachmentStrip,

    // -- input --
    chat_input: ChatInput,
    /// Snapshot of the chat-input value used by typeahead filtering. We
    /// can't borrow `chat_input` while `typeahead` mutates so we mirror it.
    last_input_snapshot: String,

    // -- spinner --
    spinner_active: bool,
    spinner_frame: usize,
    spinner_tip_index: usize,
    tick_count: u64,

    // -- session info --
    pub(crate) model_name: String,
    pub(crate) session_id: String,
    total_input_tokens: u32,
    total_output_tokens: u32,

    // -- permission dialog --
    pending_approval: Option<PendingApproval>,

    // -- input history --
    input_history: Vec<String>,
    history_index: Option<usize>,
    /// Buffer saved when the user starts navigating history.
    saved_input: String,

    // -- streaming --
    /// Index in `conversation` of the assistant bubble that is currently
    /// receiving streaming text. None when no stream is in flight.
    streaming_idx: Option<usize>,
    /// Index in `conversation` of the open thinking block — collected
    /// across `ReasoningDelta`s and sealed on `ReasoningBlock`. The block
    /// stays in the conversation (folded by default) once sealed.
    thinking_idx: Option<usize>,
    /// Total chars accumulated in the current thinking block — used for
    /// the "1.2K chars" badge after seal.
    thinking_chars: usize,
    /// Pending tool blocks awaiting completion: (tool_name, conv_idx).
    /// We drain in reverse on ToolExecutionEnd to match the most recent
    /// "running" entry for that tool.
    pending_tools: Vec<(String, usize)>,

    // -- typeahead / slash-command autocomplete --
    typeahead: Typeahead,

    // -- @-file picker (overlay) --
    /// Open file-picker when the cursor is in an `@…` token. Built lazily
    /// from a `walkdir`-style scan of `workspace_root`, capped at 500 files
    /// so very large repos stay responsive.
    file_picker: Option<Picker<PathBuf>>,
    /// Workspace root used by the file picker — stored here so on_key can
    /// rebuild the picker without threading workspace through every call.
    workspace_root: Option<PathBuf>,

    // -- theme --
    theme: Theme,

    // -- exit flag --
    pub(crate) should_quit: bool,
}

impl TuiApp {
    /// Create a new TUI app with the given model name and session ID.
    pub fn new(model_name: &str, session_id: &str) -> Self {
        Self {
            conversation: ConversationView::new(),
            pinned_question: None,
            attachments: AttachmentStrip::new(),

            chat_input: ChatInput::new("Send a message — / for commands, @ for files, Shift+Enter for newline"),
            last_input_snapshot: String::new(),

            spinner_active: false,
            spinner_frame: 0,
            spinner_tip_index: 0,
            tick_count: 0,

            model_name: model_name.to_string(),
            session_id: session_id.to_string(),
            total_input_tokens: 0,
            total_output_tokens: 0,

            pending_approval: None,

            input_history: Vec::new(),
            history_index: None,
            saved_input: String::new(),

            streaming_idx: None,
            thinking_idx: None,
            thinking_chars: 0,
            pending_tools: Vec::new(),

            typeahead: Typeahead::new(default_slash_commands()),

            file_picker: None,
            workspace_root: None,

            theme: Theme::new(ThemeMode::Dark),

            should_quit: false,
        }
    }

    /// Tell the app where the workspace root is so the @-file picker can
    /// scan it. `run_tui` calls this once at startup.
    pub(crate) fn set_workspace(&mut self, root: PathBuf) {
        self.workspace_root = Some(root);
    }

    // -- state mutation helpers ------------------------------------------------

    pub(crate) fn push_user_message(&mut self, text: &str) {
        self.pinned_question = Some(text.to_string());
        self.conversation.push(ConversationItem::Message(MessageBubble::user(text)));
        self.conversation.stick_to_bottom();
    }

    pub(crate) fn push_system_message(&mut self, text: &str) {
        self.conversation.push(ConversationItem::Message(MessageBubble::system(text)));
        self.conversation.stick_to_bottom();
    }

    pub(crate) fn push_error_message(&mut self, text: &str) {
        self.conversation.push(ConversationItem::Message(MessageBubble::error(text)));
        self.conversation.stick_to_bottom();
    }

    /// Reset conversation (used by /clear and /new). Keeps history & input.
    pub(crate) fn reset_conversation(&mut self) {
        self.conversation = ConversationView::new();
        self.pinned_question = None;
        self.streaming_idx = None;
        self.thinking_idx = None;
        self.thinking_chars = 0;
        self.pending_tools.clear();
    }

    fn begin_streaming(&mut self) {
        // Push an empty assistant bubble — subsequent deltas append into it.
        self.conversation
            .push(ConversationItem::Message(MessageBubble::assistant(String::new())));
        self.streaming_idx = Some(self.conversation.items().len() - 1);
        self.conversation.stick_to_bottom();
    }

    fn append_streaming_text(&mut self, text: &str) {
        let Some(idx) = self.streaming_idx else { return; };
        if let Some(ConversationItem::Message(b)) = self.conversation.items_mut().get_mut(idx) {
            b.text.push_str(text);
        }
        self.conversation.stick_to_bottom();
    }

    fn end_streaming(&mut self) {
        self.streaming_idx = None;
    }

    fn add_tool_start(&mut self, name: &str, input_summary: &str) {
        let header = if input_summary.is_empty() {
            name.to_string()
        } else {
            format!("{} · {}", name, truncate(input_summary, 60))
        };
        let body = Text::from("(running…)");
        let block = CollapsibleBlock::tool_call(header, body).with_badge("running");
        self.conversation.push(ConversationItem::Block(block));
        let idx = self.conversation.items().len() - 1;
        self.pending_tools.push((name.to_string(), idx));
        self.conversation.stick_to_bottom();
    }

    fn complete_tool(&mut self, name: &str, is_error: bool, preview: &str) {
        // Match the most recent pending tool with this name.
        let Some(pos) = self
            .pending_tools
            .iter()
            .rposition(|(n, _)| n == name)
        else { return; };
        let (_, idx) = self.pending_tools.remove(pos);
        let Some(ConversationItem::Block(block)) =
            self.conversation.items_mut().get_mut(idx) else { return; };
        block.badge = Some(if is_error { "✗ error".into() } else { "✓ done".into() });
        let preview_owned = preview.to_string();
        block.body = Text::from(preview_owned);
    }

    /// Append a `ReasoningDelta` text fragment to the open thinking block,
    /// creating one (collapsed) if there isn't one in flight. The block
    /// stays in place once sealed so the user can expand it for review.
    fn append_thinking(&mut self, text: &str) {
        let idx = match self.thinking_idx {
            Some(i) => i,
            None => {
                let block = CollapsibleBlock::thinking("Thinking…", Text::from(String::new()))
                    .with_badge("…");
                self.conversation.push(ConversationItem::Block(block));
                let i = self.conversation.items().len() - 1;
                self.thinking_idx = Some(i);
                self.thinking_chars = 0;
                i
            }
        };
        if let Some(ConversationItem::Block(b)) = self.conversation.items_mut().get_mut(idx) {
            // Append the delta to the body's last line (or push a new line).
            let new_text = format!("{}{}", body_text(b), text);
            b.body = Text::from(new_text);
            self.thinking_chars += text.chars().count();
            b.badge = Some(format_chars(self.thinking_chars));
        }
        self.conversation.stick_to_bottom();
    }

    /// Seal the current thinking block — called on `ReasoningBlock` so the
    /// authoritative final text replaces the streamed accumulation.
    fn seal_thinking(&mut self, final_text: Option<&str>) {
        let Some(idx) = self.thinking_idx.take() else { return; };
        if let Some(ConversationItem::Block(b)) = self.conversation.items_mut().get_mut(idx) {
            if let Some(t) = final_text {
                b.body = Text::from(t.to_string());
                self.thinking_chars = t.chars().count();
            }
            b.header = "Thought".to_string();
            b.badge = Some(format_chars(self.thinking_chars));
        }
        self.thinking_chars = 0;
    }

    // -- input helpers --------------------------------------------------------

    /// Push the just-submitted text to history and clear the input editor.
    fn record_submission(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() { return; }
        self.input_history.push(text.to_string());
        self.history_index = None;
        self.saved_input.clear();
        self.chat_input.clear();
        self.last_input_snapshot.clear();
    }

    fn history_prev(&mut self) {
        if self.input_history.is_empty() { return; }
        match self.history_index {
            None => {
                self.saved_input = self.chat_input.value();
                let idx = self.input_history.len() - 1;
                self.history_index = Some(idx);
                self.chat_input.set_value(self.input_history[idx].clone());
            }
            Some(idx) if idx > 0 => {
                let new_idx = idx - 1;
                self.history_index = Some(new_idx);
                self.chat_input.set_value(self.input_history[new_idx].clone());
            }
            _ => {}
        }
        self.last_input_snapshot = self.chat_input.value();
    }

    fn history_next(&mut self) {
        if let Some(idx) = self.history_index {
            if idx + 1 < self.input_history.len() {
                let new_idx = idx + 1;
                self.history_index = Some(new_idx);
                self.chat_input.set_value(self.input_history[new_idx].clone());
            } else {
                self.history_index = None;
                let saved = std::mem::take(&mut self.saved_input);
                self.chat_input.set_value(saved);
            }
            self.last_input_snapshot = self.chat_input.value();
        }
    }

    // -- keyboard handling ----------------------------------------------------

    /// Process a key event. Returns `true` if the event was consumed.
    fn on_key(&mut self, key: KeyEvent) -> KeyAction {
        // If a permission dialog is active, handle it first.
        if self.pending_approval.is_some() {
            return self.on_key_approval(key);
        }

        // File picker (opened by `@`) takes priority over both typeahead
        // and ChatInput edits for navigation keys. Typing characters falls
        // through so the user can keep filtering by extending the @-token.
        if self.file_picker.is_some() {
            match key.code {
                KeyCode::Up | KeyCode::Down | KeyCode::PageUp | KeyCode::PageDown => {
                    if let Some(p) = self.file_picker.as_mut() {
                        let action = p.handle_event(Event::Key(key));
                        let _ = action;
                    }
                    return KeyAction::None;
                }
                KeyCode::Enter | KeyCode::Tab => {
                    let pick = self.file_picker.as_ref().and_then(|p| {
                        let label = p.selected_label()?.to_string();
                        let path = p.selected_value()?.clone();
                        Some((label, path))
                    });
                    if let Some((label, path)) = pick {
                        self.accept_file_pick(&label, &path);
                    }
                    return KeyAction::None;
                }
                KeyCode::Esc => { self.close_file_picker(); return KeyAction::None; }
                _ => {} // fall through to ChatInput
            }
        }

        // Typeahead has priority while open: ↑/↓ navigate, Enter/Tab accept,
        // Esc dismiss. Anything else falls through to ChatInput so the user
        // can keep editing while the menu re-filters live.
        if self.typeahead.is_active() {
            match key.code {
                KeyCode::Up => { self.typeahead.prev(); return KeyAction::None; }
                KeyCode::Down => { self.typeahead.next(); return KeyAction::None; }
                KeyCode::Enter | KeyCode::Tab => {
                    if let Some(cmd) = self.typeahead.accept() {
                        // Replace the buffer with `/<cmd>` (Typeahead returns
                        // the bare command name).
                        let value = if cmd.starts_with('/') { cmd } else { format!("/{}", cmd) };
                        self.chat_input.set_value(value);
                        self.last_input_snapshot = self.chat_input.value();
                    }
                    return KeyAction::None;
                }
                KeyCode::Esc => { self.typeahead.dismiss(); return KeyAction::None; }
                _ => {} // fall through to normal edit handling
            }
        }

        // Top-level shortcuts that should not be claimed by the editor.
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                self.should_quit = true;
                return KeyAction::None;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => return KeyAction::Interrupt,
            // Conversation scroll keys (only when not paging history).
            (_, KeyCode::PageUp) => { self.conversation.scroll_up(10); return KeyAction::None; }
            (_, KeyCode::PageDown) => {
                // ConversationView clamps via render; pass a reasonable view height.
                self.conversation.scroll_down(10, 80, 20);
                return KeyAction::None;
            }
            // History navigation: Alt+Up/Alt+Down (Up/Down are reserved for
            // multi-line cursor movement inside the editor).
            (KeyModifiers::ALT, KeyCode::Up)   => { self.history_prev(); self.refresh_typeahead(); return KeyAction::None; }
            (KeyModifiers::ALT, KeyCode::Down) => { self.history_next(); self.refresh_typeahead(); return KeyAction::None; }
            (_, KeyCode::Esc) => {
                // Clear the buffer and dismiss any open menu.
                self.chat_input.clear();
                self.last_input_snapshot.clear();
                self.typeahead.dismiss();
                return KeyAction::None;
            }
            _ => {}
        }

        // Forward to ChatInput. It owns Enter (submit), Shift+Enter (newline),
        // and all the editing keys.
        let action = self.chat_input.handle_event(Event::Key(key));
        match action {
            ChatInputAction::Submit(text) => {
                self.record_submission(&text);
                self.typeahead.dismiss();
                KeyAction::Submit(text)
            }
            ChatInputAction::Cancel => KeyAction::Interrupt,
            ChatInputAction::SlashTrigger(token) => {
                // Reedline-style: open and prefix-filter on the token.
                self.typeahead.update(&format!("/{}", token));
                self.last_input_snapshot = self.chat_input.value();
                KeyAction::None
            }
            ChatInputAction::AtTrigger(token) => {
                self.typeahead.dismiss();
                self.open_or_filter_file_picker(&token);
                self.last_input_snapshot = self.chat_input.value();
                KeyAction::None
            }
            ChatInputAction::Changed => {
                self.refresh_typeahead();
                KeyAction::None
            }
            ChatInputAction::None => KeyAction::None,
        }
    }

    /// Re-run typeahead filtering against the current ChatInput value.
    /// Called after history nav or non-trigger edits.
    fn refresh_typeahead(&mut self) {
        let v = self.chat_input.value();
        self.last_input_snapshot = v.clone();
        if v.starts_with('/') {
            self.typeahead.update(&v);
        } else if self.typeahead.is_active() {
            self.typeahead.dismiss();
        }
        // The @-picker stays open as long as the cursor is inside an @-word.
        // ChatInput emits AtTrigger every keystroke while inside one; if no
        // trigger fires, close the picker.
        if self.file_picker.is_some() {
            self.close_file_picker();
        }
    }

    /// Open or refresh the file picker filtered by `token` (the chars
    /// after `@`). First-open scans the workspace once; subsequent calls
    /// just update the query.
    fn open_or_filter_file_picker(&mut self, token: &str) {
        if self.file_picker.is_none() {
            let root = match &self.workspace_root {
                Some(r) => r.clone(),
                None => return, // can't scan without a root
            };
            let entries = scan_workspace_files(&root, 500);
            if entries.is_empty() { return; }
            let items: Vec<(PathBuf, String)> = entries
                .iter()
                .map(|p| {
                    let label = p.strip_prefix(&root).unwrap_or(p).display().to_string();
                    (p.clone(), label)
                })
                .collect();
            let picker = Picker::new(items, "Files").show_query(true);
            self.file_picker = Some(picker);
        }
        if let Some(p) = self.file_picker.as_mut() {
            p.set_query(token);
        }
    }

    fn close_file_picker(&mut self) {
        self.file_picker = None;
    }

    /// Accept a file-picker selection. Two paths depending on file type:
    /// - **Image** (png/jpg/...) → push onto AttachmentStrip and just strip
    ///   the `@token` from the input. The attachment chip shows the file.
    /// - **Other** → replace `@token` with `@<rel_path>` text (plus a
    ///   trailing space) so the agent can read the path from the prompt.
    fn accept_file_pick(&mut self, label: &str, path: &Path) {
        let attachment = Attachment::auto(path.to_path_buf());
        let is_image = matches!(attachment.kind, AttachmentKind::Image);

        let buf = self.chat_input.value();
        let at_pos = find_at_token_start(&buf);

        let new_value = if is_image {
            self.attachments.push(attachment);
            // Strip the @-token entirely; the chip represents the image.
            match at_pos {
                Some(at) => buf[..at].trim_end().to_string(),
                None => buf,
            }
        } else {
            match at_pos {
                Some(at) => format!("{}@{} ", &buf[..at], label),
                None => format!("{}@{} ", buf, label),
            }
        };
        self.chat_input.set_value(new_value);
        self.last_input_snapshot = self.chat_input.value();
        self.close_file_picker();
    }

    fn on_key_approval(&mut self, key: KeyEvent) -> KeyAction {
        let decision = match key.code {
            KeyCode::Char('y') | KeyCode::Enter => Some(PermissionDecision::AllowOnce),
            KeyCode::Char('a') => Some(PermissionDecision::AllowAlways),
            KeyCode::Char('n') | KeyCode::Esc => Some(PermissionDecision::RejectOnce),
            KeyCode::Char('d') => Some(PermissionDecision::RejectAlways),
            _ => None,
        };

        if let Some(decision) = decision {
            if let Some(approval) = self.pending_approval.take() {
                return KeyAction::ApprovalDecision {
                    request: approval.request,
                    decision,
                };
            }
        }

        KeyAction::None
    }

    // -- mouse handling -------------------------------------------------------

    /// Process a mouse event. Wheel scrolls the conversation; right-click
    /// snaps to bottom (re-engages auto-stick).
    fn on_mouse(&mut self, mouse: MouseEvent, _area: Rect) {
        match mouse.kind {
            MouseEventKind::ScrollUp => self.conversation.scroll_up(3),
            MouseEventKind::ScrollDown => self.conversation.scroll_down(3, 200, 20),
            MouseEventKind::Down(MouseButton::Right) => self.conversation.stick_to_bottom(),
            _ => {}
        }
    }

    // -- rendering ------------------------------------------------------------

    fn draw(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
        // Snapshot dynamic-section heights so the layout stays stable while
        // rendering (the components borrow &self).
        let pinned_h = if self.pinned_question.is_some() { 4u16 } else { 0u16 };
        let attach_h = if self.attachments.is_empty() { 0u16 } else { 2u16 };

        terminal.draw(|frame| {
            let area = frame.area();

            // Layout: status (1) | pinned (0 or 4) | conversation (min)
            //       | attachments (0 or 2) | chat input (5)
            let chunks = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(pinned_h),
                Constraint::Min(3),
                Constraint::Length(attach_h),
                Constraint::Length(5),
            ])
            .split(area);

            let status_area = chunks[0];
            let pinned_area = chunks[1];
            let conv_area   = chunks[2];
            let attach_area = chunks[3];
            let input_area  = chunks[4];

            self.render_status_bar(frame, status_area);

            if let Some(q) = self.pinned_question.as_deref() {
                PinnedHeader::new(q).render(frame, pinned_area, &self.theme);
            }

            self.conversation.render(frame, conv_area, &self.theme);

            self.attachments.render(frame, attach_area, &self.theme);

            self.chat_input.render(frame, input_area, &self.theme);

            // -- Typeahead popup (anchored above the chat input) --
            if self.typeahead.is_active() {
                self.render_typeahead(frame, conv_area);
            }

            // -- File picker overlay (anchored above the input, larger) --
            if let Some(picker) = self.file_picker.as_ref() {
                self.render_file_picker(frame, conv_area, picker);
            }

            // -- Spinner overlay (bottom edge of conversation) --
            if self.spinner_active {
                self.render_spinner(frame, conv_area);
            }

            // -- Permission dialog overlay --
            if let Some(ref approval) = self.pending_approval {
                self.render_permission_dialog(frame, area, approval);
            }
        })?;

        Ok(())
    }

    fn render_status_bar(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let total_tokens = self.total_input_tokens + self.total_output_tokens;
        let status = Line::from(vec![
            Span::styled(
                format!(" {} ", self.model_name),
                self.theme.status_bar.add_modifier(Modifier::BOLD),
            ),
            Span::styled(" | ", self.theme.status_bar),
            Span::styled(
                format!(
                    "Session: {} ",
                    if self.session_id.len() > 8 {
                        &self.session_id[..8]
                    } else {
                        &self.session_id
                    }
                ),
                self.theme.status_bar,
            ),
            Span::styled(" | ", self.theme.status_bar),
            Span::styled(
                format!("{}T ", total_tokens),
                self.theme.status_bar,
            ),
            // Fill the rest of the status bar
            Span::styled(
                " ".repeat(area.width.saturating_sub(40) as usize),
                self.theme.status_bar,
            ),
        ]);

        let paragraph = Paragraph::new(status).style(self.theme.status_bar);
        frame.render_widget(paragraph, area);
    }

    /// Render the slash-command typeahead as a small bordered popup anchored
    /// to the bottom-left of `message_area`, just above the input prompt.
    /// Shows up to 6 candidates; the selected row is highlighted via theme
    /// `selection`. Indicator on the left edge of each row mirrors fish/zsh.
    fn render_typeahead(&self, frame: &mut ratatui::Frame<'_>, message_area: Rect) {
        use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
        use ratatui::style::{Modifier, Style};

        let items = self.typeahead.items();
        if items.is_empty() { return; }
        let visible = items.len().min(6);
        let height = (visible as u16) + 2; // +2 for top/bottom border

        // Width: longest candidate + "/ " prefix + 4 padding, capped to area.
        let max_len = items.iter().map(|s| s.len()).max().unwrap_or(0) + 6;
        let width = (max_len as u16).min(message_area.width.saturating_sub(2)).max(20);

        let popup = Rect {
            x: message_area.x + 2,
            y: message_area.y + message_area.height.saturating_sub(height + 1),
            width,
            height,
        };

        // Clear the underlying region so we draw on a clean canvas.
        frame.render_widget(ratatui::widgets::Clear, popup);

        let list_items: Vec<ListItem> = items
            .iter()
            .take(visible)
            .map(|name| ListItem::new(format!(" /{}", name)))
            .collect();

        let list = List::new(list_items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(self.theme.border)
                    .title(format!(" Slash {}/{} ", self.typeahead.selected() + 1, items.len())),
            )
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let mut state = ListState::default();
        state.select(Some(self.typeahead.selected()));

        frame.render_stateful_widget(list, popup, &mut state);
    }

    /// Render the @-file picker as a bordered popup anchored above the
    /// chat input, taller than the slash typeahead (15 visible rows).
    fn render_file_picker(
        &self,
        frame: &mut ratatui::Frame<'_>,
        conv_area: Rect,
        picker: &Picker<PathBuf>,
    ) {
        // Reserve roughly half the conversation height, capped at 17 rows
        // (15 visible + 2 borders) — the underlying Picker pages internally.
        let height = conv_area.height.min(17).max(5);
        let width = conv_area.width.saturating_sub(4).min(60).max(20);
        let x = conv_area.x + 2;
        let y = conv_area.y + conv_area.height.saturating_sub(height + 1);
        let area = Rect { x, y, width, height };

        frame.render_widget(ratatui::widgets::Clear, area);
        picker.render(frame, area, &self.theme);
    }

    fn render_spinner(&self, frame: &mut ratatui::Frame<'_>, message_area: Rect) {
        // Render spinner at the bottom of the message area.
        if message_area.height < 2 {
            return;
        }
        let spinner_area = Rect {
            x: message_area.x + 1,
            y: message_area.y + message_area.height - 1,
            width: message_area.width.saturating_sub(2),
            height: 1,
        };

        let tip = SPINNER_TIPS[self.spinner_tip_index % SPINNER_TIPS.len()];
        let widget = SpinnerWidget::new(self.spinner_frame, "Thinking...", &self.theme)
            .tip(tip);

        frame.render_widget(widget, spinner_area);
    }

    fn render_permission_dialog(
        &self,
        frame: &mut ratatui::Frame<'_>,
        area: Rect,
        approval: &PendingApproval,
    ) {
        let perm_type = match approval.request.tool_name.as_str() {
            name if name.contains("shell") || name.contains("bash") => PermissionType::Bash,
            name if name.contains("edit") => PermissionType::FileEdit,
            name if name.contains("write") => PermissionType::FileWrite,
            name if name.contains("fetch") || name.contains("web") => PermissionType::WebFetch,
            name if name.contains("read") => PermissionType::FileRead,
            _ => PermissionType::Bash,
        };

        // Build detail string from arguments.
        let mut detail = format!("Tool: {}", approval.request.tool_name);
        if let Some(cmd) = approval.request.arguments.get("command").and_then(|v| v.as_str()) {
            detail.push_str(&format!("\nCommand: {}", cmd));
        }
        if let Some(path) = approval
            .request
            .arguments
            .get("path")
            .or_else(|| approval.request.arguments.get("file_path"))
            .and_then(|v| v.as_str())
        {
            detail.push_str(&format!("\nPath: {}", path));
        }

        let widget = PermissionDialogWidget::from_detail(perm_type, &detail, &self.theme);
        frame.render_widget(widget, area);
    }
}

// ---------------------------------------------------------------------------
// Key action enum
// ---------------------------------------------------------------------------

enum KeyAction {
    None,
    Submit(String),
    Interrupt,
    ApprovalDecision {
        request: ApprovalRequest,
        decision: PermissionDecision,
    },
}

// ---------------------------------------------------------------------------
// Terminal lifecycle helpers
// ---------------------------------------------------------------------------

/// Which screen mode to render the TUI in. `Inline(N)` keeps shell
/// scrollback visible (paste-friendly, claude-code style); `Fullscreen`
/// takes over via alternate screen (classic TUI).
#[derive(Debug, Clone, Copy)]
pub enum UiViewport {
    Inline(u16),
    Fullscreen,
}

impl UiViewport {
    /// Parse from a string ("inline" / "fullscreen"). Default Inline(30).
    pub fn parse(mode: Option<&str>, inline_rows: Option<u16>) -> Self {
        match mode.unwrap_or("inline") {
            "fullscreen" | "full" | "fs" => UiViewport::Fullscreen,
            _ => UiViewport::Inline(inline_rows.unwrap_or(30)),
        }
    }
}

fn setup_terminal(viewport: UiViewport) -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // Mouse capture for click/scroll/drag in either mode.
    execute!(stdout, crossterm::event::EnableMouseCapture)?;
    // Alternate screen ONLY for Fullscreen — Inline mode renders within
    // the existing terminal so scrollback above stays visible after exit.
    if matches!(viewport, UiViewport::Fullscreen) {
        execute!(stdout, EnterAlternateScreen)?;
    }
    let backend = CrosstermBackend::new(stdout);
    let terminal = match viewport {
        UiViewport::Fullscreen => Terminal::new(backend)?,
        UiViewport::Inline(rows) => Terminal::with_options(
            backend,
            ratatui::TerminalOptions {
                viewport: ratatui::Viewport::Inline(rows),
            },
        )?,
    };
    Ok(terminal)
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    viewport: UiViewport,
) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture,
    )?;
    if matches!(viewport, UiViewport::Fullscreen) {
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    }
    terminal.show_cursor()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the ratatui-based TUI in either Inline or Fullscreen mode (see
/// `UiViewport`). Owns the terminal lifecycle for the chosen mode and
/// returns when the user exits.
pub async fn run_tui(
    agent: &BaseAgent,
    config: &ProviderConfig,
    workspace: &Path,
    paths: &AlvaPaths,
    session_manager: &JsonFileSessionManager,
    checkpoint_mgr: &checkpoint::CheckpointManager,
    approval_rx: &mut mpsc::UnboundedReceiver<ApprovalRequest>,
    viewport: UiViewport,
) -> Result<(), Box<dyn std::error::Error>> {
    // -- Session setup (same logic as run_repl) --
    let (mut session_id, mut active_session) = match session_manager.latest() {
        Some(id) => match session_manager.load(&id).await {
            Some(sess) => {
                agent.swap_session(sess.clone()).await;
                (id, sess)
            }
            None => {
                let sess = session_manager.create("").await;
                let id = sess.session_id().to_string();
                agent.swap_session(sess.clone()).await;
                (id, sess)
            }
        },
        None => {
            let sess = session_manager.create("").await;
            let id = sess.session_id().to_string();
            agent.swap_session(sess.clone()).await;
            (id, sess)
        }
    };

    // -- Initialize terminal --
    let mut terminal = setup_terminal(viewport)?;

    // -- Build app state --
    let mut app = TuiApp::new(&config.model, &session_id);
    app.set_workspace(workspace.to_path_buf());

    // Welcome message
    app.push_system_message(&format!(
        "Alva Agent v{} | Model: {} | Workspace: {}",
        env!("CARGO_PKG_VERSION"),
        config.model,
        workspace.display()
    ));
    app.push_system_message("Type /help for commands, Ctrl+D to exit.");

    // Initial draw
    app.draw(&mut terminal)?;

    // -- Main event loop --
    // We use a channel-based approach: agent events and terminal events are
    // processed in a select loop.
    let mut agent_event_rx: Option<mpsc::UnboundedReceiver<AgentEvent>> = None;

    loop {
        // Poll terminal events (non-blocking with tick rate).
        // We use tokio::task::block_in_place to allow the async runtime to
        // continue processing while we poll for terminal events.
        let term_event = {
            let tick = TICK_RATE;
            tokio::task::block_in_place(|| poll_event(tick))
        };

        // Handle terminal event
        if let Some(event) = term_event {
            match event {
                TerminalEvent::Mouse(mouse) => {
                    let area = terminal.get_frame().area();
                    app.on_mouse(mouse, area);
                }
                TerminalEvent::Key(key) => {
                    let action = app.on_key(key);
                    match action {
                        KeyAction::Submit(text) => {
                            // Handle slash commands locally.
                            if text.starts_with('/') {
                                handle_slash_command(
                                    &mut app,
                                    &text,
                                    agent,
                                    config,
                                    workspace,
                                    paths,
                                    session_manager,
                                    checkpoint_mgr,
                                    &mut session_id,
                                    &mut active_session,
                                )
                                .await;
                            } else if text.starts_with('!') {
                                // Shell command
                                let shell_cmd = text[1..].trim();
                                if !shell_cmd.is_empty() {
                                    handle_shell_command(&mut app, shell_cmd, workspace);
                                }
                            } else {
                                // Regular prompt
                                app.push_user_message(&text);
                                app.spinner_active = true;
                                agent_event_rx = Some(agent.prompt_text(&text));
                            }
                        }
                        KeyAction::Interrupt => {
                            if app.spinner_active {
                                agent.cancel();
                                app.spinner_active = false;
                                app.push_system_message("Interrupted.");
                            }
                        }
                        KeyAction::ApprovalDecision { request, decision } => {
                            agent
                                .resolve_permission(
                                    &request.request_id,
                                    &request.tool_name,
                                    decision,
                                )
                                .await;
                        }
                        KeyAction::None => {}
                    }
                }
                TerminalEvent::Resize(_, _) => {
                    // Terminal will auto-resize on next draw.
                }
                TerminalEvent::Tick => {
                    // Advance spinner
                    app.tick_count += 1;
                    if app.spinner_active {
                        app.spinner_frame = (app.spinner_frame + 1) % SPINNER_FRAMES.len();
                        // Rotate tips every ~3 seconds
                        if app.tick_count % 90 == 0 {
                            app.spinner_tip_index += 1;
                        }
                    }
                }
            }
        }

        // Drain agent events (non-blocking)
        if let Some(ref mut rx) = agent_event_rx {
            loop {
                match rx.try_recv() {
                    Ok(event) => {
                        handle_agent_event(&mut app, event);
                    }
                    Err(mpsc::error::TryRecvError::Empty) => break,
                    Err(mpsc::error::TryRecvError::Disconnected) => {
                        // Agent finished; stop spinner, refresh index summary.
                        app.spinner_active = false;
                        let event_count = active_session.count(&EventQuery::default()).await;
                        session_manager.refresh_summary(&session_id, event_count, None);
                        agent_event_rx = None;
                        break;
                    }
                }
            }
        }

        // Drain approval requests (non-blocking)
        loop {
            match approval_rx.try_recv() {
                Ok(req) => {
                    app.pending_approval = Some(PendingApproval { request: req });
                }
                Err(_) => break,
            }
        }

        // Check quit flag
        if app.should_quit {
            break;
        }

        // Redraw
        app.draw(&mut terminal)?;
    }

    // -- Final flush --
    let _ = active_session.flush().await;

    // -- Restore terminal --
    restore_terminal(&mut terminal, viewport)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Agent event handler
// ---------------------------------------------------------------------------

fn handle_agent_event(app: &mut TuiApp, event: AgentEvent) {
    match event {
        AgentEvent::AgentStart => {
            app.spinner_active = true;
        }
        AgentEvent::MessageStart { .. } => {
            app.begin_streaming();
        }
        AgentEvent::MessageUpdate { delta, .. } => {
            use alva_kernel_abi::StreamEvent;
            match &delta {
                StreamEvent::TextDelta { text } => app.append_streaming_text(text),
                StreamEvent::ReasoningDelta { text } => app.append_thinking(text),
                StreamEvent::ReasoningBlock { text, .. } => {
                    app.seal_thinking(Some(text));
                }
                _ => {}
            }
        }
        AgentEvent::MessageEnd { message } => {
            app.end_streaming();
            // Defensive: if the model emitted reasoning deltas without a
            // ReasoningBlock seal, close the block now using the streamed text.
            app.seal_thinking(None);
            if let AgentMessage::Standard(msg) = &message {
                if let Some(usage) = &msg.usage {
                    app.total_input_tokens += usage.input_tokens;
                    app.total_output_tokens += usage.output_tokens;
                }
            }
        }
        AgentEvent::MessageError { error, .. } => {
            app.end_streaming();
            app.push_error_message(&error);
        }
        AgentEvent::ToolExecutionStart { tool_call } => {
            let input_summary = tool_call
                .arguments
                .get("command")
                .or_else(|| tool_call.arguments.get("path"))
                .or_else(|| tool_call.arguments.get("file_path"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            app.add_tool_start(&tool_call.name, &input_summary);
        }
        AgentEvent::ToolExecutionEnd { tool_call, result } => {
            app.complete_tool(&tool_call.name, result.is_error, &result.model_text());
        }
        AgentEvent::AgentEnd { error } => {
            app.spinner_active = false;
            if let Some(e) = error {
                app.push_error_message(&e);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Slash command handler
// ---------------------------------------------------------------------------

async fn handle_slash_command(
    app: &mut TuiApp,
    cmd: &str,
    agent: &BaseAgent,
    config: &ProviderConfig,
    workspace: &Path,
    _paths: &AlvaPaths,
    session_manager: &JsonFileSessionManager,
    _checkpoint_mgr: &checkpoint::CheckpointManager,
    session_id: &mut String,
    active_session: &mut std::sync::Arc<JsonFileAgentSession>,
) {
    match cmd {
        "/quit" | "/exit" => {
            app.should_quit = true;
        }
        "/help" => {
            app.push_system_message(
                "Commands:\n\
                 /new        Start a fresh session\n\
                 /fork       Fork current session\n\
                 /resume     Resume a saved session\n\
                 /sessions   List all sessions\n\
                 /plan       Toggle plan mode (read-only)\n\
                 /model [id] Switch model\n\
                 /clear      Clear messages\n\
                 /config     Show current config\n\
                 /help       Show this help\n\
                 /quit /exit Exit\n\
                 \n\
                 !cmd        Run shell command directly",
            );
        }
        "/clear" => {
            app.reset_conversation();
            app.push_system_message("Screen cleared.");
        }
        "/config" => {
            app.push_system_message(&format!(
                "Model:     {}\n\
                 Base URL:  {}\n\
                 Workspace: {}\n\
                 Session:   {}",
                config.model,
                config.base_url,
                workspace.display(),
                session_id,
            ));
        }
        "/plan" => {
            use alva_app_core::PermissionMode;
            let current = agent.permission_mode();
            let new_mode = if current == PermissionMode::Plan {
                PermissionMode::Ask
            } else {
                PermissionMode::Plan
            };
            agent.set_permission_mode(new_mode);
            if new_mode == PermissionMode::Plan {
                app.push_system_message("Plan mode ON -- read-only, no file changes or commands");
            } else {
                app.push_system_message("Plan mode OFF -- tools can modify files");
            }
        }
        "/auto" => {
            use alva_app_core::PermissionMode;
            let current = agent.permission_mode();
            let new_mode = if current == PermissionMode::AcceptShell {
                PermissionMode::Ask
            } else {
                PermissionMode::AcceptShell
            };
            agent.set_permission_mode(new_mode);
            if new_mode == PermissionMode::AcceptShell {
                app.push_system_message("Auto-shell ON -- non-destructive shell commands run without prompting");
            } else {
                app.push_system_message("Auto-shell OFF -- shell commands ask for approval");
            }
        }
        "/new" => {
            let _ = active_session.flush().await;
            let new_session = session_manager.create("").await;
            *session_id = new_session.session_id().to_string();
            agent.swap_session(new_session.clone()).await;
            *active_session = new_session;
            app.session_id = session_id.clone();
            app.reset_conversation();
            app.total_input_tokens = 0;
            app.total_output_tokens = 0;
            app.push_system_message(&format!("New session: {}", session_id));
        }
        "/fork" => {
            let _ = active_session.flush().await;
            let old_id = session_id.clone();
            let messages = agent.messages().await;
            let new_session = session_manager.create("").await;
            *session_id = new_session.session_id().to_string();
            // Copy messages into new session
            for msg in &messages {
                new_session.append_message(msg.clone(), None).await;
            }
            agent.swap_session(new_session.clone()).await;
            *active_session = new_session;
            app.session_id = session_id.clone();
            app.push_system_message(&format!(
                "Forked from {} -> {}\n{} messages carried over.",
                &old_id[..8.min(old_id.len())],
                &session_id[..8.min(session_id.len())],
                messages.len()
            ));
        }
        "/sessions" => {
            let sessions = session_manager.list();
            if sessions.is_empty() {
                app.push_system_message("No sessions.");
            } else {
                let mut info = String::from("Sessions:\n");
                for s in sessions.iter().take(20) {
                    let date = chrono::DateTime::from_timestamp_millis(s.updated_at)
                        .map(|d| d.format("%m-%d %H:%M").to_string())
                        .unwrap_or_default();
                    let marker = if s.session_id == *session_id { " <-" } else { "" };
                    info.push_str(&format!(
                        "  {} | {} events | {}{}\n",
                        date, s.event_count, s.preview, marker,
                    ));
                }
                app.push_system_message(&info);
            }
        }
        cmd if cmd.starts_with("/model ") => {
            let model_id = cmd.strip_prefix("/model ").unwrap().trim();
            if model_id.is_empty() {
                let current = agent.model_id().await;
                app.push_system_message(&format!("Current model: {}", current));
            } else {
                let mut new_config = config.clone();
                new_config.model = model_id.to_string();
                let new_model = Arc::new(OpenAIChatProvider::new(new_config));
                agent.set_model(new_model).await;
                app.model_name = model_id.to_string();
                app.push_system_message(&format!("Switched to model: {}", model_id));
            }
        }
        "/model" => {
            let current = agent.model_id().await;
            app.push_system_message(&format!(
                "Current model: {}\nUsage: /model <model_id>",
                current
            ));
        }
        _ => {
            app.push_error_message(&format!("Unknown command: {}\nType /help for available commands.", cmd));
        }
    }
}

// ---------------------------------------------------------------------------
// Shell command handler
// ---------------------------------------------------------------------------

fn handle_shell_command(app: &mut TuiApp, shell_cmd: &str, workspace: &Path) {
    match std::process::Command::new("sh")
        .arg("-c")
        .arg(shell_cmd)
        .current_dir(workspace)
        .output()
    {
        Ok(out) => {
            let mut output = String::new();
            if !out.stdout.is_empty() {
                output.push_str(&String::from_utf8_lossy(&out.stdout));
            }
            if !out.stderr.is_empty() {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&String::from_utf8_lossy(&out.stderr));
            }
            if !out.status.success() {
                output.push_str(&format!(
                    "\n(exit code: {})",
                    out.status.code().unwrap_or(-1)
                ));
            }
            if output.is_empty() {
                output = "(no output)".to_string();
            }
            app.push_system_message(&format!("$ {}\n{}", shell_cmd, output));
        }
        Err(e) => {
            app.push_error_message(&format!("Failed to execute: {}", e));
        }
    }
}

// ---------------------------------------------------------------------------
// Misc helpers
// ---------------------------------------------------------------------------

/// Trim a string to `max` chars, appending an ellipsis when cut. Used for
/// the one-line tool-call header summary.
fn truncate(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max { return s.to_string(); }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// Flatten a Text body back to a plain String so we can append to it. Used
/// during reasoning streaming where the body is rebuilt each delta.
fn body_text(block: &CollapsibleBlock) -> String {
    block
        .body
        .lines
        .iter()
        .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format a char count as "1.2K" / "823" for the thinking-block badge.
fn format_chars(n: usize) -> String {
    if n < 1000 { format!("{} chars", n) }
    else if n < 1_000_000 { format!("{:.1}K chars", n as f64 / 1000.0) }
    else { format!("{:.1}M chars", n as f64 / 1_000_000.0) }
}

/// Locate the byte offset of the active `@`-token in `buf`. Returns the
/// position of the `@` itself, or `None` if there is no @-word at the end.
/// Mirrors the rule ChatInput uses to detect the @-trigger.
fn find_at_token_start(buf: &str) -> Option<usize> {
    for (i, c) in buf.char_indices().rev() {
        if c.is_whitespace() { return None; }
        if c == '@' {
            let prev_is_boundary = i == 0
                || buf[..i].chars().last().map(|p| p.is_whitespace()).unwrap_or(true);
            return prev_is_boundary.then_some(i);
        }
    }
    None
}

/// One-shot scan of the workspace for files, capped at `limit` entries to
/// keep the picker responsive on huge repos. Respects `.gitignore` and skips
/// hidden files via the `ignore` crate.
fn scan_workspace_files(root: &Path, limit: usize) -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(limit.min(64));
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();
    for entry in walker {
        let Ok(e) = entry else { continue; };
        if !e.file_type().map(|t| t.is_file()).unwrap_or(false) { continue; }
        out.push(e.into_path());
        if out.len() >= limit { break; }
    }
    out
}
