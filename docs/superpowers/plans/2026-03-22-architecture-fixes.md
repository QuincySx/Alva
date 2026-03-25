# Architecture Fixes (P0–P3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix all architecture issues identified in the review: hardcoded paths (P0), type design problems (P1), hook system limitations (P2), and engine functionality gaps (P3).

**Architecture:** Bottom-up approach — fix foundation types first (alva-types), then engine (alva-core), then graph layer (alva-graph), then protocol crates. Each task produces a compiling workspace. Breaking API changes are batched to minimize churn for consumers (alva-app, alva-app-core).

**Tech Stack:** Rust, async-trait, tokio, serde, futures-core

---

## File Structure

### alva-types changes
- Modify: `crates/alva-types/src/message.rs` — remove `tool_calls` field from Message, add computed accessor
- Modify: `crates/alva-types/src/content.rs` — keep ContentBlock::ToolUse as canonical location
- Modify: `crates/alva-types/src/tool.rs` — remove `tool_call_id` from ToolResult (engine fills it)

### alva-core changes
- Modify: `crates/alva-core/src/types.rs` — async hooks, composable Vec, remove system_prompt from convert_to_llm signature
- Modify: `crates/alva-core/src/agent.rs` — adapt to new hook signatures
- Modify: `crates/alva-core/src/agent_loop.rs` — add streaming support, adapt to new Message/hook APIs
- Modify: `crates/alva-core/src/tool_executor.rs` — adapt to async hooks
- Modify: `crates/alva-core/src/event.rs` — no changes needed
- Modify: `crates/alva-core/Cargo.toml` — may need futures-util for StreamExt

### alva-graph changes
- Modify: `crates/alva-graph/src/graph.rs` — make StateGraph generic over `S: Serialize + DeserializeOwned + Send`
- Modify: `crates/alva-graph/src/pregel.rs` — make CompiledGraph generic, add parallel superstep execution
- Modify: `crates/alva-graph/src/channel.rs` — keep as-is (already typed, will be integrated later)
- Modify: `crates/alva-graph/src/session.rs` — adapt to generic CompiledGraph
- Modify: `crates/alva-graph/src/compaction.rs` — adapt to new Message API
- Modify: `crates/alva-graph/src/lib.rs` — update re-exports

### protocol crate changes
- Modify: `crates/alva-mcp/src/config.rs` — remove hardcoded path, inject via parameter
- Modify: `crates/alva-mcp/src/tool_adapter.rs` — remove empty tool_call_id
- Modify: `crates/alva-acp/src/connection.rs` — inject packages_dir, extract ExternalAgentKind to app layer
- Modify: `crates/alva-acp/src/delegate.rs` — adapt to new ExternalAgentKind
- Modify: `crates/alva-acp/src/lib.rs` — update exports

### alva-app-core duplicate code (must update in lockstep with protocol crates)
- Modify: `crates/alva-app-core/src/mcp/config.rs` — has same hardcoded `~/.srow` path as alva-mcp
- Modify: `crates/alva-app-core/src/agent/agent_client/connection/discovery.rs` — duplicate ExternalAgentKind + AgentDiscovery + builtin_packages_dir
- Modify: `crates/alva-app-core/src/agent/agent_client/delegate.rs` — duplicate agent_kind() match
- Modify: `crates/alva-app-core/src/agent/agent_client/connection/factory.rs` — calls AgentDiscovery::discover() statically
- Modify: `crates/alva-app-core/src/agent/agent_client/mod.rs` — re-exports ExternalAgentKind
- Modify: `crates/alva-app-core/tests/acp_integration.rs` — uses ExternalAgentKind::Generic

### Consumer changes
- Modify: `crates/alva-app/src/chat/gpui_chat.rs` — adapt to new APIs

---

## Task 1: P0 — Remove hardcoded paths from both MCP config files

**Files:**
- Modify: `crates/alva-mcp/src/config.rs`
- Modify: `crates/alva-app-core/src/mcp/config.rs` (duplicate with same hardcoded path)

**Note:** alva-app-core has its own `McpConfig` struct at `crates/alva-app-core/src/mcp/config.rs` with the same `default_path()`, `load_default()`, `save_default()` hardcoding `~/.srow/`. Both must be fixed.

- [ ] **Step 1: Remove `default_path()` and `load_default()`/`save_default()` from alva-mcp**

```rust
// REMOVE these methods from McpConfigFile impl in alva-mcp/src/config.rs:
// - pub fn default_path() -> PathBuf         (line 61)
// - pub async fn load_default()              (line 67)
// - pub async fn save_default(&self)         (line 88)
// Also remove module doc reference to ~/.srow (line 5-6).
// The existing load(path) and save(path) methods stay as-is.
```

- [ ] **Step 2: Remove same methods from alva-app-core's McpConfig**

```rust
// REMOVE from crates/alva-app-core/src/mcp/config.rs:
// - pub fn default_path() -> PathBuf
// - pub async fn load_default()
// - pub async fn save_default(&self)
// Also remove module doc reference to ~/.srow.
```

- [ ] **Step 3: Find and fix all callers**

Run: `grep -rn "load_default\|save_default\|default_path" crates/`
All callers must pass an explicit path. The app-specific default path (`~/.srow/mcpServerConfig.json`) should be defined in alva-app or alva-app-core as a constant, not in the generic protocol crate.

- [ ] **Step 4: Run `cargo check` to verify full workspace compiles**

- [ ] **Step 5: Commit**

```bash
git add crates/alva-mcp/src/config.rs crates/alva-app-core/src/mcp/config.rs
git commit -m "fix: remove hardcoded ~/.srow path from MCP config — callers supply path"
```

---

## Task 2: P0 — Remove hardcoded paths from alva-acp

**Files:**
- Modify: `crates/alva-acp/src/connection.rs`
- Modify: `crates/alva-acp/src/delegate.rs`
- Modify: `crates/alva-acp/src/lib.rs`

- [ ] **Step 1: Make `builtin_packages_dir()` accept an app_name parameter**

```rust
// BEFORE (line 177):
fn builtin_packages_dir() -> PathBuf { ... .join("srow-agent") ... }

// AFTER:
fn builtin_packages_dir(app_name: &str) -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            #[cfg(target_os = "windows")]
            { PathBuf::from("C:\\Temp") }
            #[cfg(not(target_os = "windows"))]
            { PathBuf::from("/tmp") }
        })
        .join(app_name)
        .join("packages")
}
```

- [ ] **Step 2: Add `packages_dir` to `AgentDiscovery`**

```rust
pub struct AgentDiscovery {
    packages_dir: PathBuf,
}

impl AgentDiscovery {
    pub fn new(app_name: &str) -> Self {
        Self {
            packages_dir: builtin_packages_dir(app_name),
        }
    }

    pub fn with_packages_dir(packages_dir: PathBuf) -> Self {
        Self { packages_dir }
    }

    pub fn discover(&self, kind: &ExternalAgentKind) -> Result<AgentCliCommand, AcpError> {
        match kind {
            ExternalAgentKind::Generic { command } => Self::discover_generic(command),
            _ => self.discover_known(kind),
        }
    }
}
```

- [ ] **Step 3: Replace `ExternalAgentKind` named variants with a registry pattern**

```rust
// BEFORE:
pub enum ExternalAgentKind {
    ClaudeCode,
    QwenCode,
    CodexCli,
    GeminiCli,
    Generic { command: String },
}

// AFTER — protocol crate only knows Generic + Named:
pub enum ExternalAgentKind {
    /// Well-known agent with a string identifier and discovery hints.
    Named {
        id: String,
        executables: Vec<String>,    // binary names to search in PATH
        fallback_npx: Option<String>, // optional npx package fallback
    },
    /// User-specified arbitrary command.
    Generic { command: String },
}
```

- [ ] **Step 4: Update `AgentDiscovery` methods to use the new enum**

Replace the four per-agent methods (`discover_claude_code`, `discover_qwen_code`, etc.) with a single generic `discover_named()` that iterates `executables`, checks builtin packages dir, and falls back to npx.

```rust
fn discover_named(&self, kind: &ExternalAgentKind) -> Result<AgentCliCommand, AcpError> {
    let ExternalAgentKind::Named { id, executables, fallback_npx } = kind else {
        unreachable!();
    };

    // 1. Try each executable name in PATH
    for exe_name in executables {
        if let Some(exe) = which(exe_name) {
            return Ok(AgentCliCommand {
                kind: kind.clone(),
                executable: exe,
                args: vec![],
            });
        }
    }

    // 2. Try builtin packages dir
    for exe_name in executables {
        let builtin = self.packages_dir
            .join(id)
            .join("node_modules")
            .join(".bin")
            .join(exe_name);
        if builtin.exists() {
            return Ok(AgentCliCommand {
                kind: kind.clone(),
                executable: builtin,
                args: vec![],
            });
        }
    }

    // 3. Try npx fallback
    if let Some(npx_pkg) = fallback_npx {
        if let Some(npx) = which("npx") {
            return Ok(AgentCliCommand {
                kind: kind.clone(),
                executable: npx,
                args: vec![npx_pkg.clone()],
            });
        }
    }

    Err(AcpError::AgentNotFound {
        kind: id.clone(),
        hint: format!("Ensure one of {:?} is in $PATH", executables),
    })
}
```

- [ ] **Step 5: Update `delegate.rs` to use new enum**

```rust
// agent_kind() now returns the id string:
fn agent_kind(&self) -> &str {
    match &self.kind {
        ExternalAgentKind::Named { id, .. } => id.as_str(),
        ExternalAgentKind::Generic { command } => command.as_str(),
    }
}
```

- [ ] **Step 6: Update `AcpProcessManager::spawn` to take `AgentDiscovery` reference**

```rust
pub async fn spawn(
    &self,
    discovery: &AgentDiscovery,
    kind: ExternalAgentKind,
    bootstrap: BootstrapPayload,
) -> Result<String, AcpError> {
    let cmd = discovery.discover(&kind)?;
    // ... rest unchanged
}
```

- [ ] **Step 7: Delete alva-app-core's duplicate discovery code and re-export from alva-acp**

**Critical:** alva-app-core has a COMPLETE DUPLICATE of `ExternalAgentKind`, `AgentDiscovery`, `AgentCliCommand`, `builtin_packages_dir()` at `crates/alva-app-core/src/agent/agent_client/connection/discovery.rs`. This must be deleted and replaced with re-exports from `alva-acp`.

Files to update:
- `crates/alva-app-core/src/agent/agent_client/connection/discovery.rs` — delete all discovery logic, replace with re-exports + well-known agent constants
- `crates/alva-app-core/src/agent/agent_client/delegate.rs:66-76` — update `agent_kind()` match to use new enum
- `crates/alva-app-core/src/agent/agent_client/connection/factory.rs:69` — update `AgentDiscovery::discover(&kind)?` to use instance method
- `crates/alva-app-core/src/agent/agent_client/mod.rs` — update re-exports
- `crates/alva-app-core/tests/acp_integration.rs:47` — update `ExternalAgentKind::Generic` usage

Replace `crates/alva-app-core/src/agent/agent_client/connection/discovery.rs` with:

```rust
// Re-export alva-acp types
pub use alva_acp::{ExternalAgentKind, AgentCliCommand, AgentDiscovery};

// Well-known agent kind constructors (app-specific knowledge lives here, not in the protocol crate)
pub fn claude_code() -> ExternalAgentKind {
    ExternalAgentKind::Named {
        id: "claude-code".into(),
        executables: vec!["claude-code-acp".into()],
        fallback_npx: None,
    }
}

pub fn qwen_code() -> ExternalAgentKind {
    ExternalAgentKind::Named {
        id: "qwen-code".into(),
        executables: vec!["qwen".into()],
        fallback_npx: None,
    }
}

pub fn codex_cli() -> ExternalAgentKind {
    ExternalAgentKind::Named {
        id: "codex-cli".into(),
        executables: vec!["codex-acp".into()],
        fallback_npx: Some("@zed-industries/codex-acp".into()),
    }
}

pub fn gemini_cli() -> ExternalAgentKind {
    ExternalAgentKind::Named {
        id: "gemini-cli".into(),
        executables: vec!["gemini".into(), "gemini-cli".into()],
        fallback_npx: None,
    }
}
```

Update `crates/alva-app-core/src/agent/agent_client/connection/factory.rs:69`:
```rust
// BEFORE:
let cmd = super::discovery::AgentDiscovery::discover(&kind)?;

// AFTER (factory must hold or receive an AgentDiscovery instance):
let cmd = discovery.discover(&kind)?;
```

Update `crates/alva-app-core/src/agent/agent_client/delegate.rs:66-76`:
```rust
// BEFORE: match on ClaudeCode/QwenCode/CodexCli/GeminiCli
// AFTER:
fn agent_kind(&self) -> &str {
    match &self.kind {
        ExternalAgentKind::Named { id, .. } => id.as_str(),
        ExternalAgentKind::Generic { command } => command.as_str(),
    }
}
```

Update `crates/alva-app-core/tests/acp_integration.rs:47`:
```rust
// BEFORE: ExternalAgentKind::Generic { command: "echo".into() }
// AFTER: same (Generic variant unchanged)
```

- [ ] **Step 8: Run `cargo check` to verify full workspace compiles**

- [ ] **Step 9: Commit**

```bash
git add crates/alva-acp/ crates/alva-app-core/
git commit -m "fix(alva-acp): remove hardcoded paths and product-specific agent kinds"
```

---

## Task 3: P1 — Unify ToolCall representation in alva-types

**Files:**
- Modify: `crates/alva-types/src/message.rs`
- Modify: `crates/alva-core/src/agent_loop.rs`
- Modify: `crates/alva-app/src/chat/gpui_chat.rs`

- [ ] **Step 1: Replace `tool_calls: Vec<ToolCallData>` with a computed accessor**

In `message.rs`, remove the `tool_calls` field and `ToolCallData` struct. Tool calls are already represented in `content` as `ContentBlock::ToolUse`:

```rust
// REMOVE from Message:
//   pub tool_calls: Vec<ToolCallData>,

// REMOVE struct ToolCallData entirely.

// ADD computed accessor to Message impl:
impl Message {
    /// Extract tool calls from content blocks.
    pub fn tool_calls(&self) -> Vec<&ContentBlock> {
        self.content
            .iter()
            .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
            .collect()
    }

    /// Check if this message contains any tool calls.
    pub fn has_tool_calls(&self) -> bool {
        self.content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }))
    }
}
```

- [ ] **Step 2: Update agent_loop.rs tool call extraction (lines 114-122)**

```rust
// BEFORE:
let tool_calls: Vec<ToolCall> = assistant_message
    .tool_calls
    .iter()
    .map(|tc| ToolCall {
        id: tc.id.clone(),
        name: tc.name.clone(),
        arguments: tc.arguments.clone(),
    })
    .collect();

// AFTER:
let tool_calls: Vec<ToolCall> = assistant_message
    .content
    .iter()
    .filter_map(|b| match b {
        ContentBlock::ToolUse { id, name, input } => Some(ToolCall {
            id: id.clone(),
            name: name.clone(),
            arguments: input.clone(),
        }),
        _ => None,
    })
    .collect();
```

- [ ] **Step 3: Update lib.rs re-exports — remove ToolCallData**

```rust
// BEFORE:
pub use message::{Message, MessageRole, ToolCallData, UsageMetadata};

// AFTER:
pub use message::{Message, MessageRole, UsageMetadata};
```

- [ ] **Step 4: Fix all consumers that reference `tool_calls` field or `ToolCallData`**

In `gpui_chat.rs` (line 62), remove `tool_calls: vec![],` from Message construction. The `content` vec already serves as the canonical source.

Search for other consumers: `grep -r "ToolCallData\|\.tool_calls" crates/`

- [ ] **Step 5: Run `cargo check` to verify full workspace compiles**

- [ ] **Step 6: Run existing tests**

Run: `cargo test -p alva-types -p alva-core`

- [ ] **Step 7: Commit**

```bash
git add crates/alva-types/ crates/alva-core/ crates/alva-app/
git commit -m "refactor(alva-types): unify tool call representation — ContentBlock::ToolUse is canonical"
```

---

## Task 4: P1 — Make StateGraph and CompiledGraph generic

**Files:**
- Modify: `crates/alva-graph/src/graph.rs`
- Modify: `crates/alva-graph/src/pregel.rs`
- Modify: `crates/alva-graph/src/session.rs`
- Modify: `crates/alva-graph/src/lib.rs`

- [ ] **Step 1: Make StateGraph generic over state type S**

```rust
use serde::{Serialize, de::DeserializeOwned};

pub(crate) type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;
pub(crate) type NodeFn<S> =
    Box<dyn Fn(S) -> BoxFuture<S> + Send + Sync>;
pub(crate) type RouterFn<S> = Box<dyn Fn(&S) -> String + Send + Sync>;

pub(crate) enum Edge<S> {
    Direct { from: String, to: String },
    Conditional { from: String, router: RouterFn<S> },
}

pub struct StateGraph<S> {
    nodes: HashMap<String, NodeFn<S>>,
    edges: Vec<Edge<S>>,
    entry_point: Option<String>,
}

impl<S: Send + 'static> StateGraph<S> {
    pub fn new() -> Self { ... }

    pub fn add_node(
        &mut self,
        name: &str,
        node: impl Fn(S) -> BoxFuture<S> + Send + Sync + 'static,
    ) { ... }

    pub fn add_conditional_edge(
        &mut self,
        from: &str,
        router: impl Fn(&S) -> String + Send + Sync + 'static,
    ) { ... }

    pub fn compile(self) -> Result<CompiledGraph<S>, AgentError> { ... }
}
```

- [ ] **Step 2: Make CompiledGraph generic**

```rust
pub struct CompiledGraph<S> {
    pub(crate) nodes: HashMap<String, NodeFn<S>>,
    pub(crate) edges: Vec<Edge<S>>,
    pub(crate) entry_point: String,
}

impl<S: Send + 'static> CompiledGraph<S> {
    pub async fn invoke(&self, input: S) -> Result<S, AgentError> {
        let mut state = input;
        let mut current_node = self.entry_point.clone();

        loop {
            if current_node == END {
                return Ok(state);
            }
            let node_fn = self.nodes.get(&current_node).ok_or_else(|| {
                AgentError::ConfigError(format!("Node not found: {}", current_node))
            })?;
            state = node_fn(state).await;
            current_node = self.resolve_next(&current_node, &state)?;
        }
    }
}
```

- [ ] **Step 3: Update session.rs — keep serde_json::Value for now**

Session wraps both `Agent` (non-generic) and `CompiledGraph<S>`. Keep it pinned to `serde_json::Value`:

```rust
enum SessionKind {
    Linear(Agent),
    Graph(CompiledGraph<serde_json::Value>),
}
```

- [ ] **Step 4: Update lib.rs re-exports**

```rust
pub use graph::StateGraph;
pub use pregel::CompiledGraph;
```

- [ ] **Step 5: Update tests to work with generic types**

The existing tests already use `serde_json::Value`, so they should work with `StateGraph<serde_json::Value>` with minimal changes.

- [ ] **Step 6: Run tests**

Run: `cargo test -p alva-graph`

- [ ] **Step 7: Commit**

```bash
git add crates/alva-graph/
git commit -m "refactor(alva-graph): make StateGraph<S> and CompiledGraph<S> generic over state type"
```

---

## Task 5: P1 — Add parallel superstep execution to Pregel engine

**Files:**
- Modify: `crates/alva-graph/src/pregel.rs`
- Modify: `crates/alva-graph/src/graph.rs`

- [ ] **Step 1: Write test for parallel execution**

```rust
#[tokio::test]
async fn parallel_nodes_execute_concurrently() {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    let counter = Arc::new(AtomicU32::new(0));

    let mut graph = StateGraph::<serde_json::Value>::new();

    // fan-out node
    graph.add_node("fan_out", |s| Box::pin(async { s }));

    // Two parallel nodes
    let c1 = counter.clone();
    graph.add_node("parallel_a", move |s: serde_json::Value| {
        let c = c1.clone();
        Box::pin(async move {
            // Record execution order
            let order = c.fetch_add(1, Ordering::SeqCst);
            let mut s = s;
            s["a_order"] = order.into();
            s
        })
    });

    let c2 = counter.clone();
    graph.add_node("parallel_b", move |s: serde_json::Value| {
        let c = c2.clone();
        Box::pin(async move {
            let order = c.fetch_add(1, Ordering::SeqCst);
            let mut s = s;
            s["b_order"] = order.into();
            s
        })
    });

    // fan-in node
    graph.add_node("fan_in", |s| Box::pin(async { s }));

    graph.set_entry_point("fan_out");
    // fan_out -> [parallel_a, parallel_b] -> fan_in
    graph.add_edge("fan_out", "parallel_a");
    graph.add_edge("fan_out", "parallel_b");
    graph.add_edge("parallel_a", "fan_in");
    graph.add_edge("parallel_b", "fan_in");
    graph.add_edge("fan_in", END);

    let compiled = graph.compile().unwrap();
    let result = compiled.invoke(json!({})).await.unwrap();

    // Both nodes executed
    assert!(result.get("a_order").is_some());
    assert!(result.get("b_order").is_some());
}
```

- [ ] **Step 2: Replace `resolve_next` (first-match-wins) with `resolve_next_nodes` (collect ALL matching edges)**

**Critical:** The current `resolve_next()` in `pregel.rs:66-84` short-circuits on the first matching edge. For fan-out (multiple edges from same node), we need to collect ALL targets.

```rust
impl<S: Send + 'static> CompiledGraph<S> {
    /// Collect ALL next nodes from edges matching `current`.
    /// Unlike resolve_next(), this does NOT short-circuit — it returns all targets.
    fn resolve_next_nodes(&self, current: &str, state: &S) -> Result<Vec<String>, AgentError> {
        let mut targets = Vec::new();

        for edge in &self.edges {
            match edge {
                Edge::Direct { from, to } if from == current => {
                    targets.push(to.clone());
                }
                Edge::Conditional { from, router } if from == current => {
                    targets.push(router(state));
                }
                _ => continue,
            }
        }

        // No edge found — default to END
        if targets.is_empty() {
            targets.push(END.to_string());
        }

        Ok(targets)
    }
}
```

- [ ] **Step 3: Implement BSP-style parallel execution using `resolve_next_nodes`**

For state merging in parallel nodes, use a simple strategy: each parallel node receives a clone of the state, and results are merged. For `serde_json::Value`, merge JSON objects (last value wins for conflicting keys, matching LangGraph's `LastValue` channel default).

```rust
impl<S: Clone + Send + 'static> CompiledGraph<S> {
    pub async fn invoke(&self, input: S) -> Result<S, AgentError> {
        let mut state = input;
        let mut current_nodes = vec![self.entry_point.clone()];

        loop {
            // Filter out END
            current_nodes.retain(|n| n != END);
            if current_nodes.is_empty() {
                return Ok(state);
            }

            if current_nodes.len() == 1 {
                // Single node — execute directly (backward-compatible path)
                let node_name = &current_nodes[0];
                let node_fn = self.nodes.get(node_name).ok_or_else(|| {
                    AgentError::ConfigError(format!("Node not found: {}", node_name))
                })?;
                state = node_fn(state).await;
                current_nodes = self.resolve_next_nodes(&current_nodes[0], &state)?;
            } else {
                // Parallel superstep — execute all nodes concurrently via JoinSet
                // Each node receives a clone of the state.
                // Results are collected; the caller must provide a merge strategy
                // or we use a default (for serde_json::Value, merge objects).
                let mut join_set = tokio::task::JoinSet::new();
                for node_name in &current_nodes {
                    let node_fn = self.nodes.get(node_name).ok_or_else(|| {
                        AgentError::ConfigError(format!("Node not found: {}", node_name))
                    })?;
                    let s = state.clone();
                    let name = node_name.clone();
                    // Note: NodeFn is not Clone, so we need &self to be accessible.
                    // Use a different approach: collect futures, join them.
                    // Since NodeFn returns BoxFuture (which is Send), we can spawn.
                    let fut = node_fn(s);
                    join_set.spawn(async move { (name, fut.await) });
                }

                // Collect results
                let mut results = Vec::new();
                while let Some(Ok((name, result))) = join_set.join_next().await {
                    results.push((name, result));
                }

                // For now: use the last result as the state (simple merge).
                // TODO: integrate Channel system for proper state merging.
                if let Some((_, last)) = results.pop() {
                    state = last;
                }

                // Collect all next nodes from all executed nodes
                let mut next = Vec::new();
                for node in &current_nodes {
                    next.extend(self.resolve_next_nodes(node, &state)?);
                }
                next.sort();
                next.dedup();
                current_nodes = next;
            }
        }
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p alva-graph`
Expected: All existing tests pass + new parallel test passes.

- [ ] **Step 4: Commit**

```bash
git add crates/alva-graph/
git commit -m "feat(alva-graph): implement BSP parallel superstep execution in Pregel engine"
```

---

## Task 6: P2 — Make hooks composable (Vec) and async-capable

**Files:**
- Modify: `crates/alva-core/src/types.rs`
- Modify: `crates/alva-core/src/agent.rs`
- Modify: `crates/alva-core/src/agent_loop.rs`
- Modify: `crates/alva-core/src/tool_executor.rs`
- Modify: `crates/alva-core/Cargo.toml`
- Modify: `crates/alva-app/src/chat/gpui_chat.rs`

- [ ] **Step 1: Redefine hook types as async + Vec in types.rs**

```rust
use std::future::Future;
use std::pin::Pin;

/// Async hook result type.
pub type HookFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Converts agent messages into LLM-compatible messages.
/// System prompt is accessible via AgentContext, not passed as a separate parameter.
pub type ConvertToLlmFn =
    Arc<dyn Fn(&AgentContext<'_>) -> Vec<Message> + Send + Sync>;

pub type TransformContextFn =
    Arc<dyn Fn(&[AgentMessage]) -> Vec<AgentMessage> + Send + Sync>;

/// Async hook — can do I/O (database, network) for permission checks.
pub type BeforeToolCallFn =
    Arc<dyn Fn(ToolCall, AgentContext<'_>) -> HookFuture<ToolCallDecision> + Send + Sync>;

pub type AfterToolCallFn =
    Arc<dyn Fn(ToolCall, ToolResult, AgentContext<'_>) -> HookFuture<ToolResult> + Send + Sync>;

pub type GetSteeringMessagesFn =
    Arc<dyn Fn(&AgentContext<'_>) -> Vec<AgentMessage> + Send + Sync>;

pub type GetFollowUpMessagesFn =
    Arc<dyn Fn(&AgentContext<'_>) -> Vec<AgentMessage> + Send + Sync>;

pub struct AgentConfig {
    /// Required — turns agent messages into LLM messages.
    /// Context provides system_prompt, messages, and tools.
    pub convert_to_llm: ConvertToLlmFn,

    /// Optional — rewrite context before it is sent to the model.
    pub transform_context: Option<TransformContextFn>,

    /// Composable — all hooks run in order. First Block wins.
    pub before_tool_call: Vec<BeforeToolCallFn>,

    /// Composable — all hooks run in order, each receiving the previous result.
    pub after_tool_call: Vec<AfterToolCallFn>,

    /// Composable — all hooks run, messages are concatenated.
    pub get_steering_messages: Vec<GetSteeringMessagesFn>,

    /// Composable — all hooks run, messages are concatenated.
    pub get_follow_up_messages: Vec<GetFollowUpMessagesFn>,

    pub tool_execution: ToolExecutionMode,
    pub max_iterations: u32,
}

impl AgentConfig {
    pub fn new(convert_to_llm: ConvertToLlmFn) -> Self {
        Self {
            convert_to_llm,
            transform_context: None,
            before_tool_call: Vec::new(),
            after_tool_call: Vec::new(),
            get_steering_messages: Vec::new(),
            get_follow_up_messages: Vec::new(),
            tool_execution: ToolExecutionMode::Parallel,
            max_iterations: 100,
        }
    }
}
```

- [ ] **Step 2: Update AgentContext to include system_prompt**

`AgentContext` already has `system_prompt: &'a str`. Good, no change needed.

- [ ] **Step 3: Update convert_to_llm call site in agent_loop.rs**

```rust
// BEFORE (line 81-82):
let llm_messages =
    (config.convert_to_llm)(&context_messages, &state.system_prompt);

// AFTER:
let convert_ctx = AgentContext {
    system_prompt: &state.system_prompt,
    messages: &context_messages,
    tools: &state.tools,
};
let llm_messages = (config.convert_to_llm)(&convert_ctx);
```

- [ ] **Step 4: Update before_tool_call in tool_executor.rs to run Vec of async hooks**

```rust
// Run all before_tool_call hooks. First Block wins.
let mut decision = ToolCallDecision::Allow;
for hook in &config.before_tool_call {
    let ctx = AgentContext {
        system_prompt: context.system_prompt,
        messages: context.messages,
        tools: context.tools,
    };
    decision = hook(tc.clone(), ctx).await;
    if matches!(decision, ToolCallDecision::Block { .. }) {
        break;
    }
}
```

- [ ] **Step 5: Update after_tool_call to chain Vec of async hooks**

```rust
// Run all after_tool_call hooks in sequence, piping result through.
for hook in &config.after_tool_call {
    let ctx = AgentContext {
        system_prompt: context.system_prompt,
        messages: context.messages,
        tools: context.tools,
    };
    result = hook(tc.clone(), result, ctx).await;
}
```

- [ ] **Step 6: Update steering/follow_up hooks to concat from Vec**

```rust
// Steering: collect from all hooks
let mut steering = Vec::new();
for hook in &config.get_steering_messages {
    let ctx = AgentContext { ... };
    steering.extend(hook(&ctx));
}
```

- [ ] **Step 7: Update agent.rs — steering/follow_up channel injection now pushes to Vec**

```rust
// In agent.rs prompt() method:
cfg.get_steering_messages.push(Arc::new(move |_ctx| {
    match steering_rx_clone.try_lock() {
        Ok(mut rx) => {
            let mut msgs = Vec::new();
            while let Ok(batch) = rx.try_recv() {
                msgs.extend(batch);
            }
            msgs
        }
        Err(_) => Vec::new(),
    }
}));
```

- [ ] **Step 8: Update alva-app/gpui_chat.rs — adapt convert_to_llm signature**

```rust
// BEFORE (line 119):
let agent_config = AgentConfig::new(Arc::new(|messages, system_prompt| {
    let mut result = vec![Message::system(system_prompt)];
    ...
}));

// AFTER:
let agent_config = AgentConfig::new(Arc::new(|ctx: &AgentContext<'_>| {
    let mut result = vec![Message::system(ctx.system_prompt)];
    for m in ctx.messages {
        if let AgentMessage::Standard(msg) = m {
            result.push(msg.clone());
        }
    }
    result
}));
```

- [ ] **Step 9: Update test helpers in agent_loop.rs**

The test helper `default_convert_to_llm` at `agent_loop.rs:277-287` has the old signature `(&[AgentMessage], &str) -> Vec<Message>`. Update to match the new `ConvertToLlmFn` type:

```rust
// BEFORE:
fn default_convert_to_llm(messages: &[AgentMessage], system_prompt: &str) -> Vec<Message> {
    let mut result = vec![Message::system(system_prompt)];
    for m in messages {
        if let AgentMessage::Standard(msg) = m {
            result.push(msg.clone());
        }
    }
    result
}

// AFTER:
fn default_convert_to_llm(ctx: &AgentContext<'_>) -> Vec<Message> {
    let mut result = vec![Message::system(ctx.system_prompt)];
    for m in ctx.messages {
        if let AgentMessage::Standard(msg) = m {
            result.push(msg.clone());
        }
    }
    result
}
```

Both test functions at `agent_loop.rs:295` and `agent_loop.rs:340` use `Arc::new(default_convert_to_llm)` — the signature change ensures they compile.

- [ ] **Step 10: Run `cargo check` then `cargo test -p alva-core`**

- [ ] **Step 10: Commit**

```bash
git add crates/alva-core/ crates/alva-app/
git commit -m "refactor(alva-core): make hooks composable (Vec) and async-capable"
```

---

## Task 7: P3 — Add streaming support to agent loop

**Files:**
- Modify: `crates/alva-core/src/agent_loop.rs`
- Modify: `crates/alva-core/src/agent.rs`
- Modify: `crates/alva-core/src/types.rs`

- [ ] **Step 1: Add `use_streaming` flag to AgentState**

```rust
pub struct AgentState {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub is_streaming: bool,   // already exists
    pub model_config: ModelConfig,
}
```

The `is_streaming` field already exists. We'll use it to choose between `complete()` and `stream()`.

- [ ] **Step 2: Add stream-based LLM call path in agent_loop.rs**

After the existing `complete()` call (line 96-98), add a branch:

```rust
let assistant_message = if state.is_streaming {
    // Use streaming path
    stream_llm_response(model, &llm_messages, &tool_refs, &state.model_config, event_tx).await?
} else {
    // Use complete path (existing code)
    model.complete(&llm_messages, &tool_refs, &state.model_config).await?
};
```

- [ ] **Step 3: Implement `stream_llm_response` helper**

```rust
async fn stream_llm_response(
    model: &dyn LanguageModel,
    messages: &[Message],
    tools: &[&dyn alva_types::Tool],
    config: &ModelConfig,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<Message, AgentError> {
    use tokio_stream::StreamExt;  // NOT futures_core — StreamExt is in tokio-stream or futures-util

    let mut stream = model.stream(messages, tools, config);

    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();
    let mut usage = None;

    // Emit MessageStart with a placeholder
    // We'll build the final message from deltas.

    while let Some(event) = stream.next().await {
        match &event {
            StreamEvent::TextDelta { text: delta } => {
                text.push_str(delta);
            }
            StreamEvent::ReasoningDelta { text: delta } => {
                reasoning.push_str(delta);
            }
            StreamEvent::ToolCallDelta { id, name, arguments_delta } => {
                // Accumulate tool call arguments
                if let Some(tc) = tool_calls.iter_mut().find(|tc: &&mut ToolCallAccumulator| tc.id == *id) {
                    tc.arguments_json.push_str(arguments_delta);
                    if let Some(n) = name {
                        tc.name = n.clone();
                    }
                } else {
                    tool_calls.push(ToolCallAccumulator {
                        id: id.clone(),
                        name: name.clone().unwrap_or_default(),
                        arguments_json: arguments_delta.clone(),
                    });
                }
            }
            StreamEvent::Usage(u) => {
                usage = Some(u.clone());
            }
            StreamEvent::Error(e) => {
                return Err(AgentError::LlmError(e.clone()));
            }
            _ => {}
        }

        // Emit MessageUpdate delta
        // (build a partial AgentMessage for the event)
        let _ = event_tx.send(AgentEvent::MessageUpdate {
            message: AgentMessage::Standard(Message::user("")), // placeholder
            delta: event,
        });
    }

    // Build final message from accumulated deltas
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(ContentBlock::Text { text });
    }
    if !reasoning.is_empty() {
        content.push(ContentBlock::Reasoning { text: reasoning });
    }
    for tc in &tool_calls {
        let input: serde_json::Value = serde_json::from_str(&tc.arguments_json)
            .unwrap_or(serde_json::Value::String(tc.arguments_json.clone()));
        content.push(ContentBlock::ToolUse {
            id: tc.id.clone(),
            name: tc.name.clone(),
            input,
        });
    }

    Ok(Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: MessageRole::Assistant,
        content,
        tool_call_id: None,
        usage,
        timestamp: chrono::Utc::now().timestamp_millis(),
    })
}

struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments_json: String,
}
```

- [ ] **Step 4: Add `set_streaming` method to Agent**

```rust
impl Agent {
    pub async fn set_streaming(&self, streaming: bool) {
        let mut st = self.state.lock().await;
        st.is_streaming = streaming;
    }
}
```

- [ ] **Step 5: Add futures dependency if needed**

```toml
# In crates/alva-core/Cargo.toml, ensure:
futures-core = "0.3"
tokio-stream = "0.1"   # for StreamExt on Pin<Box<dyn Stream>>
```

- [ ] **Step 6: Write test for streaming path**

```rust
#[tokio::test]
async fn test_streaming_text_response() {
    struct StreamingMockModel;

    #[async_trait]
    impl LanguageModel for StreamingMockModel {
        async fn complete(...) -> Result<Message, AgentError> {
            unimplemented!("should use stream path")
        }

        fn stream(...) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
            Box::pin(futures::stream::iter(vec![
                StreamEvent::Start,
                StreamEvent::TextDelta { text: "Hello ".into() },
                StreamEvent::TextDelta { text: "world!".into() },
                StreamEvent::Done,
            ]))
        }

        fn model_id(&self) -> &str { "streaming-mock" }
    }

    let config = AgentConfig::new(Arc::new(default_convert_to_llm));
    let cancel = CancellationToken::new();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    let mut state = AgentState::new("test".into(), ModelConfig::default());
    state.is_streaming = true;
    state.messages.push(AgentMessage::Standard(Message::user("Hi")));

    let result = run_agent_loop(&mut state, &StreamingMockModel, &config, &cancel, &event_tx).await;
    assert!(result.is_ok());

    // Verify we got MessageUpdate events
    drop(event_tx);
    let mut got_update = false;
    while let Some(ev) = event_rx.recv().await {
        if matches!(ev, AgentEvent::MessageUpdate { .. }) {
            got_update = true;
        }
    }
    assert!(got_update, "should have received MessageUpdate events");
}
```

- [ ] **Step 7: Run tests**

Run: `cargo test -p alva-core`

- [ ] **Step 8: Commit**

```bash
git add crates/alva-core/
git commit -m "feat(alva-core): add streaming support to agent loop via model.stream()"
```

---

## Task 8: P3 — Fix ToolResult.tool_call_id in MCP adapter

**Files:**
- Modify: `crates/alva-types/src/tool.rs`
- Modify: `crates/alva-mcp/src/tool_adapter.rs`
- Modify: `crates/alva-core/src/tool_executor.rs`

- [ ] **Step 1: Remove `tool_call_id` from ToolResult struct**

The `tool_call_id` should be set by the engine (tool_executor.rs), not by the Tool implementation. This removes the leaky abstraction.

```rust
// BEFORE in tool.rs:
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
    pub details: Option<serde_json::Value>,
}

// AFTER:
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    pub details: Option<serde_json::Value>,
}
```

- [ ] **Step 2: Update tool_executor.rs to set tool_call_id when building the tool message**

In `agent_loop.rs` where tool results are pushed as messages (lines 148-165), the `tool_call_id` is already taken from `result.tool_call_id`. Change this to use `tc.id` from the ToolCall:

```rust
// In agent_loop.rs, the tool result message construction already has access to
// the ToolCall. Use tc.id instead of result.tool_call_id:
for (tc, result) in tool_calls.iter().zip(results.iter()) {
    let tool_msg = Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: MessageRole::Tool,
        content: vec![ContentBlock::ToolResult {
            id: tc.id.clone(),      // from the ToolCall, not from ToolResult
            content: result.content.clone(),
            is_error: result.is_error,
        }],
        tool_call_id: Some(tc.id.clone()),
        usage: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
    };
    state.messages.push(AgentMessage::Standard(tool_msg));
}
```

- [ ] **Step 3: Update tool_executor.rs — remove tool_call_id from error ToolResults**

All places that construct `ToolResult { tool_call_id: ... }` need updating. The id is no longer part of the struct.

- [ ] **Step 4: Update MCP tool_adapter.rs — remove empty tool_call_id**

```rust
// BEFORE (line 79-84):
Ok(ToolResult {
    tool_call_id: String::new(), // Filled by engine layer
    content: output,
    is_error: false,
    details: None,
})

// AFTER:
Ok(ToolResult {
    content: output,
    is_error: false,
    details: None,
})
```

- [ ] **Step 5: Update AgentEvent::ToolExecutionEnd to carry tool_call_id separately**

The event already has `tool_call: ToolCall` which contains the id. No change needed.

- [ ] **Step 6: Fix all `alva_types::ToolResult` constructions across the workspace**

Run: `grep -rn "ToolResult {" crates/` — but note: `alva-app-core` has its OWN `ToolResult` struct at `crates/alva-app-core/src/domain/tool.rs` with different fields (`tool_name`, `output`, `duration_ms`). Only update `alva_types::ToolResult` usages:
- `crates/alva-core/src/tool_executor.rs` — 7 constructions
- `crates/alva-mcp/src/tool_adapter.rs` — 1 construction
Do NOT touch `alva-app-core`'s separate `ToolResult` type.

- [ ] **Step 7: Run `cargo check` then `cargo test`**

- [ ] **Step 8: Commit**

```bash
git add crates/alva-types/ crates/alva-core/ crates/alva-mcp/
git commit -m "refactor(alva-types): remove tool_call_id from ToolResult — engine sets it"
```

---

## Execution Order

Tasks should be executed in this order to minimize rework:

1. **Task 8** (P3 — ToolResult.tool_call_id) — smallest, least dependencies
2. **Task 3** (P1 — Unify ToolCall) — changes Message struct, affects everything
3. **Task 1** (P0 — MCP config path) — isolated to alva-mcp + alva-app-core/mcp
4. **Task 2** (P0 — ACP paths + alva-app-core dedup) — alva-acp + alva-app-core duplicate cleanup
5. **Task 6** (P2 — Async composable hooks) — major API change to alva-core
6. **Task 7** (P3 — Streaming) — builds on new hook API
7. **Task 4** (P1 — Generic StateGraph) — isolated to alva-graph
8. **Task 5** (P1 — Parallel Pregel) — builds on generic StateGraph

Tasks 1+2 can run in parallel (independent protocol crates). Tasks 4+5 can run after 1-3 are done. Task 7 depends on Task 6.

**Note:** Task 2 includes cleaning up alva-app-core's duplicate ExternalAgentKind/AgentDiscovery code. This is the most impactful P0 change — ensure alva-app-core re-exports from alva-acp rather than maintaining parallel copies.

Tasks 1+2 can run in parallel. Tasks 4+5 can run after 1-3 are done. Task 7 depends on Task 6.
