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
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Terminal;

use alva_app_core::{AgentEvent, AgentMessage, AlvaPaths, BaseAgent, PermissionDecision};
use alva_host_native::middleware::security::ApprovalRequest;
use alva_llm_provider::{OpenAIChatProvider, ProviderConfig};
use tokio::sync::mpsc;

use crate::checkpoint;
use crate::session_store::SessionStore;

use super::event::{poll_event, TerminalEvent};
use super::message_list::{DisplayMessage, MessageListWidget, MessageRole, ToolStatus, ToolUseDisplay};
use super::permission_dialog::{PermissionDialogWidget, PermissionType};
use super::prompt_input::{InputMode, PromptInputWidget};
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
// TuiApp
// ---------------------------------------------------------------------------

/// Full-screen terminal UI application state.
pub struct TuiApp {
    // -- UI state --
    messages: Vec<DisplayMessage>,
    input_buffer: String,
    cursor_col: usize,
    input_mode: InputMode,
    scroll_offset: u16,
    /// Total content height in lines (approximated after each render).
    content_height: u16,
    auto_scroll: bool,

    // -- spinner --
    spinner_active: bool,
    spinner_frame: usize,
    spinner_tip_index: usize,
    tick_count: u64,

    // -- session info --
    model_name: String,
    session_id: String,
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
    /// Text accumulated during assistant streaming.
    streaming_text: String,
    is_streaming: bool,

    // -- theme --
    theme: Theme,

    // -- exit flag --
    should_quit: bool,
}

impl TuiApp {
    /// Create a new TUI app with the given model name and session ID.
    pub fn new(model_name: &str, session_id: &str) -> Self {
        Self {
            messages: Vec::new(),
            input_buffer: String::new(),
            cursor_col: 0,
            input_mode: InputMode::Normal,
            scroll_offset: 0,
            content_height: 0,
            auto_scroll: true,

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

            streaming_text: String::new(),
            is_streaming: false,

            theme: Theme::new(ThemeMode::Dark),

            should_quit: false,
        }
    }

    // -- state mutation helpers ------------------------------------------------

    fn push_message(&mut self, msg: DisplayMessage) {
        self.messages.push(msg);
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    fn push_user_message(&mut self, text: &str) {
        self.push_message(DisplayMessage {
            role: MessageRole::User,
            content: text.to_string(),
            tool_uses: Vec::new(),
            timestamp: Some(chrono::Local::now().format("%H:%M:%S").to_string()),
            is_streaming: false,
        });
    }

    fn push_system_message(&mut self, text: &str) {
        self.push_message(DisplayMessage {
            role: MessageRole::System,
            content: text.to_string(),
            tool_uses: Vec::new(),
            timestamp: None,
            is_streaming: false,
        });
    }

    fn push_error_message(&mut self, text: &str) {
        self.push_message(DisplayMessage {
            role: MessageRole::Error,
            content: text.to_string(),
            tool_uses: Vec::new(),
            timestamp: None,
            is_streaming: false,
        });
    }

    fn begin_streaming(&mut self) {
        self.streaming_text.clear();
        self.is_streaming = true;
        self.push_message(DisplayMessage {
            role: MessageRole::Assistant,
            content: String::new(),
            tool_uses: Vec::new(),
            timestamp: Some(chrono::Local::now().format("%H:%M:%S").to_string()),
            is_streaming: true,
        });
    }

    fn append_streaming_text(&mut self, text: &str) {
        self.streaming_text.push_str(text);
        if let Some(last) = self.messages.last_mut() {
            last.content = self.streaming_text.clone();
        }
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    fn end_streaming(&mut self) {
        self.is_streaming = false;
        if let Some(last) = self.messages.last_mut() {
            last.is_streaming = false;
            last.content = self.streaming_text.clone();
        }
        self.streaming_text.clear();
    }

    fn add_tool_start(&mut self, name: &str, input_summary: &str) {
        // Attach to the last assistant message, or create one.
        if self.messages.last().map_or(true, |m| m.role != MessageRole::Assistant) {
            self.push_message(DisplayMessage {
                role: MessageRole::Assistant,
                content: String::new(),
                tool_uses: Vec::new(),
                timestamp: None,
                is_streaming: false,
            });
        }
        if let Some(last) = self.messages.last_mut() {
            last.tool_uses.push(ToolUseDisplay {
                name: name.to_string(),
                status: ToolStatus::Running,
                input_summary: input_summary.to_string(),
                output_preview: String::new(),
            });
        }
        if self.auto_scroll {
            self.scroll_to_bottom();
        }
    }

    fn complete_tool(&mut self, name: &str, is_error: bool, preview: &str) {
        // Find the last tool use with this name that is still running.
        for msg in self.messages.iter_mut().rev() {
            for tool in msg.tool_uses.iter_mut().rev() {
                if tool.name == name && tool.status == ToolStatus::Running {
                    tool.status = if is_error {
                        ToolStatus::Error
                    } else {
                        ToolStatus::Success
                    };
                    let preview_clean = preview.replace('\n', " ");
                    tool.output_preview = if preview_clean.len() > 100 {
                        format!("{}...", &preview_clean[..100])
                    } else {
                        preview_clean
                    };
                    return;
                }
            }
        }
    }

    fn scroll_to_bottom(&mut self) {
        // Will be clamped during rendering.
        self.scroll_offset = self.content_height.saturating_sub(1);
    }

    fn scroll_up(&mut self, lines: u16) {
        self.auto_scroll = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    fn scroll_down(&mut self, lines: u16, visible_height: u16) {
        let max = self.content_height.saturating_sub(visible_height);
        self.scroll_offset = (self.scroll_offset + lines).min(max);
        if self.scroll_offset >= max {
            self.auto_scroll = true;
        }
    }

    // -- input helpers --------------------------------------------------------

    fn submit_input(&mut self) -> Option<String> {
        let text = self.input_buffer.trim().to_string();
        if text.is_empty() {
            return None;
        }
        // Save to history.
        self.input_history.push(text.clone());
        self.history_index = None;
        self.saved_input.clear();
        self.input_buffer.clear();
        self.cursor_col = 0;
        self.input_mode = InputMode::Normal;
        self.auto_scroll = true;
        Some(text)
    }

    fn history_prev(&mut self) {
        if self.input_history.is_empty() {
            return;
        }
        match self.history_index {
            None => {
                self.saved_input = self.input_buffer.clone();
                let idx = self.input_history.len() - 1;
                self.history_index = Some(idx);
                self.input_buffer = self.input_history[idx].clone();
            }
            Some(idx) if idx > 0 => {
                let new_idx = idx - 1;
                self.history_index = Some(new_idx);
                self.input_buffer = self.input_history[new_idx].clone();
            }
            _ => {}
        }
        self.cursor_col = self.input_buffer.len();
    }

    fn history_next(&mut self) {
        match self.history_index {
            Some(idx) => {
                if idx + 1 < self.input_history.len() {
                    let new_idx = idx + 1;
                    self.history_index = Some(new_idx);
                    self.input_buffer = self.input_history[new_idx].clone();
                } else {
                    self.history_index = None;
                    self.input_buffer = self.saved_input.clone();
                    self.saved_input.clear();
                }
            }
            None => {}
        }
        self.cursor_col = self.input_buffer.len();
    }

    fn detect_input_mode(&mut self) {
        if self.input_buffer.starts_with('/') {
            self.input_mode = InputMode::Command;
        } else if self.input_buffer.starts_with('!') {
            self.input_mode = InputMode::Shell;
        } else {
            self.input_mode = InputMode::Normal;
        }
    }

    // -- keyboard handling ----------------------------------------------------

    /// Process a key event. Returns `true` if the event was consumed.
    fn on_key(&mut self, key: KeyEvent) -> KeyAction {
        // If a permission dialog is active, handle it first.
        if self.pending_approval.is_some() {
            return self.on_key_approval(key);
        }

        match (key.modifiers, key.code) {
            // Exit
            (KeyModifiers::CONTROL, KeyCode::Char('d')) => {
                self.should_quit = true;
                KeyAction::None
            }
            // Interrupt
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => KeyAction::Interrupt,
            // Submit
            (_, KeyCode::Enter) => {
                if let Some(text) = self.submit_input() {
                    KeyAction::Submit(text)
                } else {
                    KeyAction::None
                }
            }
            // Escape
            (_, KeyCode::Esc) => {
                self.input_buffer.clear();
                self.cursor_col = 0;
                self.input_mode = InputMode::Normal;
                KeyAction::None
            }
            // History navigation
            (_, KeyCode::Up) => {
                self.history_prev();
                self.detect_input_mode();
                KeyAction::None
            }
            (_, KeyCode::Down) => {
                self.history_next();
                self.detect_input_mode();
                KeyAction::None
            }
            // Scrolling
            (_, KeyCode::PageUp) => {
                self.scroll_up(10);
                KeyAction::None
            }
            (_, KeyCode::PageDown) => {
                // visible_height will be applied during render; use a reasonable default.
                self.scroll_down(10, 20);
                KeyAction::None
            }
            // Cursor movement within input
            (_, KeyCode::Left) => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
                KeyAction::None
            }
            (_, KeyCode::Right) => {
                if self.cursor_col < self.input_buffer.len() {
                    self.cursor_col += 1;
                }
                KeyAction::None
            }
            (_, KeyCode::Home) => {
                self.cursor_col = 0;
                KeyAction::None
            }
            (_, KeyCode::End) => {
                self.cursor_col = self.input_buffer.len();
                KeyAction::None
            }
            // Backspace
            (_, KeyCode::Backspace) => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                    self.input_buffer.remove(self.cursor_col);
                    self.detect_input_mode();
                }
                KeyAction::None
            }
            // Delete
            (_, KeyCode::Delete) => {
                if self.cursor_col < self.input_buffer.len() {
                    self.input_buffer.remove(self.cursor_col);
                }
                KeyAction::None
            }
            // Character input
            (_, KeyCode::Char(c)) => {
                self.input_buffer.insert(self.cursor_col, c);
                self.cursor_col += 1;
                self.detect_input_mode();
                KeyAction::None
            }
            _ => KeyAction::None,
        }
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

    /// Process a mouse event (vim-style mouse support).
    fn on_mouse(&mut self, mouse: MouseEvent, area: Rect) {
        // Determine which UI zone the event is in based on y coordinate.
        // Layout: status_bar(1) | messages(fill) | input(3)
        let status_h = 1u16;
        let input_h = 3u16;
        let msg_y_start = area.y + status_h;
        let msg_y_end = area.y + area.height.saturating_sub(input_h);
        let msg_height = msg_y_end.saturating_sub(msg_y_start);

        match mouse.kind {
            // Scroll up in message area
            MouseEventKind::ScrollUp => {
                if mouse.row >= msg_y_start && mouse.row < msg_y_end {
                    self.scroll_up(3);
                }
            }
            // Scroll down in message area
            MouseEventKind::ScrollDown => {
                if mouse.row >= msg_y_start && mouse.row < msg_y_end {
                    self.scroll_down(3, msg_height);
                }
            }
            // Left click in input area → focus input (move cursor)
            MouseEventKind::Down(MouseButton::Left) => {
                if mouse.row >= msg_y_end {
                    // Clicked in input area — estimate cursor position
                    let prefix_len = self.input_mode.prefix().len() as u16;
                    let click_col = mouse.column.saturating_sub(area.x + 1 + prefix_len) as usize;
                    self.cursor_col = click_col.min(self.input_buffer.len());
                } else if mouse.row >= msg_y_start && mouse.row < msg_y_end {
                    // Clicked in message area — disable auto-scroll (allows reading)
                    self.auto_scroll = false;
                }
            }
            // Double-click or right-click in message area → scroll to bottom
            MouseEventKind::Down(MouseButton::Right) => {
                if mouse.row >= msg_y_start && mouse.row < msg_y_end {
                    self.auto_scroll = true;
                    self.scroll_to_bottom();
                }
            }
            // Drag in message area → scroll proportionally
            MouseEventKind::Drag(MouseButton::Left) => {
                if mouse.row >= msg_y_start && mouse.row < msg_y_end && msg_height > 0 {
                    let relative_y = mouse.row.saturating_sub(msg_y_start);
                    let ratio = relative_y as f64 / msg_height as f64;
                    let target = (self.content_height as f64 * ratio) as u16;
                    self.scroll_offset = target.min(
                        self.content_height.saturating_sub(msg_height),
                    );
                    self.auto_scroll = false;
                }
            }
            _ => {}
        }
    }

    // -- rendering ------------------------------------------------------------

    fn draw(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
        terminal.draw(|frame| {
            let area = frame.area();

            // Layout: status bar (1) | messages (fill) | input (3)
            let chunks = Layout::vertical([
                Constraint::Length(1),  // status bar
                Constraint::Min(3),    // message list
                Constraint::Length(3), // prompt input
            ])
            .split(area);

            let status_area = chunks[0];
            let message_area = chunks[1];
            let input_area = chunks[2];

            // -- Status bar --
            self.render_status_bar(frame, status_area);

            // -- Message list --
            self.render_messages(frame, message_area);

            // -- Input prompt --
            self.render_input(frame, input_area);

            // -- Spinner overlay (below messages, above input) --
            if self.spinner_active {
                self.render_spinner(frame, message_area);
            }

            // -- Permission dialog overlay --
            if let Some(ref approval) = self.pending_approval {
                self.render_permission_dialog(frame, area, approval);
            }

            // -- Set cursor position --
            if self.pending_approval.is_none() {
                let prefix_len = self.input_mode.prefix().len() as u16;
                // Input area: block border (top) + 1 line padding = first content line
                let cursor_x = input_area.x + 1 + prefix_len + self.cursor_col as u16;
                let cursor_y = input_area.y + 1; // after the top border
                frame.set_cursor_position((cursor_x, cursor_y));
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

    fn render_messages(&mut self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let block = Block::default()
            .borders(Borders::NONE);

        // Estimate total content height for scrolling.
        // Each message: 1 header + content lines + blank separator + tool lines
        let mut total_lines: u16 = 0;
        for (idx, msg) in self.messages.iter().enumerate() {
            if idx > 0 {
                total_lines += 1; // separator
            }
            total_lines += 1; // header
            total_lines += msg.content.lines().count().max(1) as u16;
            for tool in &msg.tool_uses {
                total_lines += 1; // tool line
                if !tool.output_preview.is_empty() {
                    total_lines += 1;
                }
            }
        }
        self.content_height = total_lines;

        // Clamp scroll offset
        let inner_height = area.height;
        if self.auto_scroll {
            self.scroll_offset = total_lines.saturating_sub(inner_height);
        } else {
            let max = total_lines.saturating_sub(inner_height);
            self.scroll_offset = self.scroll_offset.min(max);
        }

        let widget = MessageListWidget::new(&self.messages, &self.theme)
            .block(block)
            .scroll(self.scroll_offset);

        frame.render_widget(widget, area);
    }

    fn render_input(&self, frame: &mut ratatui::Frame<'_>, area: Rect) {
        let total_tokens = self.total_input_tokens + self.total_output_tokens;
        let widget = PromptInputWidget::new(&self.input_buffer, &self.theme)
            .cursor(self.cursor_col)
            .mode(self.input_mode)
            .model_name(&self.model_name)
            .token_count(total_tokens);

        frame.render_widget(widget, area);
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

fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    // Enable mouse capture (click, scroll, drag — vim-style mouse mode)
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the full-screen TUI as an alternative to `run_repl`.
///
/// This function owns the terminal lifecycle (raw mode, alternate screen)
/// and returns when the user exits.
pub async fn run_tui(
    agent: &BaseAgent,
    config: &ProviderConfig,
    workspace: &Path,
    paths: &AlvaPaths,
    store: &SessionStore,
    checkpoint_mgr: &checkpoint::CheckpointManager,
    approval_rx: &mut mpsc::UnboundedReceiver<ApprovalRequest>,
) -> Result<(), Box<dyn std::error::Error>> {
    // -- Session setup (same logic as run_repl) --
    let mut session_id = match store.latest() {
        Some(id) => {
            let sessions = store.list();
            let _meta = sessions.iter().find(|m| m.id == id);

            agent.new_session().await;
            let saved = store.load_messages(&id);
            if !saved.is_empty() {
                crate::repl::restore_messages(agent, saved).await;
            }
            id
        }
        None => store.create(""),
    };

    // -- Initialize terminal --
    let mut terminal = setup_terminal()?;

    // -- Build app state --
    let mut app = TuiApp::new(&config.model, &session_id);

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
                                    store,
                                    checkpoint_mgr,
                                    &mut session_id,
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
                        // Agent finished; stop spinner, save session.
                        app.spinner_active = false;
                        let messages = agent.messages().await;
                        store.save_messages(&session_id, &messages);
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

    // -- Final save --
    let messages = agent.messages().await;
    store.save_messages(&session_id, &messages);

    // -- Restore terminal --
    restore_terminal(&mut terminal)?;

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
            if let alva_kernel_abi::StreamEvent::TextDelta { text } = &delta {
                app.append_streaming_text(text);
            }
        }
        AgentEvent::MessageEnd { message } => {
            app.end_streaming();
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
    store: &SessionStore,
    _checkpoint_mgr: &checkpoint::CheckpointManager,
    session_id: &mut String,
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
            app.messages.clear();
            app.scroll_offset = 0;
            app.content_height = 0;
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
        "/new" => {
            let messages = agent.messages().await;
            store.save_messages(session_id, &messages);
            agent.new_session().await;
            *session_id = store.create("");
            app.session_id = session_id.clone();
            app.messages.clear();
            app.scroll_offset = 0;
            app.content_height = 0;
            app.total_input_tokens = 0;
            app.total_output_tokens = 0;
            app.push_system_message(&format!("New session: {}", session_id));
        }
        "/fork" => {
            let messages = agent.messages().await;
            store.save_messages(session_id, &messages);
            let old_id = session_id.clone();
            *session_id = store.create("");
            store.save_messages(session_id, &messages);
            app.session_id = session_id.clone();
            app.push_system_message(&format!(
                "Forked from {} -> {}\n{} messages carried over.",
                &old_id[..8.min(old_id.len())],
                &session_id[..8.min(session_id.len())],
                messages.len()
            ));
        }
        "/sessions" => {
            let sessions = store.list();
            if sessions.is_empty() {
                app.push_system_message("No sessions.");
            } else {
                let mut info = String::from("Sessions:\n");
                for s in sessions.iter().take(20) {
                    let date = chrono::DateTime::from_timestamp_millis(s.updated_at)
                        .map(|d| d.format("%m-%d %H:%M").to_string())
                        .unwrap_or_default();
                    let marker = if s.id == *session_id { " <-" } else { "" };
                    info.push_str(&format!(
                        "  {} | {} msgs | {}{}\n",
                        date, s.message_count, s.summary, marker,
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
