# SpawnScope Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build SpawnScope — the unified execution context that manages all agent lifecycle concerns (boards, sessions, depth, tools, budgets), making Agent a pure stateless executor.

**Architecture:** SpawnScope is a tree of scopes mirroring the agent spawn tree. Each scope owns its isolation/sharing rules. Agent receives a scope at creation time and queries it for everything — model, tools, board, session. Scope is defined as traits in `alva-types` (so all crates can depend on it) with concrete implementation in `alva-app-core`.

**Tech Stack:** Rust, alva-types (traits), alva-app-core (implementation), Arc + AtomicU32 for shared state, serde for persistence.

---

## File Structure

### New files

| File | Responsibility |
|------|---------------|
| `crates/alva-types/src/scope.rs` | SpawnScope trait + ScopeConfig types + ScopeId |
| `crates/alva-app-core/src/scope/mod.rs` | Module root |
| `crates/alva-app-core/src/scope/scope_impl.rs` | Concrete `SpawnScopeImpl` |
| `crates/alva-app-core/src/scope/scope_tree.rs` | Tree tracker (parent-child relationships) |
| `crates/alva-app-core/src/scope/board_registry.rs` | Board lifecycle + visibility rules |
| `crates/alva-app-core/src/scope/session_tracker.rs` | Session tree + persistence |

### Modified files

| File | Change |
|------|--------|
| `crates/alva-types/src/lib.rs` | Add `pub mod scope` |
| `crates/alva-types/src/tool_guard.rs` | Minor: expose `active` count for scope reporting |
| `crates/alva-app-core/src/lib.rs` | Add `pub mod scope` |
| `crates/alva-app-core/src/plugins/agent_spawn.rs` | Rewrite to use SpawnScope instead of ad-hoc state |
| `crates/alva-app-core/src/base_agent.rs` | Create root SpawnScope in `build()` |
| `crates/alva-app-core/src/bin/cli/main.rs` | Pass scope config |

---

## Design: SpawnScope trait

```rust
// alva-types/src/scope.rs — the contract

pub trait SpawnScope: Send + Sync {
    /// Unique scope identifier.
    fn id(&self) -> &ScopeId;

    /// Parent scope (None = root).
    fn parent(&self) -> Option<&ScopeId>;

    /// Current depth in the spawn tree (root = 0).
    fn depth(&self) -> u32;

    /// Try to create a child scope for a new agent spawn.
    /// Returns Err if depth/budget exceeded.
    fn spawn_child(&self, config: ChildScopeConfig) -> Result<Arc<dyn SpawnScope>, ScopeError>;

    /// Get the model to use for this scope.
    fn model(&self) -> Arc<dyn LanguageModel>;

    /// Get the tools available in this scope.
    fn tools(&self, inherit_parent: bool) -> Vec<Arc<dyn Tool>>;

    /// Get or create a named board. Visibility follows scope rules.
    fn board(&self, board_id: &str) -> Arc<Blackboard>;

    /// Get the board visible from this scope (parent's board, read-only view).
    fn parent_board(&self) -> Option<Arc<Blackboard>>;

    /// Session ID for this scope.
    fn session_id(&self) -> &str;

    /// Timeout for agent execution in this scope.
    fn timeout(&self) -> Duration;

    /// Max iterations for agent loop.
    fn max_iterations(&self) -> u32;

    /// Record that this scope's agent has completed.
    fn mark_completed(&self, output: &str);
}
```

---

## Task Breakdown

### Task 1: SpawnScope trait + types in alva-types

**Files:**
- Create: `crates/alva-types/src/scope.rs`
- Modify: `crates/alva-types/src/lib.rs`

- [ ] **Step 1: Write the failing test**

```rust
// crates/alva-types/src/scope.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_id_display() {
        let id = ScopeId::new();
        assert!(!id.to_string().is_empty());
    }

    #[test]
    fn child_config_builder() {
        let config = ChildScopeConfig::new("planner")
            .with_system_prompt("You plan.")
            .with_timeout(Duration::from_secs(120))
            .inherit_tools(true);
        assert_eq!(config.role, "planner");
        assert!(config.inherit_tools);
    }

    #[test]
    fn scope_error_display() {
        let err = ScopeError::DepthExceeded { current: 3, max: 3 };
        assert!(err.to_string().contains("depth"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p alva-types -- scope::tests -v`
Expected: FAIL — module doesn't exist

- [ ] **Step 3: Implement scope types**

```rust
// crates/alva-types/src/scope.rs

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::model::LanguageModel;
use crate::tool::Tool;

/// Unique scope identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopeId(String);

impl ScopeId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScopeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Configuration for creating a child scope.
#[derive(Debug, Clone)]
pub struct ChildScopeConfig {
    /// Agent role name.
    pub role: String,
    /// System prompt for the child agent.
    pub system_prompt: String,
    /// Whether to inherit parent's tools.
    pub inherit_tools: bool,
    /// Optional board ID to join (None = create isolated scope).
    pub board_id: Option<String>,
    /// Override timeout (None = inherit from parent).
    pub timeout: Option<Duration>,
    /// Override max iterations (None = inherit from parent).
    pub max_iterations: Option<u32>,
}

impl ChildScopeConfig {
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            system_prompt: String::new(),
            inherit_tools: false,
            board_id: None,
            timeout: None,
            max_iterations: None,
        }
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    pub fn inherit_tools(mut self, yes: bool) -> Self {
        self.inherit_tools = yes;
        self
    }

    pub fn with_board(mut self, board_id: impl Into<String>) -> Self {
        self.board_id = Some(board_id.into());
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = Some(max);
        self
    }
}

/// Errors when scope operations fail.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ScopeError {
    #[error("depth exceeded: current {current} >= max {max}")]
    DepthExceeded { current: u32, max: u32 },

    #[error("budget exceeded: {reason}")]
    BudgetExceeded { reason: String },

    #[error("scope error: {0}")]
    Other(String),
}

/// Snapshot of a scope's state (for debugging/logging).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScopeSnapshot {
    pub id: String,
    pub parent_id: Option<String>,
    pub depth: u32,
    pub role: String,
    pub board_id: Option<String>,
    pub session_id: String,
    pub children_count: usize,
    pub completed: bool,
}
```

- [ ] **Step 4: Register module**

Add to `crates/alva-types/src/lib.rs`:
```rust
pub mod scope;
```

- [ ] **Step 5: Run tests, verify pass**

Run: `cargo test -p alva-types -- scope -v`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/alva-types/src/scope.rs crates/alva-types/src/lib.rs
git commit -m "feat(types): add SpawnScope trait, ScopeId, ChildScopeConfig, ScopeError"
```

---

### Task 2: Board registry with visibility rules

**Files:**
- Create: `crates/alva-app-core/src/scope/board_registry.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn same_scope_shares_board() {
        let reg = BoardRegistry::new();
        let scope_a = ScopeId::new();

        let b1 = reg.get_or_create(&scope_a, "team-1").await;
        let b2 = reg.get_or_create(&scope_a, "team-1").await;

        // Same board instance
        b1.post(BoardMessage::new("a", "hello")).await;
        assert_eq!(b2.message_count().await, 1);
    }

    #[tokio::test]
    async fn different_scope_different_board() {
        let reg = BoardRegistry::new();
        let scope_a = ScopeId::new();
        let scope_b = ScopeId::new();

        let b1 = reg.get_or_create(&scope_a, "work").await;
        let b2 = reg.get_or_create(&scope_b, "work").await;

        b1.post(BoardMessage::new("a", "hello")).await;
        assert_eq!(b2.message_count().await, 0); // isolated
    }

    #[tokio::test]
    async fn child_can_read_parent_board() {
        let reg = BoardRegistry::new();
        let parent = ScopeId::new();
        let child = ScopeId::new();

        reg.set_parent(&child, &parent);

        let parent_board = reg.get_or_create(&parent, "team").await;
        parent_board.post(BoardMessage::new("boss", "task")).await;

        let view = reg.parent_board_for(&child, "team").await;
        assert!(view.is_some());
        assert_eq!(view.unwrap().message_count().await, 1);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p alva-app-core -- scope::board_registry::tests -v`
Expected: FAIL

- [ ] **Step 3: Implement BoardRegistry**

```rust
// crates/alva-app-core/src/scope/board_registry.rs

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use alva_types::scope::ScopeId;
use crate::plugins::blackboard::Blackboard;

/// Manages Blackboard instances scoped to SpawnScope IDs.
///
/// Rules:
/// - Same scope + same board_id → same Blackboard instance
/// - Different scope → different Blackboard (even with same board_id)
/// - Child scope can read (not write) parent's board via parent_board_for()
pub struct BoardRegistry {
    /// (scope_id, board_id) → Blackboard
    boards: Mutex<HashMap<(String, String), Arc<Blackboard>>>,
    /// child_scope → parent_scope
    parents: Mutex<HashMap<String, String>>,
}
```

Key method: `get_or_create`, `parent_board_for`, `set_parent`.

- [ ] **Step 4: Run tests, verify pass**

- [ ] **Step 5: Commit**

```bash
git add crates/alva-app-core/src/scope/
git commit -m "feat(scope): add BoardRegistry with scope-based isolation + parent read access"
```

---

### Task 3: Session tracker (tree-structured)

**Files:**
- Create: `crates/alva-app-core/src/scope/session_tracker.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_session_has_no_parent() {
        let tracker = SessionTracker::new();
        let root = tracker.create_root("test-workspace");
        assert!(tracker.parent_of(&root).is_none());
    }

    #[test]
    fn child_session_links_to_parent() {
        let tracker = SessionTracker::new();
        let root = tracker.create_root("ws");
        let child = tracker.create_child(&root, "planner");
        assert_eq!(tracker.parent_of(&child), Some(root.clone()));
    }

    #[test]
    fn children_listed_under_parent() {
        let tracker = SessionTracker::new();
        let root = tracker.create_root("ws");
        let c1 = tracker.create_child(&root, "planner");
        let c2 = tracker.create_child(&root, "coder");
        let children = tracker.children_of(&root);
        assert_eq!(children.len(), 2);
    }

    #[test]
    fn tree_snapshot() {
        let tracker = SessionTracker::new();
        let root = tracker.create_root("ws");
        let _c1 = tracker.create_child(&root, "planner");
        let snapshot = tracker.snapshot(&root);
        assert_eq!(snapshot.children_count, 1);
    }
}
```

- [ ] **Step 2: Implement SessionTracker**

```rust
// Tracks session parent-child relationships.
// Each scope gets a session_id; children link to parents.
// Provides tree traversal and snapshot for debugging.

pub struct SessionTracker {
    sessions: Mutex<HashMap<String, SessionNode>>,
}

struct SessionNode {
    id: String,
    parent_id: Option<String>,
    role: String,
    children: Vec<String>,
    created_at: i64,
    completed: bool,
    output_summary: Option<String>,
}
```

- [ ] **Step 3: Run tests, verify pass**

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(scope): add SessionTracker with tree-structured session management"
```

---

### Task 4: SpawnScopeImpl (concrete implementation)

**Files:**
- Create: `crates/alva-app-core/src/scope/scope_impl.rs`
- Create: `crates/alva-app-core/src/scope/mod.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn root_scope_depth_is_zero() {
        let scope = test_root_scope();
        assert_eq!(scope.depth(), 0);
        assert!(scope.parent().is_none());
    }

    #[tokio::test]
    async fn child_scope_increments_depth() {
        let root = test_root_scope();
        let child = root.spawn_child(ChildScopeConfig::new("planner")).unwrap();
        assert_eq!(child.depth(), 1);
        assert_eq!(child.parent(), Some(root.id()));
    }

    #[tokio::test]
    async fn depth_limit_enforced() {
        let root = test_root_scope_with_max_depth(2);
        let c1 = root.spawn_child(ChildScopeConfig::new("a")).unwrap();
        let c2 = c1.spawn_child(ChildScopeConfig::new("b")).unwrap();
        let result = c2.spawn_child(ChildScopeConfig::new("c"));
        assert!(matches!(result, Err(ScopeError::DepthExceeded { .. })));
    }

    #[tokio::test]
    async fn board_isolation_between_scopes() {
        let root = test_root_scope();
        let c1 = root.spawn_child(
            ChildScopeConfig::new("a").with_board("work")
        ).unwrap();
        let c2 = root.spawn_child(
            ChildScopeConfig::new("b").with_board("work")
        ).unwrap();

        // c1 and c2 are siblings under root — same board ID
        // under the SAME parent, so they share the board
        let b1 = c1.board("work");
        let b2 = c2.board("work");
        b1.post(BoardMessage::new("a", "hello")).await;
        assert_eq!(b2.message_count().await, 1);
    }

    #[tokio::test]
    async fn child_reads_parent_board() {
        let root = test_root_scope();
        let root_board = root.board("team");
        root_board.post(BoardMessage::new("root", "task")).await;

        let child = root.spawn_child(
            ChildScopeConfig::new("worker").with_board("team")
        ).unwrap();

        let parent_view = child.parent_board();
        assert!(parent_view.is_some());
    }

    #[tokio::test]
    async fn tools_inheritance() {
        let root = test_root_scope();
        let tools = root.tools(false);
        // Should only have spawn tool (no inheritance = reasoning only)
        let has_agent_tool = tools.iter().any(|t| t.name() == "agent");
        assert!(has_agent_tool);

        let inherited_tools = root.tools(true);
        assert!(inherited_tools.len() >= tools.len());
    }
}
```

- [ ] **Step 2: Implement SpawnScopeImpl**

```rust
pub struct SpawnScopeImpl {
    id: ScopeId,
    parent_id: Option<ScopeId>,
    depth: u32,
    role: String,
    session_id: String,

    // Shared across tree
    model: Arc<dyn LanguageModel>,
    guard: ToolGuard,
    parent_tools: Arc<Vec<Arc<dyn Tool>>>,
    board_registry: Arc<BoardRegistry>,
    session_tracker: Arc<SessionTracker>,

    // Per-scope config
    board_id: Option<String>,
    system_prompt: String,
    timeout: Duration,
    max_iterations: u32,
    inherit_tools: bool,
}
```

Key: `spawn_child()` creates a new `SpawnScopeImpl` with:
- `depth = self.depth + 1`
- Same `guard`, `board_registry`, `session_tracker` (Arc clone)
- New `ScopeId`, new `session_id`
- Registered as child in `session_tracker`

- [ ] **Step 3: Run tests, verify pass**

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(scope): implement SpawnScopeImpl with tree spawning, board isolation, tool inheritance"
```

---

### Task 5: Rewrite AgentSpawnTool to use SpawnScope

**Files:**
- Modify: `crates/alva-app-core/src/plugins/agent_spawn.rs`

- [ ] **Step 1: Write the failing test**

Test that AgentSpawnTool creates child scope, respects depth, uses correct board.

- [ ] **Step 2: Rewrite AgentSpawnTool**

Before (current):
```rust
pub struct AgentSpawnTool {
    model: Arc<dyn LanguageModel>,
    parent_tools: Arc<Vec<Arc<dyn Tool>>>,
    guard: ToolGuard,
    boards: Arc<Mutex<HashMap<String, Arc<Blackboard>>>>,
}
```

After:
```rust
pub struct AgentSpawnTool {
    scope: Arc<dyn SpawnScope>,
}

impl AgentSpawnTool {
    pub fn new(scope: Arc<dyn SpawnScope>) -> Self {
        Self { scope }
    }
}
```

`execute()` becomes:
```rust
async fn execute(&self, input, cancel, ctx) {
    let child_scope = self.scope.spawn_child(ChildScopeConfig {
        role: input.role,
        system_prompt: input.system_prompt,
        inherit_tools: input.inherit_tools,
        board_id: input.board,
        ..
    })?;

    let tools = child_scope.tools(input.inherit_tools);
    let agent = Agent::new(child_scope.model(), child_scope.session_id(), ...);
    agent.set_tools(tools).await;

    // ... run agent ...

    child_scope.mark_completed(&output);
    Ok(ToolResult { content: output, .. })
}
```

Massive simplification — all state management delegated to scope.

- [ ] **Step 3: Run tests, verify pass**

- [ ] **Step 4: Commit**

```bash
git commit -m "refactor(agent-spawn): rewrite to use SpawnScope — tool becomes stateless"
```

---

### Task 6: Wire into BaseAgent

**Files:**
- Modify: `crates/alva-app-core/src/base_agent.rs`
- Modify: `crates/alva-app-core/src/bin/cli/main.rs`

- [ ] **Step 1: Create root scope in BaseAgent::build()**

```rust
// In build():
let root_scope = Arc::new(SpawnScopeImpl::root(
    model.clone(),
    ToolGuard::max_depth(self.sub_agent_max_depth),
    alva_tools_list.clone(),
    ScopeConfig {
        timeout: Duration::from_secs(300),
        max_iterations: self.max_iterations,
    },
));

// Register spawn tool with the root scope
if self.enable_sub_agents {
    alva_tools_list.push(Arc::new(
        AgentSpawnTool::new(root_scope.clone())
    ));
}
```

- [ ] **Step 2: CLI gets scope config for free**

```rust
// No changes needed in CLI — BaseAgent handles it
.with_sub_agents()
.sub_agent_max_depth(3)  // policy in app layer
.build(model)
```

- [ ] **Step 3: Run full test suite**

Run: `cargo test -p alva-app-core --lib`
Expected: All pass

- [ ] **Step 4: Commit**

```bash
git commit -m "feat(base-agent): create root SpawnScope in build(), wire into agent spawn tool"
```

---

### Task 7: Integration test — multi-level spawn

**Files:**
- Create: `crates/alva-app-core/tests/scope_integration.rs`

- [ ] **Step 1: Write integration test**

Test scenario:
```
Root Agent (depth 0)
  → spawn planner (depth 1, board="proj")
    → spawn researcher (depth 2, own board, can read "proj")
  → spawn coder (depth 1, board="proj", sees planner output)
  → spawn at depth 3 → REFUSED
```

Verify:
- Depth is tracked correctly
- Board isolation works (siblings share, children read parent)
- Sessions form a tree
- All sessions can be serialized

- [ ] **Step 2: Run integration test**

- [ ] **Step 3: Commit**

```bash
git commit -m "test(scope): integration test for multi-level spawn with board isolation"
```

---

## Board Visibility Rules (Reference)

```
Rule 1: 同一个 parent scope 下的 children，传同一个 board_id → 共享同一个 Board
         (siblings on the same team share a board)

Rule 2: child scope 可以只读访问 parent scope 的 board
         (children can see what the parent team is doing)

Rule 3: child scope 自己创建的 board，parent 看不到
         (children's internal communication is private)

Rule 4: 跨 parent 的 scope，即使 board_id 一样也不共享
         (unrelated scopes are fully isolated)
```

```
Scope-0 (root)
├── board "team-A"
│
├── Scope-1a (planner, board="team-A")
│   ├── can READ "team-A" (same parent → shared)
│   └── own board "research" (root can't see)
│       │
│       └── Scope-2a (researcher, board="research")
│           ├── can READ Scope-1a's "research" (parent)
│           └── cannot see Scope-0's "team-A" (grandparent, too far)
│
└── Scope-1b (coder, board="team-A")
    └── can READ "team-A" (same parent → shared with planner)
```

---

## Session Tree (Reference)

```
root-session-abc
├── spawn-planner-123
│   ├── messages: [user, assistant, ...]
│   └── spawn-researcher-456
│       └── messages: [user, assistant, ...]
├── spawn-coder-789
│   └── messages: [user, assistant, ...]
└── metadata: { total_children: 3, total_depth: 2 }
```

Persisted to `.alva/sessions/{root-id}/tree.json` with all sub-sessions inline.
