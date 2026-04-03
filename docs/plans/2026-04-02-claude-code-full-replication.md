# Claude Code 100% Functionality Replication Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 100% replicate all Claude Code (`/src`) functionality into the Alva Rust codebase, mapping each module to the appropriate crate.

**Architecture:** Claude Code is a TypeScript/Ink terminal app with 1,900 files across 35 modules. Alva is a Rust workspace with 24 crates. We map each Claude Code module to the best-fit Alva crate, enhancing existing crates and creating new ones as needed. The CLI stays in `alva-app-cli`, core agent logic in `alva-agent-core`, tools in `alva-agent-tools`, and orchestration in `alva-app-core`.

**Tech Stack:** Rust, tokio, serde, ratatui (terminal UI), reqwest, chromiumoxide, sqlx

---

## Module → Crate Mapping

| Claude Code Module | Target Alva Crate | Action |
|---|---|---|
| Tool.ts (tool interface) | alva-types | Enhance Tool trait |
| Task.ts (task types) | alva-types | Add TaskState, TaskType |
| context.ts (system/user context) | alva-agent-context | Enhance |
| query.ts / QueryEngine.ts | alva-agent-core | Enhance run_agent |
| history.ts (session history) | alva-app-cli | Add history module |
| commands.ts (command system) | alva-app-cli | Add command system |
| Tools (44 tools) | alva-agent-tools | Enhance + add 28 new |
| Services/API | alva-provider | Enhance |
| Services/Compact | alva-agent-context | Add compaction |
| Services/SessionMemory | alva-agent-memory | Enhance |
| Services/TokenEstimation | alva-types | Add token counting |
| Services/OAuth | alva-app-core | Add OAuth module |
| Services/RateLimiting | alva-provider | Add rate limiting |
| Services/MCP | alva-protocol-mcp | Enhance |
| Services/Plugins | alva-app-core/plugins | Enhance |
| Services/Analytics | alva-app-core | Add analytics |
| Services/LSP | alva-app-core | Add LSP module |
| Services/Notifier | alva-app-cli | Add notifications |
| Services/Voice | alva-app-cli | Add voice input |
| Services/PreventSleep | alva-app-cli | Add sleep prevention |
| Services/Tips | alva-app-cli | Add tips system |
| State Management | alva-app-core | Add AppState |
| Permission System | alva-agent-security | Enhance |
| Settings System | alva-app-core | Add settings module |
| Swarm/Team | alva-app-core | Add swarm module |
| Terminal UI | alva-app-cli | Add ratatui UI |
| Hooks System | alva-app-core | Add hooks module |
| Constants | alva-types + alva-app-core | Add constants |

---

## Phase 1: Type Foundation & Constants

### Task 1.1: Enhanced Tool Trait (alva-types)

**Files:**
- Modify: `crates/alva-types/src/tool.rs`
- Create: `crates/alva-types/src/tool_metadata.rs`

**What to add (matching Claude Code's Tool.ts):**
- `is_concurrency_safe(&self, input) -> bool` (default false)
- `is_read_only(&self, input) -> bool` (default false)
- `is_destructive(&self, input) -> bool` (default false)
- `is_search_or_read_command(&self, input) -> Option<SearchReadInfo>`
- `check_permissions(&self, input, ctx) -> PermissionResult`
- `user_facing_name(&self, input) -> String`
- `max_result_size_chars(&self) -> Option<usize>`
- `should_defer(&self) -> bool` (default false)
- `aliases(&self) -> &[&str]` (default empty)
- `is_enabled(&self) -> bool` (default true)
- `prompt(&self, options) -> String` (tool-specific prompt)
- `ToolPermissionResult` enum: Allow, Deny(reason), Ask(question)

**Tests:**
- Default trait method behavior
- Permission result types

---

### Task 1.2: Task Types (alva-types)

**Files:**
- Create: `crates/alva-types/src/task.rs`
- Modify: `crates/alva-types/src/lib.rs`

**What to add (matching Claude Code's Task.ts):**
```rust
pub enum TaskType {
    LocalBash,
    LocalAgent,
    RemoteAgent,
    InProcessTeammate,
    LocalWorkflow,
    MonitorMcp,
    Dream,
}

pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
}

pub struct TaskState {
    pub id: String,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub description: String,
    pub tool_use_id: Option<String>,
    pub start_time: u64,
    pub end_time: Option<u64>,
    pub total_paused_ms: Option<u64>,
    pub output_file: PathBuf,
    pub output_offset: usize,
    pub notified: bool,
}

pub fn generate_task_id(task_type: &TaskType) -> String;
pub fn is_terminal_status(status: &TaskStatus) -> bool;
```

**Tests:**
- Task ID generation format (prefix + random)
- Terminal status check

---

### Task 1.3: Token Estimation (alva-types)

**Files:**
- Create: `crates/alva-types/src/token_estimation.rs`
- Modify: `crates/alva-types/src/lib.rs`

**What to add (matching Claude Code's tokenEstimation.ts):**
```rust
pub trait TokenEstimator: Send + Sync {
    fn estimate_tokens(&self, text: &str) -> usize;
    fn count_message_tokens(&self, messages: &[Message]) -> usize;
}

pub struct SimpleTokenEstimator; // ~4 chars per token approximation
```

**Tests:**
- Token estimation accuracy

---

### Task 1.4: Constants Module (alva-types)

**Files:**
- Create: `crates/alva-types/src/constants.rs`

**What to add (matching Claude Code's constants/):**
- Tool limits (max result sizes, timeouts)
- API limits (max tokens, max messages)
- System prompt section identifiers
- Agent tool availability matrix (which tools each agent type can use)

---

## Phase 2: Enhanced Existing Tools (alva-agent-tools)

### Task 2.1: FileWriteTool Enhancement

**Files:**
- Modify: `crates/alva-agent-tools/src/create_file.rs`

**Claude Code's FileWriteTool features missing:**
- Overwrite existing files (not just create)
- Line ending preservation (detect and maintain CRLF/LF)
- File staleness detection (warn if file changed since last read)
- Git diff generation for display
- File state tracking via read cache

---

### Task 2.2: GrepTool Enhancement

**Files:**
- Modify: `crates/alva-agent-tools/src/grep_search.rs`

**Claude Code's GrepTool features missing:**
- Multiple output modes: `content`, `files_with_matches`, `count`
- Context lines: `-B` (before), `-A` (after), `-C` (around)
- Line numbers: `-n`
- Case insensitive: `-i`
- File type filter: `type` parameter
- Head limit: `head_limit` with offset
- Multiline mode
- VCS directory exclusion (.git, node_modules)

---

### Task 2.3: BashTool Enhancement

**Files:**
- Modify: `crates/alva-agent-tools/src/execute_shell.rs`

**Claude Code's BashTool features missing:**
- Background task support (`run_in_background`)
- Environment variable injection (`env_vars` parameter)
- Timeout support (`timeout_ms` parameter)
- Image output detection and handling
- Git operation tracking
- Plugin hint detection
- Sandbox detection and enforcement

---

### Task 2.4: FileReadTool Enhancement

**Files:**
- Modify: `crates/alva-agent-tools/src/read_file.rs`

**Claude Code's FileReadTool features missing:**
- Offset/limit pagination (read specific line ranges)
- Line ending detection and reporting
- Encoding detection
- PDF reading with page selection (`pages` parameter)
- Jupyter notebook (.ipynb) reading with cell output
- Image file support (return as content block)
- File state tracking

---

### Task 2.5: FileEditTool Enhancement

**Files:**
- Modify: `crates/alva-agent-tools/src/file_edit.rs`

**Claude Code's FileEditTool features missing:**
- Staleness detection (file modified since last read)
- Quote normalization (smart quotes → ASCII quotes)
- `replace_all` parameter for global replacement
- File history tracking for undo

---

### Task 2.6: WebFetchTool Enhancement

**Files:**
- Modify: `crates/alva-agent-tools/src/read_url.rs`

**Claude Code's WebFetchTool features missing:**
- HTML to markdown conversion
- Pre-approval host checking
- Content size limiting
- Rate limiting per domain
- LRU cache (15-minute TTL)
- Prompt parameter for content filtering

---

### Task 2.7: WebSearchTool Enhancement

**Files:**
- Modify: `crates/alva-agent-tools/src/internet_search.rs`

**Claude Code's WebSearchTool features missing:**
- Domain filtering (allowed_domains, blocked_domains)
- Progress tracking events
- Search result synthesis

---

### Task 2.8: AskHumanTool Enhancement

**Files:**
- Modify: `crates/alva-agent-tools/src/ask_human.rs`

**Claude Code's AskUserQuestionTool features missing:**
- Multiple choice questions with options (2-4)
- Multi-select support
- Preview rendering
- Annotations collection
- Header text
- Question metadata

---

### Task 2.9: FindFilesTool Enhancement

**Files:**
- Modify: `crates/alva-agent-tools/src/find_files.rs`

**Claude Code's GlobTool features missing:**
- 100-file result limit
- Relative path translation
- Sort by modification time

---

## Phase 3: New Tools (alva-agent-tools)

### Task 3.1: Task Management Tools

**Files:**
- Create: `crates/alva-agent-tools/src/task_create.rs`
- Create: `crates/alva-agent-tools/src/task_update.rs`
- Create: `crates/alva-agent-tools/src/task_get.rs`
- Create: `crates/alva-agent-tools/src/task_list.rs`
- Create: `crates/alva-agent-tools/src/task_output.rs`
- Create: `crates/alva-agent-tools/src/task_stop.rs`

**Matching Claude Code's Task tools:**
- TaskCreate: Create tasks with subject, description, metadata
- TaskUpdate: Update status, owner, blocking relations, add blocks
- TaskGet: Retrieve task details by ID
- TaskList: List all tasks with filtering
- TaskOutput: Get task output/results from output file
- TaskStop: Cancel/kill running tasks

Each tool needs:
- Input schema (serde + JSON schema)
- Tool trait implementation
- Integration with TaskState in shared state

---

### Task 3.2: Team Management Tools

**Files:**
- Create: `crates/alva-agent-tools/src/team_create.rs`
- Create: `crates/alva-agent-tools/src/team_delete.rs`

**Matching Claude Code's Team tools:**
- TeamCreate: Create multi-agent teams with name, description, agent_type
- TeamDelete: Delete teams and associated task lists

---

### Task 3.3: Agent & Communication Tools

**Files:**
- Create: `crates/alva-agent-tools/src/agent_tool.rs`
- Create: `crates/alva-agent-tools/src/send_message.rs`

**Matching Claude Code:**
- AgentTool: Spawn sub-agents with model selection, CWD isolation, permission delegation
- SendMessageTool: Send messages between agents (mailbox routing, peer discovery)

---

### Task 3.4: Skill & Search Tools

**Files:**
- Create: `crates/alva-agent-tools/src/skill_tool.rs`
- Create: `crates/alva-agent-tools/src/tool_search.rs`

**Matching Claude Code:**
- SkillTool: Invoke skills/commands with forked agent context
- ToolSearchTool: Search and discover available tools by query

---

### Task 3.5: Notebook Edit Tool

**Files:**
- Create: `crates/alva-agent-tools/src/notebook_edit.rs`

**Matching Claude Code's NotebookEditTool:**
- Edit Jupyter notebook cells (code or markdown)
- Input: notebook_path, cell_id, new_source, cell_type, edit_mode (replace/insert/delete)
- JSON parsing of .ipynb format

---

### Task 3.6: Worktree & Plan Tools

**Files:**
- Create: `crates/alva-agent-tools/src/enter_worktree.rs`
- Create: `crates/alva-agent-tools/src/exit_worktree.rs`
- Create: `crates/alva-agent-tools/src/enter_plan_mode.rs`
- Create: `crates/alva-agent-tools/src/exit_plan_mode.rs`

**Matching Claude Code:**
- EnterWorktree: Create isolated git worktree, switch CWD
- ExitWorktree: Exit worktree, optionally keep/remove, discard changes
- EnterPlanMode: Switch to planning mode (restrict destructive tools)
- ExitPlanMode: Return to normal mode

---

### Task 3.7: Configuration & Display Tools

**Files:**
- Create: `crates/alva-agent-tools/src/config_tool.rs`
- Create: `crates/alva-agent-tools/src/brief_tool.rs`
- Create: `crates/alva-agent-tools/src/todo_write.rs`

**Matching Claude Code:**
- ConfigTool: Read/write settings.json configuration
- BriefTool: Display project briefing/overview
- TodoWriteTool: Write progress notes to CLAUDE.md

---

### Task 3.8: Scheduling & Remote Tools

**Files:**
- Create: `crates/alva-agent-tools/src/schedule_cron.rs`
- Create: `crates/alva-agent-tools/src/remote_trigger.rs`

**Matching Claude Code:**
- ScheduleCronTool: Create cron schedules (5-field format, 7-day expiration)
- RemoteTriggerTool: Manage remote scheduled agents (list/get/create/update/run)

---

### Task 3.9: MCP Tools

**Files:**
- Create: `crates/alva-agent-tools/src/mcp_tool.rs`
- Create: `crates/alva-agent-tools/src/list_mcp_resources.rs`
- Create: `crates/alva-agent-tools/src/read_mcp_resource.rs`
- Create: `crates/alva-agent-tools/src/mcp_auth.rs`

**Matching Claude Code:**
- MCPTool: Generic MCP tool executor with pass-through schema
- ListMcpResourcesTool: List resources from MCP servers (with caching)
- ReadMcpResourceTool: Read specific MCP resource by URI
- McpAuthTool: Manage MCP server authentication

---

### Task 3.10: Utility Tools

**Files:**
- Create: `crates/alva-agent-tools/src/sleep_tool.rs`
- Create: `crates/alva-agent-tools/src/synthetic_output.rs`

**Matching Claude Code:**
- SleepTool: Pause execution for specified duration
- SyntheticOutputTool: Generate synthetic tool output

---

### Task 3.11: LSP Tool

**Files:**
- Create: `crates/alva-agent-tools/src/lsp_tool.rs`

**Matching Claude Code's LSPTool (9 operations):**
- goToDefinition, findReferences, hover
- documentSymbol, workspaceSymbol
- goToImplementation
- prepareCallHierarchy, incomingCalls, outgoingCalls

---

### Task 3.12: Tool Registration Update

**Files:**
- Modify: `crates/alva-agent-tools/src/lib.rs`

**Update `register_all_tools()` to include all 44 tools with proper feature gating.**

---

## Phase 4: Context & Compaction (alva-agent-context)

### Task 4.1: Context Compaction Service

**Files:**
- Create: `crates/alva-agent-context/src/compact.rs`
- Create: `crates/alva-agent-context/src/auto_compact.rs`

**Matching Claude Code's compact/:**
- Message summarization/compaction
- Auto-compact trigger thresholds (token count based)
- Micro-compact for incremental compaction
- Preserve thinking blocks across compaction
- Token budget tracking

---

### Task 4.2: System & User Context Enhancement

**Files:**
- Modify: `crates/alva-agent-context/src/lib.rs`
- Create: `crates/alva-agent-context/src/system_context.rs`
- Create: `crates/alva-agent-context/src/user_context.rs`

**Matching Claude Code's context.ts:**
- `get_system_context()`: Git status, cache breakers (memoized)
- `get_user_context()`: CLAUDE.md contents, current date (memoized)
- `get_git_status()`: Branch, status, recent commits (truncated)
- System prompt injection support

---

## Phase 5: Session & Memory (alva-agent-memory)

### Task 5.1: Session Memory Service Enhancement

**Files:**
- Modify: `crates/alva-agent-memory/src/service.rs`

**Matching Claude Code's SessionMemory/:**
- Automatic periodic extraction of key information
- Forked sub-agent pattern for background extraction
- Configurable extraction thresholds
- Integration with compaction

---

### Task 5.2: Memory Extraction

**Files:**
- Create: `crates/alva-agent-memory/src/extract.rs`

**Matching Claude Code's extractMemories/:**
- Auto-memory directory writing
- Manifest creation (MEMORY.md index)
- Team memory support

---

## Phase 6: Permission System Enhancement (alva-agent-security)

### Task 6.1: Fine-Grained Permission Rules

**Files:**
- Modify: `crates/alva-agent-security/src/permission.rs`
- Create: `crates/alva-agent-security/src/rules.rs`
- Create: `crates/alva-agent-security/src/classifier.rs`

**Matching Claude Code's permission system:**
- Path-based permission scopes (glob patterns)
- Command whitelist/blacklist for BashTool
- Permission caching (allow-always, deny-always per tool+input pattern)
- Auto-approval classifier for safe bash commands
- Permission mode: interactive, auto, plan, bypass
- Audit logging of permission decisions
- Permission rule validation (Zod-like schema validation)

---

### Task 6.2: Sandbox Modes

**Files:**
- Modify: `crates/alva-agent-security/src/sandbox.rs`

**Matching Claude Code:**
- Sandbox detection and enforcement
- Read-only mode (plan mode)
- File write restrictions per sandbox config
- Network access restrictions

---

## Phase 7: Settings System (alva-app-core)

### Task 7.1: Settings Infrastructure

**Files:**
- Create: `crates/alva-app-core/src/settings/mod.rs`
- Create: `crates/alva-app-core/src/settings/types.rs`
- Create: `crates/alva-app-core/src/settings/loader.rs`
- Create: `crates/alva-app-core/src/settings/validation.rs`
- Create: `crates/alva-app-core/src/settings/cache.rs`

**Matching Claude Code's settings/ (5-level cascade):**
- Settings schema with serde + validation
- Source hierarchy: user → project → local → flag → policy
- Each level overrides previous
- Session-level caching
- File change detection
- Settings paths:
  - User: `~/.claude/settings.json`
  - Project: `.claude/settings.json`
  - Local: `.claude/settings.local.json`

**Key settings fields (matching Claude Code):**
```rust
pub struct Settings {
    pub permissions: PermissionSettings,
    pub sandbox: SandboxSettings,
    pub hooks: HooksSettings,
    pub env: HashMap<String, String>,
    pub model: Option<String>,
    pub theme: Option<String>,
    pub verbose: bool,
    pub expand_output: bool,
    // ... all fields from Claude Code
}
```

---

### Task 7.2: Hooks System

**Files:**
- Create: `crates/alva-app-core/src/hooks/mod.rs`
- Create: `crates/alva-app-core/src/hooks/types.rs`
- Create: `crates/alva-app-core/src/hooks/executor.rs`

**Matching Claude Code's hooks system:**
- Hook types: PreToolUse, PostToolUse, SessionStart, SessionEnd, Notification
- Hook configuration in settings.json
- Shell command execution with environment
- Hook result processing (approve, deny, modify)
- Matcher patterns for tool/event filtering

---

## Phase 8: Command System (alva-app-cli)

### Task 8.1: Command Infrastructure

**Files:**
- Create: `crates/alva-app-cli/src/commands/mod.rs`
- Create: `crates/alva-app-cli/src/commands/types.rs`
- Create: `crates/alva-app-cli/src/commands/registry.rs`
- Create: `crates/alva-app-cli/src/commands/executor.rs`

**Matching Claude Code's command system:**
```rust
pub enum CommandType {
    Prompt,    // Expands to LLM prompt
    Local,     // Executes synchronously, returns text
    LocalUI,   // Renders interactive UI
}

pub trait Command: Send + Sync {
    fn name(&self) -> &str;
    fn aliases(&self) -> &[&str];
    fn description(&self) -> &str;
    fn command_type(&self) -> CommandType;
    fn is_enabled(&self) -> bool;
    fn availability(&self) -> CommandAvailability;
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandResult;
}

pub struct CommandRegistry {
    commands: Vec<Box<dyn Command>>,
}
```

---

### Task 8.2: Built-in Commands (100+ commands)

**Files (one per command group):**
- Create: `crates/alva-app-cli/src/commands/session.rs` — /new, /resume, /sessions, /clear, /compact
- Create: `crates/alva-app-cli/src/commands/navigation.rs` — /help, /exit, /quit
- Create: `crates/alva-app-cli/src/commands/config.rs` — /config, /model, /theme, /permissions
- Create: `crates/alva-app-cli/src/commands/git.rs` — /commit, /branch, /pr
- Create: `crates/alva-app-cli/src/commands/tools.rs` — /tools, /mcp, /lsp
- Create: `crates/alva-app-cli/src/commands/agents.rs` — /agents, /team, /tasks
- Create: `crates/alva-app-cli/src/commands/info.rs` — /cost, /usage, /status, /doctor
- Create: `crates/alva-app-cli/src/commands/export.rs` — /export, /copy, /summary
- Create: `crates/alva-app-cli/src/commands/mode.rs` — /plan, /vim, /fast
- Create: `crates/alva-app-cli/src/commands/plugin.rs` — /install, /uninstall

**Key prompt commands (matching Claude Code exactly):**
- `/commit` — Git commit workflow with status + diff + log
- `/review` — Code review with PR context
- `/init` — Project initialization

---

### Task 8.3: History System

**Files:**
- Create: `crates/alva-app-cli/src/history.rs`

**Matching Claude Code's history.ts:**
- JSONL persistence to `~/.alva/history.jsonl`
- Session-scoped + project-scoped history
- Deduplication by display text
- Async flush with file locking
- Pasted content handling (inline small, external store for large)
- Ctrl+R picker (search through history)

---

## Phase 9: State Management (alva-app-core)

### Task 9.1: AppState

**Files:**
- Create: `crates/alva-app-core/src/state/mod.rs`
- Create: `crates/alva-app-core/src/state/app_state.rs`
- Create: `crates/alva-app-core/src/state/store.rs`
- Create: `crates/alva-app-core/src/state/selectors.rs`

**Matching Claude Code's AppState:**
```rust
pub struct AppState {
    pub messages: Vec<Message>,
    pub settings: Settings,
    pub tools: Vec<Arc<dyn Tool>>,
    pub mcp_tools: Vec<Arc<dyn Tool>>,
    pub tasks: HashMap<String, TaskState>,
    pub agent_registry: HashMap<String, String>,
    pub permission_context: PermissionContext,
    pub team_context: Option<TeamContext>,
    pub is_loading: bool,
    pub current_model: String,
    pub plugins: PluginState,
    pub mcp_clients: Vec<McpClientState>,
    // ... full state
}

pub struct AppStateStore {
    state: Arc<RwLock<AppState>>,
    subscribers: Vec<Box<dyn Fn(&AppState)>>,
}
```

---

## Phase 10: Provider Enhancement (alva-provider)

### Task 10.1: Rate Limiting

**Files:**
- Create: `crates/alva-provider/src/rate_limit.rs`

**Matching Claude Code's rate limiting:**
- 5-hour and 7-day rate limit tracking
- Early warning detection
- Quota status from API response headers
- Overage handling

---

### Task 10.2: Multi-Provider Support

**Files:**
- Modify: `crates/alva-provider/src/openai.rs`
- Create: `crates/alva-provider/src/anthropic.rs`

**Matching Claude Code's API service:**
- Direct Anthropic API support (not just OpenAI-compatible)
- Streaming and non-streaming queries
- Beta feature management (thinking, structured outputs)
- Multiple providers: Direct, Bedrock, Vertex
- Token counting per-request

---

## Phase 11: MCP Enhancement (alva-protocol-mcp)

### Task 11.1: Transport Enhancements

**Files:**
- Modify: `crates/alva-protocol-mcp/src/transport.rs`

**Matching Claude Code's MCP client:**
- SSE transport
- Streamable HTTP transport
- WebSocket transport
- In-process transport
- Connection lifecycle management
- Reconnection logic

---

### Task 11.2: Resource & Prompt Support

**Files:**
- Create: `crates/alva-protocol-mcp/src/resources.rs`
- Create: `crates/alva-protocol-mcp/src/prompts.rs`
- Create: `crates/alva-protocol-mcp/src/elicitation.rs`

**Matching Claude Code:**
- Resource listing and reading
- Prompt template listing and invocation
- Elicitation handler for MCP requests
- Channel allowlist management

---

## Phase 12: Plugin System (alva-app-core)

### Task 12.1: Plugin Infrastructure

**Files:**
- Create: `crates/alva-app-core/src/plugins/mod.rs`
- Create: `crates/alva-app-core/src/plugins/types.rs`
- Create: `crates/alva-app-core/src/plugins/manager.rs`
- Create: `crates/alva-app-core/src/plugins/installation.rs`

**Matching Claude Code's plugin system:**
- Plugin lifecycle (install, uninstall, enable, disable, update)
- Plugin version management
- Plugin scope (user, project, local, managed)
- Plugin commands registration
- Plugin state persistence
- Marketplace reconciliation

---

## Phase 13: Analytics (alva-app-core)

### Task 13.1: Analytics Service

**Files:**
- Create: `crates/alva-app-core/src/analytics/mod.rs`
- Create: `crates/alva-app-core/src/analytics/events.rs`
- Create: `crates/alva-app-core/src/analytics/sink.rs`

**Matching Claude Code's analytics/:**
- Event logging with queue pattern
- Sink multiplexing (multiple destinations)
- Fail-open if no sink attached
- Metadata extraction (no code/filepaths)
- Feature gates (GrowthBook integration)

---

## Phase 14: Multi-Agent / Swarm (alva-app-core)

### Task 14.1: Swarm Infrastructure

**Files:**
- Create: `crates/alva-app-core/src/swarm/mod.rs`
- Create: `crates/alva-app-core/src/swarm/spawn.rs`
- Create: `crates/alva-app-core/src/swarm/backends.rs`
- Create: `crates/alva-app-core/src/swarm/permission_sync.rs`
- Create: `crates/alva-app-core/src/swarm/reconnection.rs`

**Matching Claude Code's swarm/:**
- Teammate spawning with CLI flag inheritance
- Multiple backends: In-Process, Tmux, iTerm2, Pane
- Permission synchronization (leader ↔ teammates)
- Connection recovery
- Team file management
- Agent-to-agent message routing (mailbox pattern)

---

### Task 14.2: Agent Summary & Coordination

**Files:**
- Create: `crates/alva-app-core/src/swarm/agent_summary.rs`
- Create: `crates/alva-app-core/src/swarm/coordinator.rs`

**Matching Claude Code:**
- Periodic 30-second background summarization for sub-agents
- Coordinator mode for swarm leader
- Progress display

---

## Phase 15: Terminal UI (alva-app-cli)

### Task 15.1: Rich Terminal Output with ratatui

**Files:**
- Create: `crates/alva-app-cli/src/ui/mod.rs`
- Create: `crates/alva-app-cli/src/ui/message_list.rs`
- Create: `crates/alva-app-cli/src/ui/permission_dialog.rs`
- Create: `crates/alva-app-cli/src/ui/prompt_input.rs`
- Create: `crates/alva-app-cli/src/ui/tool_use_display.rs`
- Create: `crates/alva-app-cli/src/ui/spinner.rs`
- Create: `crates/alva-app-cli/src/ui/markdown.rs`

**Matching Claude Code's terminal UI:**
- Virtual message list (efficient scrolling for 1000+ messages)
- Rich markdown rendering with syntax highlighting
- Permission request dialogs per tool type
- Tool execution status (animated indicators)
- Prompt input with typeahead, history, vim mode
- Cost/token display
- Spinner with contextual tips
- Table formatting
- Code block rendering with language detection
- ANSI color management

---

### Task 15.2: Input System

**Files:**
- Create: `crates/alva-app-cli/src/ui/input/mod.rs`
- Create: `crates/alva-app-cli/src/ui/input/typeahead.rs`
- Create: `crates/alva-app-cli/src/ui/input/history_search.rs`
- Create: `crates/alva-app-cli/src/ui/input/vim_mode.rs`

**Matching Claude Code:**
- Mode-based input (`/` commands, `!` shell, `@` mentions)
- Typeahead autocomplete for commands and file paths
- Arrow key history navigation
- Ctrl+R history search
- Vim mode (optional)
- Paste detection and handling

---

### Task 15.3: Permission Dialogs

**Files:**
- Create: `crates/alva-app-cli/src/ui/permissions/mod.rs`
- Create: `crates/alva-app-cli/src/ui/permissions/bash.rs`
- Create: `crates/alva-app-cli/src/ui/permissions/file_edit.rs`
- Create: `crates/alva-app-cli/src/ui/permissions/file_write.rs`
- Create: `crates/alva-app-cli/src/ui/permissions/web.rs`
- Create: `crates/alva-app-cli/src/ui/permissions/filesystem.rs`

**Matching Claude Code's tool-specific permission UIs:**
- BashPermissionRequest: Destructive command warnings, colored display
- FileEditPermissionRequest: Diff view of proposed changes
- FilesystemPermissionRequest: Path display
- WebFetchPermissionRequest: Domain display
- Each with: allow/deny/always-allow/always-deny options

---

## Phase 16: Notification & System Services (alva-app-cli)

### Task 16.1: Notification Service

**Files:**
- Create: `crates/alva-app-cli/src/services/notifier.rs`

**Matching Claude Code's notifier.ts:**
- Terminal type detection (iTerm2, Kitty, Ghostty, Apple Terminal)
- Notification channel selection
- Bell notification fallback

---

### Task 16.2: Prevent Sleep

**Files:**
- Create: `crates/alva-app-cli/src/services/prevent_sleep.rs`

**Matching Claude Code's preventSleep.ts:**
- macOS: `caffeinate` command with 5-minute timeout
- Reference-counted start/stop
- Auto-restart before timeout

---

### Task 16.3: Voice Input

**Files:**
- Create: `crates/alva-app-cli/src/services/voice.rs`

**Matching Claude Code's voice.ts:**
- Native audio capture (cpal crate for Rust)
- 16kHz mono recording
- Silence detection (2-second)
- Push-to-talk interface

---

### Task 16.4: Tips System

**Files:**
- Create: `crates/alva-app-cli/src/services/tips.rs`

**Matching Claude Code's tips/:**
- Tip registry with contextual tips
- Tip history tracking
- Cooldown management
- Longest-since-shown selection

---

## Phase 17: OAuth & Remote (alva-app-core)

### Task 17.1: OAuth Service

**Files:**
- Create: `crates/alva-app-core/src/auth/mod.rs`
- Create: `crates/alva-app-core/src/auth/oauth.rs`
- Create: `crates/alva-app-core/src/auth/token.rs`

**Matching Claude Code's oauth/:**
- OAuth 2.0 authorization code flow with PKCE
- Automatic browser flow (localhost listener)
- Manual auth code flow
- Token exchange and profile fetching
- Subscription type detection

---

### Task 17.2: Policy Limits

**Files:**
- Create: `crates/alva-app-core/src/policy/mod.rs`
- Create: `crates/alva-app-core/src/policy/limits.rs`

**Matching Claude Code's policyLimits/:**
- Organization-level policy restrictions
- ETag-based HTTP caching
- Background polling (1-hour interval)
- Fail-open graceful degradation

---

## Phase 18: LSP Integration (alva-app-core)

### Task 18.1: LSP Server Manager

**Files:**
- Create: `crates/alva-app-core/src/lsp/mod.rs`
- Create: `crates/alva-app-core/src/lsp/manager.rs`
- Create: `crates/alva-app-core/src/lsp/client.rs`
- Create: `crates/alva-app-core/src/lsp/diagnostics.rs`

**Matching Claude Code's LSP service:**
- Multi-server management
- Diagnostic collection and aggregation
- Server initialization and lifecycle
- IDE passive feedback

---

## Phase 19: Query Engine Enhancement (alva-agent-core)

### Task 19.1: Query Loop Enhancements

**Files:**
- Modify: `crates/alva-agent-core/src/run.rs`

**Matching Claude Code's query.ts:**
- Token budget tracking per turn
- Max output tokens recovery (retry with adjusted budget)
- Thinking block preservation across turns
- Tool result budget tracking
- Reactive compaction (compact when approaching limit)
- Auto-compact integration

---

### Task 19.2: Streaming Tool Executor Enhancement

**Files:**
- Modify: `crates/alva-agent-core/src/run.rs`

**Matching Claude Code's StreamingToolExecutor:**
- Concurrent tool execution (for concurrency-safe tools)
- Tool progress streaming events
- Error recovery per-tool
- Tool result size management (disk persistence for large results)

---

## Phase 20: Integration & Wiring

### Task 20.1: CLI Integration

**Files:**
- Modify: `crates/alva-app-cli/src/main.rs`
- Modify: `crates/alva-app-cli/src/repl.rs`
- Modify: `crates/alva-app-cli/src/agent_setup.rs`
- Modify: `crates/alva-app-cli/src/event_handler.rs`

**Wire everything together:**
- Command system registration in REPL
- AppState initialization
- Settings loading at startup
- Hook execution at session start/end
- Analytics initialization
- Plugin loading
- MCP client initialization
- History system integration
- Notification system integration
- Prevent sleep integration

---

### Task 20.2: Cargo.toml Updates

**Files:**
- Modify: All `Cargo.toml` files

**Add new dependencies and feature flags for all new modules.**

---

## Verification Checklist

For each phase, verify:

### Tool Verification
- [ ] All 44 tools registered and callable
- [ ] Each tool's input schema matches Claude Code exactly
- [ ] Each tool's output format matches Claude Code exactly
- [ ] Permission checking works for each tool
- [ ] Tool descriptions match Claude Code

### Command Verification
- [ ] All slash commands available and functional
- [ ] Prompt commands expand correctly
- [ ] Local commands return correct results
- [ ] Command availability filtering works
- [ ] Skill loading from all sources works

### Service Verification
- [ ] Session memory extraction works
- [ ] Compaction triggers at correct thresholds
- [ ] Token estimation matches Claude Code
- [ ] OAuth flow completes successfully
- [ ] Rate limiting tracks correctly
- [ ] Plugin install/uninstall lifecycle works
- [ ] Analytics events log correctly
- [ ] MCP connections with all transport types work
- [ ] LSP server management works
- [ ] Notification delivery works across terminal types

### State Verification
- [ ] AppState contains all fields
- [ ] Settings cascade loads correctly (5 levels)
- [ ] Permission decisions cache properly
- [ ] Task state transitions work correctly
- [ ] Team/swarm context propagates

### UI Verification
- [ ] Message list renders all message types
- [ ] Permission dialogs show for each tool
- [ ] Markdown renders with syntax highlighting
- [ ] Tool execution shows animated status
- [ ] Spinner shows contextual tips
- [ ] Cost/token display updates in real-time
- [ ] History search (Ctrl+R) works
- [ ] Typeahead autocomplete works
