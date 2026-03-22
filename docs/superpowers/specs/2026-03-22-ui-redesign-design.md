# UI Redesign: Agent Management Platform — Phase 1

## Overview

Redesign the srow-agent desktop app from a basic 3-panel chat interface into a modern Agent management platform. Phase 1 focuses on: core conversation experience, real-time Agent call chain visualization, and Agent/Skill management dialogs.

Target quality bar: Claude Desktop / ChatGPT level conversation UX, with added Agent orchestration visibility.

## Core Concept Model

- **Agent** — configurable AI entity with system prompt, model, tools, skills, knowledge base, and optional sandbox execution
- **Skill** — pluggable capability module, sourced from GitHub repositories, versioned, referenced (not owned) by Agents
- **Context** — runtime context (files, knowledge base) bound to an Agent, loaded on demand
- **Session** — conversation record with a main Agent that can dynamically dispatch to sub-Agents

Key architectural principle: everything is **pluggable and peer-level**. The AI decides at runtime how to compose Agents, Skills, and Context. The main Agent can dynamically enhance sub-Agents with additional skills/context before dispatching.

## Layout

### Two layout modes

**Default — Two columns** (no Agent selected):

```
┌── 240px ──┬── flex-1 ─────────────────────┐
│           │                               │
│  Sidebar  │         Chat Area             │
│           │                               │
└───────────┴───────────────────────────────┘
```

**Expanded — Three columns** (Agent block clicked):

```
┌── 240px ──┬── flex-1 ─────────┬── 320px ──┐
│           │                   │           │
│  Sidebar  │    Chat Area      │  Agent    │
│           │                   │  Detail   │
│           │                   │  Panel    │
└───────────┴───────────────────┴───────────┘
```

### Transition rules

- Click any Agent block (running or completed) in chat → right panel slides in (320px)
- Click ✕ on panel or click outside → panel slides out, returns to two columns
- Click a different Agent block → panel content switches, panel stays open
- Chat Area width adjusts naturally (flex-1), smooth transition animation

## Sidebar

```
┌─────────────────────┐
│  ┌───────────────┐  │
│  │ 🔍 Search...  │  │  ← Filter sessions
│  └───────────────┘  │
│                     │
│  ┌───────────────┐  │
│  │  📋 Agents    │  │  ← Opens Agents Dialog
│  ├───────────────┤  │
│  │  ⚡ Skills    │  │  ← Opens Skills Dialog
│  └───────────────┘  │     (vertical list, extensible)
│                     │
│  ─────────────────  │  ← Separator
│  ┌─ + New Chat ──┐  │
│  └───────────────┘  │
│                     │
│  Today              │  ← Time-grouped sessions
│    💬 帮我重构模块   │     (Today/Yesterday/Last 7d/Earlier)
│    💬 写个测试       │
│  Yesterday          │
│    💬 分析日志       │
│  Last 7 Days        │
│    💬 部署脚本       │
│                     │
│  ─────────────────  │
│  ⚙ Settings         │  ← Fixed at bottom
└─────────────────────┘
```

### Sidebar components

**Search box**: filters sessions by name/content match, real-time.

**Management buttons**: vertical list of buttons that open full Dialog pages. Currently Agents and Skills. Extensible — future additions (Context, MCP Servers, etc.) stack vertically.

**Session list**: grouped by time (Today / Yesterday / Last 7 Days / Earlier). Each item shows chat icon + first message summary (truncated). Selected state: accent bg + white text. Hover: surface_hover bg.

**Settings**: fixed at bottom, opens Settings Dialog (same pattern as current SettingsPanel but in a Dialog).

## Chat Area

### Structure (top to bottom)

```
┌──────────────────────────────────────────┐
│  Session Title                     ⋯     │  ← Top bar
│─────────────────────────────────────────│
│                                          │
│  [Message stream - scrollable]           │  ← Messages
│                                          │
│─── Running Agents ─────────────────────│  ← Separator (hidden when empty)
│                                          │
│  [Running Agent blocks]                  │  ← Pinned above input
│                                          │
│  ┌────────────────────────────────────┐  │
│  │ Input area                        │  │  ← Input box
│  └────────────────────────────────────┘  │
│  Enter send · Shift+Enter newline        │  ← Hint text
└──────────────────────────────────────────┘
```

### Message types

**User message:**
- Right-aligned bubble, accent background, white text
- Supports multi-line, preserves whitespace

**Assistant message:**
- Left-aligned, surface background
- Full Markdown rendering: bold, italic, lists, links, tables
- Code blocks: dark background, syntax highlighting, language label, Copy button
- Hover: shows action icons (copy, retry)

**Tool call block:**
- Left-aligned, bordered, collapsible
- Icon + tool name + status indicator (⟳ running / ✅ success / ❌ error)
- Collapsed: one-line summary. Expanded: input + output

**Agent block (completed):**
- Left-aligned, bordered, colored by result (success green tint / error red tint)
- Shows: Agent icon + name + one-line result summary + `[Details →]` link
- Click anywhere → opens Agent Detail Panel on the right

**Thinking block:**
- Left-aligned, muted background, italic text
- Default collapsed (shows "💭 Thinking..."), click to expand full reasoning

**System message:**
- Centered, muted text, no bubble
- For status updates, errors, session info

### Running Agents zone

- Located between message stream and input box, separated by `── Running Agents ──` divider
- Each running Agent shows: icon + name + spinner + progress text + current skill
- Multiple running Agents stack vertically
- Click any → opens Agent Detail Panel
- When an Agent completes: animate out of running zone → insert as completed Agent block into message stream
- Divider hides when no Agents are running

## Input Area

```
┌────────────────────────────────────────┐
│                                        │
│  Type a message...                     │  ← Multi-line textarea
│                                        │     Auto-grows 1 line → max ~6 lines
├────────────────────────────────────────┤
│  📎 Attach  │  🤖 Main Agent ▾  │ Send │  ← Toolbar
└────────────────────────────────────────┘
  Enter send · Shift+Enter newline
```

### Components

| Element | Behavior |
|---------|----------|
| Textarea | Multi-line, auto-grow (1→6 lines), scrolls beyond max |
| 📎 Attach | File picker, attaches as context for the message (Phase 2 detail) |
| 🤖 Agent selector | Dropdown to choose which Agent handles this message. Default: Main Agent |
| Send / Stop | Primary button. While Agent is running, becomes ■ Stop |

### States

- No session selected: input disabled, placeholder "Select or create a chat"
- Agent running: input enabled (queues next message), Send becomes Stop
- Agent selector change: subsequent messages route to selected Agent

## Agent Detail Panel (right side, on-demand)

Width 320px, slides in from right when an Agent block is clicked.

### Layout

```
┌──────────────────────┐
│ Agent Name         ✕ │  ← Header + close
│──────────────────────│
│                      │
│  Status              │
│  ● Running / ✅ Done │
│  Started: 12:03:45   │
│  Duration: 2.3s      │
│                      │
│──────────────────────│
│                      │
│  Skills Loaded       │
│  ⚡ analyze           │
│  ⚡ refactor          │
│                      │
│──────────────────────│
│                      │
│  Context             │
│  📄 engine.rs         │
│  📄 mod.rs            │
│  📋 prompt.md         │
│                      │
│──────────────────────│
│                      │
│  Activity Log        │  ← Scrollable, auto-scroll when running
│  12:03:45 loading    │
│    skill: analyze    │
│  12:03:46 reading    │
│    engine.rs         │
│  12:03:47 found      │
│    issue #1: ...     │
│                      │
│──────────────────────│
│                      │
│  Config              │  ← Collapsible section
│  Model: claude-so... │
│  System Prompt: ...  │
│                      │
└──────────────────────┘
```

### Running vs Completed states

**Running**: status shows real-time state, Activity Log auto-scrolls, skills/context may update as Agent dynamically loads them.

**Completed**: status shows final result + duration, Activity Log is full history, bottom shows result summary.

## Agents Dialog

Opened by clicking `📋 Agents` button in Sidebar. Modal overlay, centered, ~640px wide, max 80vh tall.

### List view

```
┌──────────────────────────────────────────────────┐
│   Agents                              ✕ Close   │
│   ┌──────────────────────────────────────────┐   │
│   │ 🔍 Search agents...                      │   │
│   └──────────────────────────────────────────┘   │
│   ┌─ + Create Agent ────────────────────────┐   │
│   └─────────────────────────────────────────┘   │
│   ┌─────────────────────────────────────────┐   │
│   │ 🤖 Main Agent                     Edit  │   │
│   │ 默认主 Agent，负责分发和协调              │   │
│   │ Skills: analyze, refactor, bash         │   │
│   │ Model: claude-sonnet-4-6                │   │
│   ├─────────────────────────────────────────┤   │
│   │ 🔍 CodeReview                     Edit  │   │
│   │ 代码审查专用，关注质量和安全              │   │
│   │ Skills: analyze, security_scan          │   │
│   │ Model: claude-sonnet-4-6                │   │
│   └─────────────────────────────────────────┘   │
└──────────────────────────────────────────────────┘
```

### Edit view (in-Dialog navigation)

Click Edit → dialog content switches to edit form. ← Back returns to list.

**Edit form fields:**
- Name (text input)
- Description (text input)
- Model (dropdown)
- System Prompt (multi-line textarea)
- Skills (tag list with ✕ remove + [+ Add] button, references by name)
- Knowledge Base: Agent's dedicated directory path, file list with index status, load strategy (Always / On Demand), [Change Directory] + [Re-index] buttons
- Sandbox: radio Local / Sandbox, endpoint URL when sandbox selected
- [Delete Agent] danger button at bottom
- [Save] primary button in header

## Skills Dialog

Opened by clicking `⚡ Skills` button in Sidebar. Same modal pattern as Agents Dialog.

### List view

```
┌──────────────────────────────────────────────────┐
│   Skills                              ✕ Close   │
│   ┌──────────────────────────────────────────┐   │
│   │ 🔍 Search skills...                      │   │
│   └──────────────────────────────────────────┘   │
│   ┌─ + Create Skill ───────────────────────┐    │
│   ├─ + Import from GitHub ─────────────────┤    │
│   └─────────────────────────────────────────┘   │
│   ┌─────────────────────────────────────────┐   │
│   │ ⚡ analyze                    v1.2  Edit │   │
│   │ 分析代码结构、依赖关系、复杂度             │   │
│   │ 📦 github:srow/skills#analyze           │   │
│   │ Used by: Main Agent, CodeReview         │   │
│   │                         🔄 Update avail │   │
│   ├─────────────────────────────────────────┤   │
│   │ ⚡ security_scan             v2.0   Edit │   │
│   │ OWASP Top 10 安全扫描                    │   │
│   │ 📦 github:srow/skills#security          │   │
│   │ Used by: CodeReview                     │   │
│   │                              ✅ Latest  │   │
│   └─────────────────────────────────────────┘   │
└──────────────────────────────────────────────────┘
```

### Edit view fields

- Name (text input)
- Description (multi-line)
- Source: radio GitHub Repository / Local
  - GitHub: `owner/repo#skill_name`, branch, version display, [Check Update] + [Force Re-download]
  - Local: file path
- [Delete Skill] danger button
- [Save] primary button

### Skill model

- Skills are **referenced, not owned** — multiple Agents can reference the same skill
- Skills have **versions** from their GitHub source
- Update = re-download from GitHub repository, overwriting local copy
- Import from GitHub = enter `owner/repo#skill_name`, downloads and registers

## Component Requirements (Phase 1)

### New components to build

| Component | Type | Description |
|-----------|------|-------------|
| `SessionList` | View | Time-grouped session list with search |
| `ManagementButtons` | View | Vertical button list (Agents, Skills, ...) |
| `MessageBubble` | View | User/assistant message with Markdown rendering |
| `CodeBlock` | View | Syntax-highlighted code with copy button |
| `ToolCallBlock` | View | Collapsible tool call display |
| `AgentBlock` | View | Running/completed Agent display in chat |
| `ThinkingBlock` | View | Collapsible reasoning display |
| `RunningAgentsZone` | View | Pinned area above input for active Agents |
| `ChatInput` | View | Multi-line input with toolbar (attach, agent selector, send/stop) |
| `AgentDetailPanel` | View | Right-side sliding panel with Agent info |
| `AgentsDialog` | Dialog | Full Agent CRUD with list/edit views |
| `SkillsDialog` | Dialog | Full Skill CRUD with list/edit/import views |
| `SettingsDialog` | Dialog | Settings (migrated from panel to dialog) |

### Existing components to modify

| Component | Change |
|-----------|--------|
| `RootView` | Two-column default, conditional third column |
| `SidePanel` | Replace with new Sidebar layout |
| `ChatPanel` | Restructure with new message types and running zone |
| `InputBox` | Replace with ChatInput (multi-line + toolbar) |
| `MessageList` | Replace with new message type rendering |

### Dependencies

- Markdown rendering: need a Markdown → GPUI element parser (or render to styled text)
- Syntax highlighting: need a code highlighter compatible with GPUI (tree-sitter or syntect)
- Dialog/Modal: gpui-component already provides `Modal`

## Domain Model Notes

Phase 1 is UI-only for Agent and Skill management. The current domain types (`AgentConfig`, `AgentTemplate`, `Skill`, `SkillKind`) will need extension in Phase 2 to support the full data model. For Phase 1:

- **Agents Dialog / Skills Dialog**: build the UI components with local view-model structs. Wire to actual persistence/domain types in Phase 2.
- **Session**: the existing `Session` struct needs `created_at` and `updated_at` fields for time-grouping. Add a `summary` field (derived from first user message, truncated) for sidebar display.
- **Skills vs Tools**: in the UI, "Skills" is the user-facing term for pluggable capability modules. "Tools" refers to individual tool calls within a message (bash, read_file, etc.). These are distinct concepts in the UI even though they share underlying infrastructure.

## Interaction Details

### Running Agents lifecycle

1. Main Agent dispatches a sub-Agent → sub-Agent block appears in Running Agents zone (above input)
2. While running: block shows spinner, progress, current skill. Clickable → opens detail panel
3. On completion: block animates out of running zone → a completed Agent block is inserted **at the end of the message stream** (chronological order, like a new message arriving)
4. If user has scrolled up: a "↓ New activity" indicator appears at bottom, chat does NOT auto-scroll (standard chat behavior)
5. Running Agents zone **pushes** the scroll area up (not overlay) — messages scroll above it

### Agent selector

- The Agent selector in the input toolbar sets the **session-level default Agent**. It persists for the session.
- This is the Agent that receives the user's message. That Agent may then dispatch to sub-Agents at its discretion.
- If a sub-Agent is already running, the user can still send a new message — it queues for the selected Agent.

### Scroll behavior

- Auto-scroll to bottom on new messages (user or assistant)
- Auto-scroll pauses when user scrolls up manually
- "↓ New activity" button appears when paused + new messages arrive
- Running Agents zone is not part of the scroll area — it's pinned between scroll and input

### Agent Detail Panel dismissal

- Close button (✕) in panel header
- ESC key
- Clicking on sidebar or a non-Agent area in chat closes the panel
- Clicking another Agent block switches panel content (does not close)

### Empty states

- **New session (no messages)**: centered placeholder "Start a conversation" with the selected Agent's name and icon
- **Agents Dialog (no agents)**: "No agents configured. Create your first agent." + Create button
- **Skills Dialog (no skills)**: "No skills installed. Create or import a skill." + Create/Import buttons
- **Search (no results)**: "No matches found" inline text
- **Error in chat**: error message block with red tint, retry button if applicable

### Dialog scroll behavior

- Search box and Create/Import buttons are **sticky at top** (do not scroll)
- Item list scrolls independently below
- Dialog body max height 80vh, internally scrollable

### Phase 1 placeholder elements

- **📎 Attach button**: rendered but disabled, tooltip "Coming soon"
- **Knowledge Base section in Agent Edit**: UI renders but [Re-index] is disabled, shows "Indexing not available yet"
- **Skill Import from GitHub**: UI renders the input, but submit shows "Import not available yet"
- **Settings**: SettingsDialog reuses the existing SettingsPanel layout wrapped in a Dialog/Modal. No redesign needed.

### Code blocks and message actions

- **Copy button**: top-right corner of code block, icon-only (📋), click copies content to clipboard. On success: icon briefly changes to ✅ (1.5s)
- **Language label**: top-left of code block (e.g., "rust", "python"), small muted text
- **Message hover actions**: appear as small icon row at top-right corner of assistant message bubble on hover. Icons: 📋 Copy (full message), 🔄 Retry (re-sends the preceding user message to regenerate this response). Retry removes all messages after the re-sent user message.

### Agent Detail Panel — Config section

- **Read-only**. Shows the Agent's runtime configuration for inspection only.
- To edit Agent config, use the Agents Dialog (click Edit from the Agent list).

## Out of Scope (Phase 1)

- File drag-and-drop / upload (📎 button renders disabled)
- Session rename / delete (right-click menu)
- Message edit / branch
- Agent runtime dynamic enhancement UI (AI handles this via API, not UI)
- Knowledge base indexing backend (UI placeholder only)
- Skill GitHub import backend (UI placeholder only)
- Responsive / narrow screen handling
- Keyboard shortcuts beyond Enter/Shift+Enter/ESC
