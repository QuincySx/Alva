# UI Redesign: Agent Management Platform — Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Redesign srow-agent from a basic 3-panel chat interface into a modern Agent management platform with Markdown rendering, Agent call chain visualization, and Agent/Skill management dialogs.

**Architecture:** Two-column default layout (Sidebar + Chat), with on-demand third column (Agent Detail Panel) that slides in when an Agent block is clicked. Running Agents pin above the input box while active, then fold into message stream on completion. Agent and Skill management via full-screen Dialogs.

**Tech Stack:** GPUI 0.2, gpui-component 0.5 (Dialog, List, Input, Button, Select), pulldown-cmark (Markdown parsing), syntect (syntax highlighting), existing alva-app-core/srow-ai infrastructure.

**Spec:** `docs/superpowers/specs/2026-03-22-ui-redesign-design.md`

---

## File Structure

### New files

| File | Responsibility |
|------|----------------|
| `crates/alva-app/src/views/sidebar/mod.rs` | New sidebar module |
| `crates/alva-app/src/views/sidebar/sidebar.rs` | Sidebar container: search + buttons + session list + settings |
| `crates/alva-app/src/views/sidebar/session_list.rs` | Time-grouped session list with search filter |
| `crates/alva-app/src/views/sidebar/management_buttons.rs` | Vertical button list (Agents, Skills, ...) |
| `crates/alva-app/src/views/chat_panel/markdown.rs` | Markdown → GPUI element renderer |
| `crates/alva-app/src/views/chat_panel/code_block.rs` | Syntax-highlighted code block with copy button |
| `crates/alva-app/src/views/chat_panel/message_bubble.rs` | User/assistant message with Markdown rendering |
| `crates/alva-app/src/views/chat_panel/tool_call_block.rs` | Collapsible tool call display |
| `crates/alva-app/src/views/chat_panel/thinking_block.rs` | Collapsible reasoning display |
| `crates/alva-app/src/views/chat_panel/agent_block.rs` | Running/completed Agent block in chat |
| `crates/alva-app/src/views/chat_panel/running_agents_zone.rs` | Pinned area above input for active Agents |
| `crates/alva-app/src/views/chat_panel/chat_input.rs` | Multi-line input with toolbar |
| `crates/alva-app/src/views/agent_detail_panel.rs` | Right-side sliding Agent detail panel |
| `crates/alva-app/src/views/dialogs/mod.rs` | Dialogs module |
| `crates/alva-app/src/views/dialogs/agents_dialog.rs` | Agent CRUD dialog (list + edit views) |
| `crates/alva-app/src/views/dialogs/skills_dialog.rs` | Skill CRUD dialog (list + edit + import views) |
| `crates/alva-app/src/views/dialogs/settings_dialog.rs` | Settings dialog (wraps existing SettingsPanel) |

### Files to modify

| File | Change |
|------|--------|
| `crates/alva-app/Cargo.toml` | Add pulldown-cmark, syntect |
| `crates/alva-app/src/lib.rs` | No change needed (views module covers new files) |
| `crates/alva-app/src/views/mod.rs` | Add new submodules: sidebar, dialogs, agent_detail_panel |
| `crates/alva-app/src/views/root_view.rs` | Two-column default, conditional third column |
| `crates/alva-app/src/views/chat_panel/mod.rs` | Add new component modules |
| `crates/alva-app/src/views/chat_panel/chat_panel.rs` | Restructure with running zone + new message rendering |
| `crates/alva-app/src/views/chat_panel/message_list.rs` | Replace with new message type rendering |
| `crates/alva-app/src/views/chat_panel/input_box.rs` | Replace with chat_input.rs (or rewrite in-place) |
| `crates/alva-app/src/models/workspace_model.rs` | Add summary field, time-group helpers |

### Files to remove (replaced)

| File | Replaced by |
|------|-------------|
| `crates/alva-app/src/views/side_panel/` | `crates/alva-app/src/views/sidebar/` |
| `crates/alva-app/src/views/agent_panel/` | `crates/alva-app/src/views/agent_detail_panel.rs` (on-demand) |

---

## Task 1: Dependencies + Layout Restructure

**Files:**
- Modify: `crates/alva-app/Cargo.toml`
- Modify: `crates/alva-app/src/views/root_view.rs`
- Modify: `crates/alva-app/src/views/mod.rs`

- [ ] **Step 1: Add new dependencies**

In `crates/alva-app/Cargo.toml`, add:
```toml
pulldown-cmark = "0.12"
syntect = { version = "5", default-features = false, features = ["default-syntaxes", "default-themes", "html", "regex-onig"] }
once_cell = "1"
```

Run: `cargo check -p alva-app`

- [ ] **Step 2: Restructure RootView to two-column default**

Read current `root_view.rs`. It has 3 fixed columns (SidePanel 220px | ChatPanel flex | AgentPanel 280px).

Change to:
- Two columns by default: Sidebar 240px | ChatPanel flex-1
- Add an `Option<AgentDetailContext>` field to track which Agent block is selected
- When `Some(agent)` → render third column (AgentDetailPanel 320px) with slide animation
- Remove the permanent AgentPanel

```rust
pub struct RootView {
    sidebar: Entity<Sidebar>,          // was: side_panel
    chat_panel: Entity<ChatPanel>,
    // agent_panel removed — now on-demand
    selected_agent: Option<AgentDetailContext>,  // None = two columns
    settings_model: Entity<SettingsModel>,
}

// New type for tracking selected Agent
pub struct AgentDetailContext {
    pub agent_id: String,
    pub agent_name: String,
    pub is_running: bool,
}
```

Render:
```rust
fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    let theme = Theme::for_appearance(window, cx);

    let mut root = div()
        .flex()
        .flex_row()
        .size_full()
        .bg(theme.background);

    // Left: Sidebar (240px)
    root = root.child(
        div().w(px(240.)).flex_none()
            .border_r_1().border_color(theme.border)
            .child(self.sidebar.clone())
    );

    // Center: Chat (flex-1)
    root = root.child(
        div().flex_1().child(self.chat_panel.clone())
    );

    // Right: Agent Detail Panel (320px, conditional)
    if self.selected_agent.is_some() {
        root = root.child(
            div().w(px(320.)).flex_none()
                .border_l_1().border_color(theme.border)
                .child(/* AgentDetailPanel */)
        );
    }

    root
}
```

- [ ] **Step 3: Create placeholder Sidebar view**

Create `crates/alva-app/src/views/sidebar/mod.rs` and `sidebar.rs` with a minimal placeholder that compiles (just renders "+ New Chat" button and text "Sidebar"). This replaces the old SidePanel temporarily.

- [ ] **Step 4: Update views/mod.rs**

Add new modules, keep old ones temporarily for compilation:
```rust
pub mod sidebar;
pub mod chat_panel;
pub mod dialogs;
pub mod agent_detail_panel;
// Remove: side_panel, agent_panel (after replacing references)
```

- [ ] **Step 5: Update RootView constructor**

Update `RootView::new()` to create a `Sidebar` instead of `SidePanel`, remove `AgentPanel` creation. Pass necessary models to the new Sidebar.

- [ ] **Step 6: Verify compilation**

Run: `cargo check -p alva-app`

- [ ] **Step 7: Commit**

```bash
git add crates/alva-app/
git commit -m "refactor(alva-app): restructure layout to two-column default with sidebar"
```

---

## Task 2: Sidebar — Session List with Time Groups

**Files:**
- Create: `crates/alva-app/src/views/sidebar/session_list.rs`
- Create: `crates/alva-app/src/views/sidebar/management_buttons.rs`
- Modify: `crates/alva-app/src/views/sidebar/sidebar.rs`
- Modify: `crates/alva-app/src/views/sidebar/mod.rs`
- Modify: `crates/alva-app/src/models/workspace_model.rs`

- [ ] **Step 1: Add time-group helpers to WorkspaceModel**

In `workspace_model.rs`, add a method that groups sessions by time:

```rust
pub enum TimeGroup {
    Today,
    Yesterday,
    Last7Days,
    Earlier,
}

impl WorkspaceModel {
    pub fn sessions_by_time_group(&self) -> Vec<(TimeGroup, Vec<&SidebarItem>)> {
        // Group sidebar_items by created_at relative to now
        // Return sorted groups with their sessions
    }
}
```

Add a `summary` field to session display (truncated first message or session name).

**Note**: The current `WorkspaceModel` has `SidebarItem` variants (`GlobalSession` and `Workspace` with nested sessions). For this redesign, flatten all sessions into a single time-grouped list — ignore the workspace hierarchy in the sidebar display. The `sessions_by_time_group()` method should iterate both global sessions and workspace children, collecting all sessions with their timestamps.

- [ ] **Step 2: Create ManagementButtons component**

`management_buttons.rs` — a simple vertical list of buttons:

```rust
pub struct ManagementButtons;

impl ManagementButtons {
    pub fn render(cx: &mut App) -> impl IntoElement {
        div().flex().flex_col().gap_1().px_2()
            .child(Button::new("agents-btn").label("Agents").icon(/* ... */).w_full())
            .child(Button::new("skills-btn").label("Skills").icon(/* ... */).w_full())
    }
}
```

Buttons open Dialogs (placeholder on_click for now — just `tracing::info!`).

- [ ] **Step 3: Create SessionList component**

`session_list.rs` — renders time-grouped session list:

```rust
pub struct SessionList {
    workspace_model: Entity<WorkspaceModel>,
    search_query: String,
}
```

Render:
- For each time group (Today/Yesterday/Last 7 Days/Earlier), render a label + session items
- Each session item: chat icon + summary text (truncated), click selects session
- Selected state: accent bg + white text
- Filter by search_query (case-insensitive substring match on name/summary)

- [ ] **Step 4: Assemble full Sidebar**

`sidebar.rs` — compose all pieces:

```
Search input (gpui-component Input)
─
ManagementButtons (Agents, Skills)
─ (divider)
+ New Chat button
─
SessionList (scrollable, flex-1)
─ (divider)
⚙ Settings button (fixed at bottom)
```

Settings button: opens SettingsDialog (placeholder on_click for now).

- [ ] **Step 5: Wire up to RootView**

Pass `workspace_model`, `chat_model`, `agent_model`, `settings_model` to Sidebar constructor.

- [ ] **Step 6: Verify and commit**

Run: `cargo check -p alva-app`

```bash
git add crates/alva-app/
git commit -m "feat(alva-app): add new sidebar with time-grouped sessions and management buttons"
```

---

## Task 3: Markdown Renderer

**Files:**
- Create: `crates/alva-app/src/views/chat_panel/markdown.rs`
- Create: `crates/alva-app/src/views/chat_panel/code_block.rs`
- Modify: `crates/alva-app/src/views/chat_panel/mod.rs`

- [ ] **Step 1: Create CodeBlock component**

`code_block.rs` — renders a syntax-highlighted code block with copy button:

```rust
pub struct CodeBlock;

impl CodeBlock {
    pub fn render(code: &str, language: Option<&str>, theme: &Theme) -> impl IntoElement {
        // Use syntect to highlight code
        // Dark background, monospace font
        // Top bar: language label (left) + Copy button (right)
        // Copy button: clipboard icon, on_click copies code to clipboard
        // On success: icon briefly changes to ✅
    }
}
```

Use `syntect::parsing::SyntaxSet::load_defaults_newlines()` and `syntect::highlighting::ThemeSet::load_defaults()` for syntax highlighting. Render highlighted lines as colored `StyledText` spans.

**Note**: Syntect returns styled spans. Convert to GPUI `StyledText` with `HighlightStyle` for each span's foreground color.

- [ ] **Step 2: Create Markdown renderer**

`markdown.rs` — parses Markdown text and returns GPUI elements:

```rust
pub struct MarkdownRenderer;

impl MarkdownRenderer {
    pub fn render(text: &str, theme: &Theme) -> Vec<impl IntoElement> {
        // Parse with pulldown_cmark::Parser
        // Convert events to GPUI elements:
        //   - Paragraphs → div with text
        //   - Bold/Italic → StyledText with font weight/style
        //   - Code spans → monospace background
        //   - Code blocks → CodeBlock::render()
        //   - Lists (ordered/unordered) → indented items with bullets/numbers
        //   - Links → colored text (not clickable in Phase 1)
        //   - Headings → sized/weighted text
        //   - Horizontal rules → divider
    }
}
```

Start simple:
1. Parse with `pulldown_cmark::Parser::new(text)`
2. Iterate events, build GPUI elements
3. For code blocks, delegate to `CodeBlock::render()`
4. For inline formatting, use `StyledText` with `HighlightStyle`

- [ ] **Step 3: Add modules to chat_panel/mod.rs**

```rust
pub mod markdown;
pub mod code_block;
```

- [ ] **Step 4: Test markdown rendering**

Create a simple test that verifies the parser doesn't panic on various Markdown inputs:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn markdown_parser_handles_basic_input() {
        // Verify pulldown_cmark parses without panic
        let input = "# Hello\n\n**bold** and *italic*\n\n```rust\nfn main() {}\n```";
        let parser = pulldown_cmark::Parser::new(input);
        let events: Vec<_> = parser.collect();
        assert!(!events.is_empty());
    }
}
```

- [ ] **Step 5: Verify and commit**

Run: `cargo check -p alva-app && cargo test -p alva-app`

```bash
git add crates/alva-app/
git commit -m "feat(alva-app): add Markdown renderer and syntax-highlighted code blocks"
```

---

## Task 4: Message Type Components

**Files:**
- Create: `crates/alva-app/src/views/chat_panel/message_bubble.rs`
- Create: `crates/alva-app/src/views/chat_panel/tool_call_block.rs`
- Create: `crates/alva-app/src/views/chat_panel/thinking_block.rs`
- Create: `crates/alva-app/src/views/chat_panel/agent_block.rs`
- Modify: `crates/alva-app/src/views/chat_panel/mod.rs`

- [ ] **Step 1: Create MessageBubble**

`message_bubble.rs` — renders a single user or assistant message:

```rust
pub struct MessageBubble;

impl MessageBubble {
    pub fn render_user(text: &str, theme: &Theme) -> impl IntoElement {
        // Right-aligned, accent bg, white text, rounded bubble
        // px_4, py_2, max_w(px(600)), rounded_lg
    }

    pub fn render_assistant(text: &str, theme: &Theme) -> impl IntoElement {
        // Left-aligned, surface bg
        // Render text through MarkdownRenderer::render()
        // Hover: show action icons (copy, retry) at top-right
    }

    pub fn render_system(text: &str, theme: &Theme) -> impl IntoElement {
        // Centered, muted text, no bubble
    }
}
```

- [ ] **Step 2: Create ToolCallBlock**

`tool_call_block.rs` — collapsible tool call display:

```rust
// NOTE: ToolCallBlock and ThinkingBlock are stateless render helpers, NOT GPUI entities.
// Collapse state is owned by the parent MessageList via HashMap<String, bool>.
// The parent passes `collapsed: bool` and an on_click callback that toggles the state.
pub struct ToolCallBlock;

impl ToolCallBlock {
    pub fn render(
        tool_name: &str,
        input: &serde_json::Value,
        state: &ToolState,
        output: Option<&serde_json::Value>,
        error: Option<&str>,
        theme: &Theme,
    ) -> impl IntoElement {
        // Bordered div, left-aligned
        // Header: icon + tool_name + status (⟳/✅/❌)
        // Collapsed: one-line header only
        // Expanded: input + output sections
        // Click header toggles collapsed state
    }
}
```

Status mapping:
- `InputStreaming | InputAvailable | ApprovalRequested | ApprovalResponded` → ⟳ running (warning color)
- `OutputAvailable` → ✅ success (success color)
- `OutputError | OutputDenied` → ❌ error (error color)

- [ ] **Step 3: Create ThinkingBlock**

`thinking_block.rs` — collapsible reasoning block:

```rust
// Same pattern as ToolCallBlock — stateless helper, parent owns state.
pub struct ThinkingBlock;

impl ThinkingBlock {
    pub fn render(text: &str, state: &TextPartState, theme: &Theme) -> impl IntoElement {
        // Muted background, italic text
        // Header: "💭 Thinking..." (streaming) or "💭 Thought" (done)
        // Default collapsed: shows header only
        // Expanded: shows full reasoning text
        // Click header toggles
    }
}
```

- [ ] **Step 4: Create AgentBlock**

`agent_block.rs` — represents a sub-Agent call in the message stream:

```rust
pub struct AgentBlock;

impl AgentBlock {
    pub fn render_completed(
        agent_name: &str,
        summary: &str,
        success: bool,
        theme: &Theme,
        on_click: impl Fn() + 'static,
    ) -> impl IntoElement {
        // Bordered div, success/error tint
        // Icon + agent_name + one-line summary
        // Right side: "Details →" link
        // Click anywhere → on_click (opens detail panel)
    }

    pub fn render_running(
        agent_name: &str,
        progress: &str,
        current_skill: Option<&str>,
        theme: &Theme,
        on_click: impl Fn() + 'static,
    ) -> impl IntoElement {
        // Accent border, spinner icon
        // agent_name + progress text + skill name
        // Click → on_click (opens detail panel)
    }
}
```

- [ ] **Step 5: Add all modules to chat_panel/mod.rs**

```rust
pub mod message_bubble;
pub mod tool_call_block;
pub mod thinking_block;
pub mod agent_block;
```

- [ ] **Step 6: Verify and commit**

Run: `cargo check -p alva-app`

```bash
git add crates/alva-app/
git commit -m "feat(alva-app): add message type components — bubble, tool, thinking, agent blocks"
```

---

## Task 5: Message List Overhaul

**Files:**
- Modify: `crates/alva-app/src/views/chat_panel/message_list.rs`

- [ ] **Step 1: Rewrite message rendering**

Replace the current simple text-bubble rendering with the new component-based approach:

```rust
fn render_message_part(&self, part: &UIMessagePart, role: &UIMessageRole, theme: &Theme) -> impl IntoElement {
    match part {
        UIMessagePart::Text { text, state } => {
            match role {
                UIMessageRole::User => MessageBubble::render_user(text, theme),
                UIMessageRole::Assistant => MessageBubble::render_assistant(text, theme),
                UIMessageRole::System => MessageBubble::render_system(text, theme),
            }
        }
        UIMessagePart::Reasoning { text, state } => {
            ThinkingBlock::render(text, state, theme)
        }
        UIMessagePart::Tool { tool_name, input, state, output, error, .. } => {
            ToolCallBlock::render(tool_name, input, state, output.as_ref(), error.as_deref(), theme)
        }
        // Agent blocks rendered via Custom part type or separate mechanism
        _ => div() // Skip unhandled parts
    }
}
```

- [ ] **Step 2: Update chat header with session title**

Replace the static "Chat" header text with the current session's name/title. Add a "⋯" menu button placeholder (non-functional in Phase 1).

- [ ] **Step 3: Add empty state**

When no messages in session, show centered placeholder:
```
"Start a conversation"
with selected Agent's name and icon
```

- [ ] **Step 4: Add error message retry**

For assistant messages that ended in error, show a "🔄 Retry" button that re-sends the preceding user message. Use `UIMessagePart::Custom { id, data }` with `data: {"type": "error", "retryable": true}` to identify retryable errors.

- [ ] **Step 5: Add scroll behavior**

- Auto-scroll to bottom on new messages
- Pause auto-scroll when user scrolls up
- Show "↓ New activity" button when paused + new messages arrive

- [ ] **Step 6: Define Agent block representation in message stream**

Agent blocks in the message stream use `UIMessagePart::Custom` with a convention:
```rust
UIMessagePart::Custom {
    id: agent_run_id.to_string(),
    data: serde_json::json!({
        "type": "agent_block",
        "agent_name": "CodeReview",
        "status": "completed",  // or "error"
        "summary": "Found 3 issues, all fixed",
    })
}
```

The `render_message_part` function checks `data["type"] == "agent_block"` and delegates to `AgentBlock::render_completed(...)`. This avoids modifying `alva-app-core`'s `UIMessagePart` enum.

- [ ] **Step 7: Verify and commit**

Run: `cargo check -p alva-app`

```bash
git add crates/alva-app/
git commit -m "feat(alva-app): overhaul message list with Markdown, code blocks, and new message types"
```

---

## Task 6: Chat Input Redesign

**Files:**
- Create: `crates/alva-app/src/views/chat_panel/chat_input.rs`
- Modify: `crates/alva-app/src/views/chat_panel/chat_panel.rs`
- Modify: `crates/alva-app/src/views/chat_panel/mod.rs`

- [ ] **Step 1: Create ChatInput component**

`chat_input.rs`:

```rust
pub struct ChatInput {
    input_state: Entity<InputState>,
    agent_model: Entity<AgentModel>,
    workspace_model: Entity<WorkspaceModel>,
    selected_agent: String,  // currently selected agent name
}
```

Render layout:
```
┌────────────────────────────────────────┐
│  Multi-line textarea (auto-grow)       │
├────────────────────────────────────────┤
│  📎 Attach(disabled) │ 🤖 Agent ▾ │ Send │
└────────────────────────────────────────┘
  Enter send · Shift+Enter newline
```

- Textarea: use gpui-component `Input` or raw GPUI `div().editable()`. Auto-grow from 1 line to ~6 lines.
- Attach button: rendered disabled, use gpui-component `Tooltip` to show "Coming soon"
- Agent selector: start with a simple Button that cycles through agents on click (avoid gpui-component `Select` complexity for now). Display current agent name. If the `Select` component's `SelectDelegate` trait is straightforward to implement, use it instead.
- Send/Stop: Primary button. Running → Stop (■) icon.
- Enter to send, Shift+Enter for newline (handle via `InputEvent::PressEnter { secondary }`)

- [ ] **Step 2: Integrate into ChatPanel**

Replace old `InputBox` entity with `ChatInput` in `chat_panel.rs`. Wire up `send_message` to use the selected Agent.

- [ ] **Step 3: Remove old input_box.rs**

Delete `input_box.rs` or keep as reference, update `mod.rs`.

- [ ] **Step 4: Verify and commit**

Run: `cargo check -p alva-app`

```bash
git add crates/alva-app/
git commit -m "feat(alva-app): redesign chat input with multi-line, agent selector, and toolbar"
```

---

## Task 7: Running Agents Zone

**Files:**
- Create: `crates/alva-app/src/views/chat_panel/running_agents_zone.rs`
- Modify: `crates/alva-app/src/views/chat_panel/chat_panel.rs`

- [ ] **Step 1: Create RunningAgentsZone**

`running_agents_zone.rs`:

```rust
pub struct RunningAgentsZone {
    agent_model: Entity<AgentModel>,
}
```

Render:
- Query `agent_model` for all agents with `Running` status
- If none running → render nothing (zone hidden, no divider)
- If running agents exist → render:
  - `── Running Agents ──` divider
  - For each running agent: `AgentBlock::render_running(...)` with on_click that emits event to open detail panel

- [ ] **Step 2: Integrate into ChatPanel layout**

ChatPanel structure becomes:
```rust
div().flex_col().size_full()
    .child(/* header */)
    .child(/* message_list - flex_1, scrollable */)
    .child(RunningAgentsZone)  // between messages and input
    .child(ChatInput)
```

- [ ] **Step 3: Agent completion lifecycle**

When an Agent's status changes from Running to Idle/Error:
- Remove from RunningAgentsZone (automatic — model-driven via AgentModel)
- ChatPanel subscribes to `AgentModelEvent::StatusChanged`. On status change to Idle/Error:
  1. Create a `UIMessagePart::Custom` with `data: {"type": "agent_block", "agent_name": ..., "status": "completed"/"error", "summary": ...}`
  2. Append it as a new part to the current assistant message (or create a new system message)
  3. This is UI-layer only — no changes to alva-app-core needed
- The RunningAgentsZone automatically shrinks as agents complete (it reads from AgentModel)

- [ ] **Step 4: Verify and commit**

Run: `cargo check -p alva-app`

```bash
git add crates/alva-app/
git commit -m "feat(alva-app): add running agents zone pinned above chat input"
```

---

## Task 8: Agent Detail Panel

**Files:**
- Create: `crates/alva-app/src/views/agent_detail_panel.rs`
- Modify: `crates/alva-app/src/views/mod.rs`
- Modify: `crates/alva-app/src/views/root_view.rs`

- [ ] **Step 1: Create AgentDetailPanel**

`agent_detail_panel.rs`:

```rust
pub struct AgentDetailPanel {
    agent_context: AgentDetailContext,
    agent_model: Entity<AgentModel>,
}

pub enum AgentDetailPanelEvent {
    Close,
}
```

Render sections (scrollable):
1. **Header**: Agent name + ✕ close button
2. **Status**: colored dot + status text + timestamps
3. **Skills Loaded**: list of loaded skill names
4. **Context**: list of bound files/resources
5. **Activity Log**: scrollable log entries (auto-scroll when running)
6. **Config**: collapsible, read-only (model, system prompt)

Close button emits `AgentDetailPanelEvent::Close`.

- [ ] **Step 2: Wire into RootView**

- RootView subscribes to events from ChatPanel/RunningAgentsZone requesting detail panel
- On "open detail" event: set `selected_agent = Some(context)`, render third column
- On "close" event: set `selected_agent = None`, third column disappears
- ESC key binding to close panel
- Clicking on sidebar or non-agent areas in chat also closes the panel. Implement by adding a click handler on the Sidebar and ChatPanel containers that emits a "close detail panel" event when clicked (unless the click target is an Agent block).

- [ ] **Step 3: Verify and commit**

Run: `cargo check -p alva-app`

```bash
git add crates/alva-app/
git commit -m "feat(alva-app): add Agent detail panel with sliding right column"
```

---

## Task 9: Agents Dialog

**Files:**
- Create: `crates/alva-app/src/views/dialogs/mod.rs`
- Create: `crates/alva-app/src/views/dialogs/agents_dialog.rs`

- [ ] **Step 1: Create AgentsDialog — list view**

```rust
pub struct AgentsDialog {
    agents: Vec<AgentViewData>,  // local view-model
    search_query: String,
    editing: Option<usize>,  // index of agent being edited, None = list view
}

struct AgentViewData {
    id: String,
    name: String,
    description: String,
    model: String,
    system_prompt: String,
    skills: Vec<String>,
    knowledge_dir: Option<String>,
    sandbox: SandboxConfig,
}
```

List view render:
- Search box (sticky top)
- "+ Create Agent" button
- Agent cards: icon + name + description + skills list + model. Each has "Edit" button.
- Search filters by name/description

- [ ] **Step 2: Create AgentsDialog — edit view**

When `editing = Some(idx)`:
- Header: "← Back" + "Edit: {name}" + "Save" button
- Form fields: Name, Description, Model (dropdown), System Prompt (textarea), Skills (tag list), Knowledge Base (directory + file list + load strategy), Sandbox (radio)
- "Delete Agent" danger button at bottom
- Save validates and updates `agents[idx]`
- Back returns to list view

- [ ] **Step 3: Wire dialog opening from Sidebar**

In Sidebar's ManagementButtons, the "Agents" button click opens the dialog:
- Use gpui-component's `Modal` or `Dialog` component
- Dialog renders centered with overlay

- [ ] **Step 4: Verify and commit**

Run: `cargo check -p alva-app`

```bash
git add crates/alva-app/
git commit -m "feat(alva-app): add Agents dialog with list and edit views"
```

---

## Task 10: Skills Dialog

**Files:**
- Create: `crates/alva-app/src/views/dialogs/skills_dialog.rs`
- Modify: `crates/alva-app/src/views/dialogs/mod.rs`

- [ ] **Step 1: Create SkillsDialog — list view**

```rust
pub struct SkillsDialog {
    skills: Vec<SkillViewData>,
    search_query: String,
    editing: Option<usize>,
}

struct SkillViewData {
    id: String,
    name: String,
    description: String,
    version: Option<String>,
    source: SkillSource,
    used_by: Vec<String>,  // agent names
    update_available: bool,
}

enum SkillSource {
    GitHub { repo: String, branch: String },
    Local { path: String },
}
```

List view:
- Search box + "+ Create Skill" + "+ Import from GitHub" buttons
- Skill cards: icon + name + version + description + source + "Used by" + update status
- Update available → "🔄 Update avail" badge. Latest → "✅ Latest".

- [ ] **Step 2: Create SkillsDialog — edit view**

Form fields: Name, Description, Source (radio GitHub/Local with appropriate fields), [Check Update], [Delete Skill], [Save].

Import from GitHub: text input for `owner/repo#skill_name`, submit button (disabled placeholder "Import not available yet").

- [ ] **Step 3: Wire dialog from Sidebar**

"Skills" button in ManagementButtons opens SkillsDialog.

- [ ] **Step 4: Verify and commit**

Run: `cargo check -p alva-app`

```bash
git add crates/alva-app/
git commit -m "feat(alva-app): add Skills dialog with list, edit, and import views"
```

---

## Task 11: Settings Dialog + Cleanup

**Files:**
- Create: `crates/alva-app/src/views/dialogs/settings_dialog.rs`
- Modify: `crates/alva-app/src/views/sidebar/sidebar.rs`
- Remove: `crates/alva-app/src/views/side_panel/` (old sidebar)
- Remove: `crates/alva-app/src/views/agent_panel/` (old agent panel)

- [ ] **Step 1: Create SettingsDialog**

Wrap the existing `SettingsPanel` in a Dialog/Modal:

```rust
pub struct SettingsDialog {
    settings_panel: Entity<SettingsPanel>,
}
```

Uses gpui-component `Modal`. Content is the existing SettingsPanel (reused as-is).

- [ ] **Step 2: Wire Settings button in Sidebar**

Bottom "⚙ Settings" button opens SettingsDialog.

- [ ] **Step 3: Remove old side_panel and agent_panel**

Delete:
- `crates/alva-app/src/views/side_panel/` (entire directory)
- `crates/alva-app/src/views/agent_panel/` (entire directory)

Update `crates/alva-app/src/views/mod.rs` to remove references. **Keep `settings_panel` module** — it's still used inside `SettingsDialog`.

- [ ] **Step 4: Full verification**

Run: `cargo check -p alva-app && cargo test --workspace`

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(alva-app): add settings dialog, remove old panels, complete UI restructure"
```

---

## Implementation Notes

### GPUI Patterns to Follow

**Entity creation**: `cx.new(|cx| MyView::new(models..., window, cx))`

**Event emission**: `cx.emit(MyEvent::Something)` with `impl EventEmitter<MyEvent> for MyView {}`

**Subscriptions**: `cx.subscribe(&entity, |this, entity, event, cx| { ... }).detach()`

**Globals**: `cx.global::<SharedRuntime>()`, `cx.set_global(MyGlobal(value))`

**Theme access**: `let theme = Theme::for_appearance(window, cx);`

**Traced events**: Use `alva_app_debug::traced!` / `alva_app_debug::traced_listener!` macros on all new event handlers.

### Markdown Rendering Strategy

Start with a minimal Markdown renderer that handles:
1. Paragraphs (plain text blocks)
2. Bold / italic (via StyledText)
3. Code spans (monospace background)
4. Code blocks (delegate to CodeBlock)
5. Ordered / unordered lists
6. Headings (h1-h6 with size/weight)

Skip in Phase 1: tables, images, nested blockquotes, footnotes.

### Syntect Integration

```rust
use syntect::parsing::SyntaxSet;
use syntect::highlighting::{ThemeSet, Style};
use syntect::easy::HighlightLines;

lazy_static! {
    static ref SYNTAX_SET: SyntaxSet = SyntaxSet::load_defaults_newlines();
    static ref THEME_SET: ThemeSet = ThemeSet::load_defaults();
}
```

Use `lazy_static` or `once_cell::sync::Lazy` to avoid re-loading on every render. The `alva-app` Cargo.toml may need `lazy_static = "1"` or `once_cell = "1"` added.

### Dialog Pattern with gpui-component

Use `WindowExt` trait from `gpui_component` to open dialogs:

```rust
use gpui_component::WindowExt;

// Opening a dialog:
window.open_dialog(cx, |dialog, window, cx| {
    dialog
        .title("Agents")
        .child(cx.new(|cx| AgentsDialogContent::new(cx)))
});
```

The `Dialog` component is created by the `open_dialog` method and passed to the closure. Use `.title()`, `.child()`, and other builder methods on it. The dialog automatically handles overlay, centering, and ESC-to-close.

**Read the actual gpui-component Dialog source** (`~/.cargo/registry/src/*/gpui-component-*/src/dialog.rs`) before implementing to verify the exact builder API.

### Dialog Empty States

Both Agents and Skills dialogs must handle the empty case:
- **Agents**: "No agents configured. Create your first agent." + prominent Create button
- **Skills**: "No skills installed. Create or import a skill." + Create/Import buttons
- Render as a centered, muted placeholder replacing the list area

### Dialog Scroll Behavior

- Search box + Create/Import buttons: **fixed at top** (outside the scroll region)
- Item list: independently scrollable with `overflow_y_scroll()`
- Dialog body: `max_h(vh(80.))` with internal scroll
