# alva-eval Phase 1: Full-Chain Observable Agent Testing Workbench

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Transform alva-eval from a basic playground into a full-chain observable testing workbench where every middleware hook, every LLM request/response, every tool call with timing, and skill injection details are visible — plus support for concurrent A/B runs with diff comparison.

**Architecture:** Add a `RecorderMiddleware` that captures every detail (messages sent to LLM, responses, tool I/O, timing, middleware hooks) into a `RunRecord` data model. Enhance the SSE stream with these detailed events. Add skill discovery from `.alva/` and `.claude/` config paths. Add a Compare view for side-by-side concurrent runs. Store completed runs in-memory for post-run inspection.

**Tech Stack:** Rust (axum 0.7, alva-agent-core middleware system, alva-protocol-skill), vanilla JS frontend with rust-embed.

---

## Current State

- `crates/alva-eval/` exists with: axum server, rust-embed static serving, basic `POST /api/run` → SSE event stream
- `AgentEvent` already derives `Serialize` with `#[serde(tag = "type")]`
- Middleware trait has hooks: `on_agent_start`, `before_llm_call`, `after_llm_call`, `wrap_llm_call`, `before_tool_call`, `after_tool_call`, `wrap_tool_call`, `on_agent_end`
- `MiddlewarePriority::OBSERVATION = 5000` is the standard tier for recording
- Skill system: `FsSkillRepository` scans `bundled/`, `mbb/`, `user/` dirs with `SKILL.md` + `state.json`
- `SkillStore` provides `list()`, `find_enabled()`, `search()` — in-memory cache after `scan()`
- `SkillLoader` builds system prompt injections: metadata summary table (Auto) or full body (Explicit/Strict)
- Settings cascade: `~/.claude/settings.json` → `<workspace>/.claude/settings.json` → `.claude/settings.local.json`
- `AlvaPaths` resolves global (`~/.config/alva/`) and project (`.alva/`) paths

## Key Files Reference

| Component | Path |
|-----------|------|
| Middleware trait | `crates/alva-agent-core/src/middleware.rs:45-141` |
| MiddlewareStack hooks | `crates/alva-agent-core/src/middleware.rs:193-335` |
| run_agent entry | `crates/alva-agent-core/src/run.rs:199-246` |
| Inner loop (turns) | `crates/alva-agent-core/src/run.rs:266-503` |
| LoopDetectionMiddleware (example) | `crates/alva-agent-core/src/builtins/loop_detection.rs` |
| MiddlewarePriority | `crates/alva-agent-core/src/shared.rs:102-112` |
| AgentEvent | `crates/alva-agent-core/src/event.rs` |
| FsSkillRepository | `crates/alva-protocol-skill/src/fs.rs` |
| SkillStore | `crates/alva-protocol-skill/src/store.rs` |
| SkillLoader / SkillInjector | `crates/alva-protocol-skill/src/loader.rs`, `injector.rs` |
| Skill types (SkillMeta, SkillBody, InjectionPolicy) | `crates/alva-protocol-skill/src/types.rs` |
| InMemorySkillRepository | `crates/alva-protocol-skill/src/memory.rs` |
| AlvaPaths | `crates/alva-app-core/src/paths.rs` |
| Settings loading | `crates/alva-app-core/src/settings/` |
| Current alva-eval main.rs | `crates/alva-eval/src/main.rs` |
| Current static files | `crates/alva-eval/static/{index.html,style.css,app.js}` |

---

### Task 1: RecorderMiddleware — Data Model

The core data structure that captures every detail of a run.

**Files:**
- Create: `crates/alva-eval/src/recorder.rs`

**Step 1: Define the RunRecord and sub-types**

```rust
// crates/alva-eval/src/recorder.rs

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Serialize;

use alva_agent_core::middleware::{
    LlmCallFn, Middleware, MiddlewareError, MiddlewarePriority, ToolCallFn,
};
use alva_agent_core::state::AgentState;
use alva_types::{Message, ToolCall, ToolOutput};

// ---------------------------------------------------------------------------
// Data model — serialized to JSON for the frontend
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct RunRecord {
    pub config_snapshot: ConfigSnapshot,
    pub turns: Vec<TurnRecord>,
    pub total_duration_ms: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigSnapshot {
    pub system_prompt: String,
    pub model_id: String,
    pub tool_names: Vec<String>,
    pub skill_names: Vec<String>,
    pub max_iterations: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnRecord {
    pub turn_number: u32,
    pub llm_call: LlmCallRecord,
    pub tool_calls: Vec<ToolCallRecord>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmCallRecord {
    /// Messages sent to the LLM (full content, expandable in UI)
    pub messages_sent: Vec<Message>,
    pub messages_sent_count: usize,
    /// LLM response (full content)
    pub response: Option<Message>,
    /// Token usage for this call
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Timing
    pub duration_ms: u64,
    /// Stop reason: "end_turn" | "tool_use" | "max_tokens" | "error"
    pub stop_reason: String,
    /// Middleware hooks that fired
    pub middleware_hooks: Vec<HookRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolCallRecord {
    pub tool_call: ToolCall,
    pub result: Option<ToolOutput>,
    pub is_error: bool,
    pub duration_ms: u64,
    pub middleware_hooks: Vec<HookRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HookRecord {
    pub middleware_name: String,
    pub hook: String, // "before_llm_call", "after_tool_call", etc.
    pub duration_ms: u64,
    pub outcome: String, // "ok", "error: ...", "blocked"
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p alva-eval`
Expected: PASS (it's just data types, no logic yet)

---

### Task 2: RecorderMiddleware — Middleware Implementation

Implement the `Middleware` trait to capture all details during a run.

**Files:**
- Modify: `crates/alva-eval/src/recorder.rs` (append to the file from Task 1)

**Step 1: Implement the RecorderMiddleware struct and Middleware trait**

```rust
// Append to crates/alva-eval/src/recorder.rs

// ---------------------------------------------------------------------------
// RecorderMiddleware — captures every detail for the eval UI
// ---------------------------------------------------------------------------

/// Internal mutable state accumulated during a run.
struct RecorderState {
    config_snapshot: Option<ConfigSnapshot>,
    turns: Vec<TurnRecord>,
    current_turn: Option<TurnBuild>,
    run_start: Instant,
}

struct TurnBuild {
    turn_number: u32,
    turn_start: Instant,
    llm_messages_sent: Vec<Message>,
    llm_start: Option<Instant>,
    llm_response: Option<Message>,
    llm_input_tokens: u32,
    llm_output_tokens: u32,
    llm_hooks: Vec<HookRecord>,
    tool_calls: Vec<ToolCallRecord>,
}

/// Middleware that records every detail of an agent run.
///
/// After the run completes, call `take_record()` to extract the full `RunRecord`.
pub struct RecorderMiddleware {
    state: Arc<Mutex<RecorderState>>,
}

impl RecorderMiddleware {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RecorderState {
                config_snapshot: None,
                turns: Vec::new(),
                current_turn: None,
                run_start: Instant::now(),
            })),
        }
    }

    /// Extract the completed run record. Call after the agent run finishes.
    pub fn take_record(&self) -> RunRecord {
        let mut s = self.state.lock().unwrap();
        let total_duration = s.run_start.elapsed();

        let mut total_in: u64 = 0;
        let mut total_out: u64 = 0;
        for t in &s.turns {
            total_in += t.llm_call.input_tokens as u64;
            total_out += t.llm_call.output_tokens as u64;
        }

        RunRecord {
            config_snapshot: s.config_snapshot.clone().unwrap_or_else(|| ConfigSnapshot {
                system_prompt: String::new(),
                model_id: String::new(),
                tool_names: vec![],
                skill_names: vec![],
                max_iterations: 0,
            }),
            turns: std::mem::take(&mut s.turns),
            total_duration_ms: total_duration.as_millis() as u64,
            total_input_tokens: total_in,
            total_output_tokens: total_out,
        }
    }
}

#[async_trait]
impl Middleware for RecorderMiddleware {
    fn name(&self) -> &str {
        "eval_recorder"
    }

    fn priority(&self) -> i32 {
        // Observation tier — runs after all other middleware
        MiddlewarePriority::OBSERVATION + 100
    }

    async fn on_agent_start(&self, state: &mut AgentState) -> Result<(), MiddlewareError> {
        let mut s = self.state.lock().unwrap();
        s.run_start = Instant::now();
        s.config_snapshot = Some(ConfigSnapshot {
            system_prompt: String::new(), // Filled in before_llm_call
            model_id: state.model.model_id().to_string(),
            tool_names: state.tools.iter().map(|t| t.name().to_string()).collect(),
            skill_names: vec![], // Filled by caller
            max_iterations: 0,   // Filled by caller
        });
        Ok(())
    }

    async fn before_llm_call(
        &self,
        _state: &mut AgentState,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        let mut s = self.state.lock().unwrap();
        let turn_number = s.turns.len() as u32 + 1;
        s.current_turn = Some(TurnBuild {
            turn_number,
            turn_start: Instant::now(),
            llm_messages_sent: messages.clone(),
            llm_start: Some(Instant::now()),
            llm_response: None,
            llm_input_tokens: 0,
            llm_output_tokens: 0,
            llm_hooks: vec![],
            tool_calls: vec![],
        });
        Ok(())
    }

    async fn after_llm_call(
        &self,
        _state: &mut AgentState,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        let mut s = self.state.lock().unwrap();
        if let Some(ref mut turn) = s.current_turn {
            let llm_duration = turn
                .llm_start
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);

            turn.llm_response = Some(response.clone());

            if let Some(ref usage) = response.usage {
                turn.llm_input_tokens = usage.input_tokens;
                turn.llm_output_tokens = usage.output_tokens;
            }

            // Determine stop reason
            let has_tool_calls = response.content.iter().any(|b| {
                matches!(b, alva_types::ContentBlock::ToolUse { .. })
            });
            let stop_reason = if has_tool_calls {
                "tool_use"
            } else {
                "end_turn"
            };

            turn.llm_hooks.push(HookRecord {
                middleware_name: "llm_call".into(),
                hook: "complete".into(),
                duration_ms: llm_duration,
                outcome: stop_reason.into(),
            });
        }
        Ok(())
    }

    async fn before_tool_call(
        &self,
        _state: &mut AgentState,
        _tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        // Timing is handled in after_tool_call
        Ok(())
    }

    async fn after_tool_call(
        &self,
        _state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        let mut s = self.state.lock().unwrap();
        if let Some(ref mut turn) = s.current_turn {
            turn.tool_calls.push(ToolCallRecord {
                tool_call: tool_call.clone(),
                result: Some(result.clone()),
                is_error: result.is_error,
                duration_ms: 0, // We don't have per-tool timing without wrap_tool_call
                middleware_hooks: vec![],
            });
        }
        Ok(())
    }

    async fn on_agent_end(
        &self,
        _state: &mut AgentState,
        _error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        let mut s = self.state.lock().unwrap();
        // Finalize current turn if exists
        if let Some(turn) = s.current_turn.take() {
            let llm_duration = turn
                .llm_start
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);

            let stop_reason = if turn.llm_response.is_some() {
                if turn.tool_calls.is_empty() {
                    "end_turn"
                } else {
                    "tool_use"
                }
            } else {
                "error"
            };

            s.turns.push(TurnRecord {
                turn_number: turn.turn_number,
                llm_call: LlmCallRecord {
                    messages_sent: turn.llm_messages_sent,
                    messages_sent_count: 0, // filled below
                    response: turn.llm_response,
                    input_tokens: turn.llm_input_tokens,
                    output_tokens: turn.llm_output_tokens,
                    duration_ms: llm_duration,
                    stop_reason: stop_reason.into(),
                    middleware_hooks: turn.llm_hooks,
                },
                tool_calls: turn.tool_calls,
                duration_ms: turn.turn_start.elapsed().as_millis() as u64,
            });

            // Fix messages_sent_count
            if let Some(last) = s.turns.last_mut() {
                last.llm_call.messages_sent_count = last.llm_call.messages_sent.len();
            }
        }
        Ok(())
    }
}
```

**Step 2: Add `mod recorder;` to main.rs**

In `crates/alva-eval/src/main.rs`, add:
```rust
mod recorder;
```

And add `async-trait` to Cargo.toml dependencies:
```toml
async-trait = "0.1"
```

**Step 3: Verify it compiles**

Run: `cargo check -p alva-eval`
Expected: PASS

---

### Task 3: Wire RecorderMiddleware into Run Pipeline + Store Records

Integrate the recorder into the existing `start_run` handler and store completed records for the Inspector view.

**Files:**
- Modify: `crates/alva-eval/src/main.rs`

**Step 1: Expand AppState and RunRequest**

Add to `AppState`:
```rust
/// Completed run records for inspection.
records: Mutex<HashMap<String, recorder::RunRecord>>,
```

Add to `RunRequest`:
```rust
/// Skill directories to scan (optional).
#[serde(default)]
skill_dirs: Option<Vec<String>>,
/// Skills to enable by name (optional).
#[serde(default)]
skills: Option<Vec<String>>,
```

Add to `RunResponse`:
```rust
skills: Vec<String>,
```

**Step 2: Integrate RecorderMiddleware into start_run**

In the `start_run` function, after creating the MiddlewareStack:

```rust
let recorder = Arc::new(recorder::RecorderMiddleware::new());
middleware.push_sorted(recorder.clone());
```

After the spawned task completes, extract the record. This requires restructuring: instead of fire-and-forget `tokio::spawn`, we use a two-phase approach:
1. Spawn the agent run
2. Spawn a second task that waits for the agent to finish, then extracts the record

```rust
let recorder_clone = recorder.clone();
let records = state.records.clone();
let run_id_clone = run_id.clone();

tokio::spawn(async move {
    let _workspace = workspace;
    let mut st = agent_state;
    let _ = run_agent(&mut st, &agent_config, cancel, messages, tx).await;

    // Agent finished — extract the record
    let record = recorder_clone.take_record();
    records.lock().await.insert(run_id_clone, record);
});
```

**Step 3: Add GET /api/records/:run_id endpoint**

```rust
async fn get_record(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let records = state.records.lock().await;
    match records.get(&run_id) {
        Some(record) => Json(record).into_response(),
        None => (StatusCode::NOT_FOUND, "record not found or still running").into_response(),
    }
}
```

Add to router:
```rust
.route("/api/records/:run_id", get(get_record))
```

**Step 4: Add GET /api/runs endpoint (list all completed runs)**

```rust
#[derive(Serialize)]
struct RunSummary {
    run_id: String,
    model_id: String,
    turns: usize,
    total_tokens: u64,
    duration_ms: u64,
}

async fn list_runs(State(state): State<Arc<AppState>>) -> Json<Vec<RunSummary>> {
    let records = state.records.lock().await;
    let summaries = records
        .iter()
        .map(|(id, r)| RunSummary {
            run_id: id.clone(),
            model_id: r.config_snapshot.model_id.clone(),
            turns: r.turns.len(),
            total_tokens: r.total_input_tokens + r.total_output_tokens,
            duration_ms: r.total_duration_ms,
        })
        .collect();
    Json(summaries)
}
```

Add to router: `.route("/api/runs", get(list_runs))`

**Step 5: Verify it compiles**

Run: `cargo check -p alva-eval`
Expected: PASS

**Step 6: Commit**

```bash
git add crates/alva-eval/
git commit -m "feat(eval): add RecorderMiddleware with full turn/tool/LLM recording"
```

---

### Task 4: Skill Discovery API

Scan `.alva/skills/`, `~/.config/alva/skills/`, and user-specified directories for available skills.

**Files:**
- Create: `crates/alva-eval/src/skills.rs`
- Modify: `crates/alva-eval/src/main.rs` (add routes)
- Modify: `crates/alva-eval/Cargo.toml` (add alva-protocol-skill dep)

**Step 1: Add dependency**

In `Cargo.toml`:
```toml
alva-protocol-skill = { path = "../alva-protocol-skill" }
dirs = "5"
```

**Step 2: Create skills.rs**

```rust
// crates/alva-eval/src/skills.rs

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use alva_protocol_skill::fs::FsSkillRepository;
use alva_protocol_skill::repository::SkillRepository;
use alva_protocol_skill::store::SkillStore;

#[derive(Serialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub kind: String,   // "bundled" | "user" | "mbb"
    pub enabled: bool,
    pub source_dir: String,
}

#[derive(Serialize)]
pub struct SkillSourceInfo {
    pub path: String,
    pub exists: bool,
    pub label: String, // "project .alva/", "global ~/.config/alva/", "custom"
}

/// Discover all skill source directories.
pub fn discover_skill_sources(workspace: Option<&Path>) -> Vec<SkillSourceInfo> {
    let mut sources = Vec::new();

    // 1. Project skills: <workspace>/.alva/skills/
    if let Some(ws) = workspace {
        let path = ws.join(".alva").join("skills");
        sources.push(SkillSourceInfo {
            exists: path.exists(),
            path: path.to_string_lossy().to_string(),
            label: "Project .alva/skills".to_string(),
        });
    }

    // 2. Global skills: ~/.config/alva/skills/
    if let Some(config_dir) = dirs::config_dir() {
        let path = config_dir.join("alva").join("skills");
        sources.push(SkillSourceInfo {
            exists: path.exists(),
            path: path.to_string_lossy().to_string(),
            label: "Global ~/.config/alva/skills".to_string(),
        });
    }

    // 3. Claude Code skills: ~/.claude/skills/ (if exists)
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".claude").join("skills");
        if path.exists() {
            sources.push(SkillSourceInfo {
                exists: true,
                path: path.to_string_lossy().to_string(),
                label: "Claude ~/.claude/skills".to_string(),
            });
        }
    }

    sources
}

/// Scan a skill directory and return all skills found.
pub async fn scan_skills(skill_dir: &Path) -> Vec<SkillInfo> {
    let bundled = skill_dir.join("bundled");
    let mbb = skill_dir.join("mbb");
    let user = skill_dir.join("user");
    let state_file = skill_dir.join("state.json");

    let repo = Arc::new(FsSkillRepository::new(bundled, mbb, user, state_file));
    let store = SkillStore::new(repo as Arc<dyn SkillRepository>);

    if store.scan().await.is_err() {
        return vec![];
    }

    match store.list().await {
        Ok(skills) => skills
            .into_iter()
            .map(|s| SkillInfo {
                name: s.meta.name.clone(),
                description: s.meta.description.clone(),
                kind: format!("{:?}", s.kind),
                enabled: s.enabled,
                source_dir: skill_dir.to_string_lossy().to_string(),
            })
            .collect(),
        Err(_) => vec![],
    }
}
```

**Step 3: Add routes to main.rs**

```rust
mod skills;

// GET /api/skills/sources — discover skill directories
async fn list_skill_sources() -> Json<Vec<skills::SkillSourceInfo>> {
    Json(skills::discover_skill_sources(None))
}

// POST /api/skills/scan — scan a specific skill directory
#[derive(Deserialize)]
struct ScanRequest { path: String }

async fn scan_skills(Json(req): Json<ScanRequest>) -> Json<Vec<skills::SkillInfo>> {
    Json(skills::scan_skills(std::path::Path::new(&req.path)).await)
}
```

Add routes:
```rust
.route("/api/skills/sources", get(list_skill_sources))
.route("/api/skills/scan", post(scan_skills_handler))
```

**Step 4: Verify it compiles**

Run: `cargo check -p alva-eval`

**Step 5: Commit**

```bash
git add crates/alva-eval/
git commit -m "feat(eval): add skill discovery — scan .alva/ .claude/ and custom skill dirs"
```

---

### Task 5: Compare API — Concurrent Dual Runs

Support starting two runs simultaneously and retrieving both records for comparison.

**Files:**
- Modify: `crates/alva-eval/src/main.rs` (add compare endpoint)

**Step 1: Add POST /api/compare endpoint**

```rust
#[derive(Deserialize)]
struct CompareRequest {
    run_a: RunRequest,
    run_b: RunRequest,
}

#[derive(Serialize)]
struct CompareResponse {
    run_id_a: String,
    run_id_b: String,
}
```

Implementation: Call `start_run` logic twice (extract into a helper), return both run_ids. The frontend opens two SSE connections in parallel.

**Step 2: Add GET /api/compare/:id_a/:id_b endpoint**

Returns both RunRecords side-by-side after both complete:

```rust
#[derive(Serialize)]
struct CompareResult {
    run_a: Option<recorder::RunRecord>,
    run_b: Option<recorder::RunRecord>,
    diff: CompareDiff,
}

#[derive(Serialize)]
struct CompareDiff {
    turns_a: usize,
    turns_b: usize,
    tokens_a: u64,
    tokens_b: u64,
    duration_a_ms: u64,
    duration_b_ms: u64,
    tool_calls_a: Vec<String>,
    tool_calls_b: Vec<String>,
    /// Tool calls that appear in A but not B (or vice versa)
    tools_only_a: Vec<String>,
    tools_only_b: Vec<String>,
}
```

**Step 3: Verify and commit**

---

### Task 6: Frontend — Inspector View (inspector.html + inspector.js)

The Inspector shows the full detail of a completed run: expandable turns, full LLM request/response, tool I/O.

**Files:**
- Create: `crates/alva-eval/static/inspector.html`
- Create: `crates/alva-eval/static/inspector.js`
- Modify: `crates/alva-eval/static/shared.js` (extract common utilities from app.js)

**Step 1: Create shared.js**

Extract from `app.js`: `escHtml()`, `truncate()`, `formatJson()`, `formatToolOutput()`, `setStatus()`, `scrollBottom()`.

Update `app.js` to remove these and add `<script src="/shared.js"></script>` in `index.html`.

**Step 2: Create inspector.html**

Layout:
```
┌─────────────────────────────────────────────────────────────────┐
│  alva-eval    [Playground]  [Inspector]  [Compare]              │
├──────────────────────┬──────────────────────────────────────────┤
│  Run Selector        │  Turn Timeline                           │
│                      │                                          │
│  ▸ Run abc123        │  ┌─ Turn 1 ─────────────────────────┐   │
│    claude-sonnet     │  │                                    │   │
│    2 turns, 1.5k tok │  │  ▸ LLM Request (1,234 tokens)     │   │
│    4.8s              │  │    [click to expand messages]      │   │
│                      │  │                                    │   │
│  ▸ Run def456        │  │  ▸ LLM Response (156 tokens)      │   │
│    gpt-4o            │  │    "Let me read that file..."      │   │
│    3 turns, 2.1k tok │  │    stop_reason: tool_use           │   │
│    6.2s              │  │    duration: 2.3s                  │   │
│                      │  │                                    │   │
│                      │  │  ▸ Tool: read_file (12ms)          │   │
│                      │  │    input: {"path": "/src/main.rs"} │   │
│                      │  │    output: "fn main() { ... }"     │   │
│                      │  └────────────────────────────────────┘   │
│                      │                                          │
│                      │  ┌─ Turn 2 ─────────────────────────┐   │
│                      │  │  ▸ LLM Request (1,546 tokens)     │   │
│                      │  │  ▸ LLM Response (312 tokens)      │   │
│                      │  │    "This file contains..."         │   │
│                      │  │    stop_reason: end_turn           │   │
│                      │  └────────────────────────────────────┘   │
│                      │                                          │
│                      │  ┌─ Summary ─────────────────────────┐   │
│                      │  │  2 turns, 1 tool call              │   │
│                      │  │  1,234 + 312 = 1,546 tokens        │   │
│                      │  │  4.8s total                        │   │
│                      │  └────────────────────────────────────┘   │
└──────────────────────┴──────────────────────────────────────────┘
```

**Step 3: Create inspector.js**

Core logic:
- On page load: `GET /api/runs` → populate left sidebar
- On run click: `GET /api/records/:run_id` → render turn timeline
- Expandable sections: click to show/hide full message content, tool args, etc.

---

### Task 7: Frontend — Compare View (compare.html + compare.js)

Side-by-side dual runs with diff highlighting.

**Files:**
- Create: `crates/alva-eval/static/compare.html`
- Create: `crates/alva-eval/static/compare.js`

**Step 1: Create compare.html**

Layout:
```
┌─────────────────────────────────────────────────────────────────┐
│  alva-eval    [Playground]  [Inspector]  [Compare]              │
├─────────────────────────────┬───────────────────────────────────┤
│  Config A                   │  Config B                         │
│  [same form as playground]  │  [same form as playground]        │
├─────────────────────────────┼───────────────────────────────────┤
│  Event Stream A             │  Event Stream B                   │
│  (SSE real-time)            │  (SSE real-time)                  │
├─────────────────────────────┴───────────────────────────────────┤
│  Diff Summary (after both complete):                            │
│  Turns: 2 vs 3  │  Tokens: 1.5k vs 2.1k  │  Time: 4.8s vs 6.2s│
│  Tools A only: [grep_search]  │  Tools B only: [list_files]     │
└─────────────────────────────────────────────────────────────────┘
```

**Step 2: Create compare.js**

Core logic:
- Two config forms (A and B)
- "Run Both" button → `POST /api/run` × 2 concurrently
- Two SSE streams side by side
- After both complete: `GET /api/compare/:id_a/:id_b` → render diff summary

---

### Task 8: Frontend — Enhanced Playground with Turn Details

Upgrade the existing Playground to show per-turn detail panels inline.

**Files:**
- Modify: `crates/alva-eval/static/app.js`
- Modify: `crates/alva-eval/static/style.css`
- Modify: `crates/alva-eval/static/index.html`

**Step 1: Add navigation tabs**

In `index.html`, add a nav bar:
```html
<nav class="nav">
  <a href="/" class="nav-link active">Playground</a>
  <a href="/inspector.html" class="nav-link">Inspector</a>
  <a href="/compare.html" class="nav-link">Compare</a>
</nav>
```

**Step 2: Add skill picker to sidebar**

After the tool picker in `index.html`, add a skill section:
```html
<label>Skills</label>
<div class="tool-actions">
  <button onclick="scanSkills()">Scan</button>
</div>
<div class="tool-picker" id="skill-picker">
  <div style="color:var(--text-dim);font-size:12px;padding:8px">Click Scan to discover skills</div>
</div>
```

**Step 3: Add "View Details" link after run completes**

In `app.js`, when `AgentEnd` event arrives, add a link:
```javascript
addCard('end-ok', 'Agent Finished',
  `... <a href="/inspector.html?run=${runId}">View Details</a>`);
```

**Step 4: Verify and commit**

---

### Task 9: Integration Test — Full Pipeline

Verify the entire flow works: start run → SSE events → record stored → inspector loads.

**Files:**
- Create: `crates/alva-eval/tests/integration.rs`

**Step 1: Write integration test**

```rust
use alva_test::mock_provider::MockLanguageModel;
use alva_test::fixtures::make_assistant_message;
// ... test that:
// 1. POST /api/run with mock model → returns run_id
// 2. GET /api/events/:run_id → receives AgentStart, MessageStart, ..., AgentEnd
// 3. GET /api/records/:run_id → returns RunRecord with correct turn count
// 4. GET /api/runs → lists the run
```

**Step 2: Run test**

Run: `cargo test -p alva-eval`

**Step 3: Commit**

```bash
git add crates/alva-eval/
git commit -m "feat(eval): full phase 1 — recorder, skills, compare, inspector views"
```

---

## Summary

| Task | Component | Description |
|------|-----------|-------------|
| 1 | recorder.rs | RunRecord data model (types only) |
| 2 | recorder.rs | RecorderMiddleware impl (Middleware trait) |
| 3 | main.rs | Wire recorder into pipeline + store records + /api/records + /api/runs |
| 4 | skills.rs | Skill discovery: scan .alva/, .claude/, custom dirs |
| 5 | main.rs | Compare API: concurrent dual runs + diff |
| 6 | inspector.{html,js} | Inspector view: expandable turn timeline |
| 7 | compare.{html,js} | Compare view: side-by-side dual runs |
| 8 | index.html, app.js | Enhanced playground: nav tabs, skill picker, detail links |
| 9 | tests/ | Integration test: full pipeline verification |
