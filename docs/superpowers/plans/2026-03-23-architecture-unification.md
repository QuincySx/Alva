# Architecture Unification Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Unify the Tool trait system, move Provider trait to agent-types, and make srow-core a proper Facade so srow-app only depends on srow-core (+ srow-debug).

**Architecture:** Three sequential phases — (1) unify the two Tool traits into a single `agent_types::Tool`, (2) move Provider/ProviderError to agent-types, (3) make srow-core re-export agent-core types so srow-app can drop direct agent-core/agent-graph/agent-types dependencies. Each phase compiles independently before the next begins.

**Tech Stack:** Rust, async-trait, serde, thiserror, tokio

---

## File Structure

### Phase 1: Tool Unification (P0)

**Modified files:**
- `crates/agent-types/src/tool.rs` — Add `ToolContext` trait + `ToolDefinition`, expand `ToolResult` fields
- `crates/agent-types/src/lib.rs` — Re-export new types
- `crates/agent-core/src/tool_executor.rs` — Pass `ToolContext` to execute()
- `crates/agent-core/src/types.rs` — Hold `Arc<dyn ToolContext>` in AgentState
- `crates/srow-core/src/ports/tool.rs` — Delete old Tool/ToolRegistry, implement ToolContext
- `crates/srow-core/src/domain/tool.rs` — Delete duplicate ToolCall/ToolResult/ToolDefinition, re-export from agent-types
- `crates/srow-core/src/lib.rs` — Update re-exports
- `crates/srow-core/src/agent/runtime/tools/execute_shell.rs` — Migrate to agent_types::Tool
- `crates/srow-core/src/agent/runtime/tools/create_file.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/file_edit.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/grep_search.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/list_files.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/ask_human.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/internet_search.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/read_url.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/view_image.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/browser/browser_start.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/browser/browser_stop.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/browser/browser_navigate.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/browser/browser_action.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/browser/browser_snapshot.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/browser/browser_screenshot.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/browser/browser_status.rs` — Migrate
- `crates/srow-core/src/agent/runtime/tools/mod.rs` — Use agent_types::ToolRegistry
- `crates/srow-core/src/mcp/tool_adapter.rs` — Migrate to agent_types::Tool
- `crates/srow-core/src/mcp/tools.rs` — Migrate McpRuntimeTool
- `crates/srow-core/src/skills/tools.rs` — Migrate SearchSkillsTool, UseSkillTool

### Phase 2: Provider Migration (P1)

**New files:**
- `crates/agent-types/src/provider.rs` — Provider trait + ProviderError

**Modified files:**
- `crates/agent-types/src/lib.rs` — Export Provider, ProviderError, ProviderRegistry
- `crates/srow-core/src/ports/provider/provider_registry.rs` — Delete trait/struct, re-export from agent-types
- `crates/srow-core/src/ports/provider/errors.rs` — Delete, moved to agent-types
- `crates/srow-core/src/ports/provider/mod.rs` — Re-export from agent-types
- `crates/srow-core/src/lib.rs` — Update Provider re-export path

### Phase 3: Facade Pattern (P2)

**Modified files:**
- `crates/srow-core/Cargo.toml` — Add agent-core dependency
- `crates/srow-core/src/lib.rs` — Re-export agent-core and agent-types types
- `crates/srow-app/Cargo.toml` — Remove agent-types, agent-core, agent-graph deps
- `crates/srow-app/src/chat/gpui_chat.rs` — Import from srow_core instead
- `crates/srow-app/src/views/chat_panel/message_list.rs` — Import from srow_core instead

---

## Task 1: Expand agent_types::Tool with ToolContext and ToolDefinition

This is the foundation change — we make `agent_types::Tool` capable of receiving runtime context, so srow-core tools can implement it directly.

**Design decision:** We add a `ToolContext` trait to agent-types (not a concrete struct) so it remains generic. agent-core's tool_executor will accept `Option<Arc<dyn ToolContext>>`. srow-core will provide its own concrete `SrowToolContext` implementing this trait.

We also merge the `ToolResult` and `ToolDefinition` types so there's only one canonical version.

**Files:**
- Modify: `crates/agent-types/src/tool.rs`
- Modify: `crates/agent-types/src/lib.rs`

- [ ] **Step 1: Update agent_types::Tool with ToolContext trait and expanded types**

In `crates/agent-types/src/tool.rs`, replace the current content with:

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::cancel::CancellationToken;
use crate::error::AgentError;

// ---------------------------------------------------------------------------
// ToolDefinition — JSON Schema description for LLM function calling
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema object describing parameters
    pub parameters: serde_json::Value,
}

// ---------------------------------------------------------------------------
// ToolCall / ToolResult — wire types flowing through the agent loop
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

// ---------------------------------------------------------------------------
// ToolContext — runtime context injected into tool execution
// ---------------------------------------------------------------------------

/// Runtime context available to tools during execution.
///
/// This is a trait (not a concrete struct) so that agent-types stays generic.
/// Each application layer provides its own implementation.
/// Tools that don't need context can ignore the parameter.
pub trait ToolContext: Send + Sync {
    /// Current workspace / project root path.
    fn workspace(&self) -> &Path;

    /// Current session identifier.
    fn session_id(&self) -> &str;

    /// Whether the tool is allowed to perform dangerous operations.
    fn allow_dangerous(&self) -> bool;
}

/// No-op context for tools that don't need runtime information.
pub struct EmptyToolContext;

impl ToolContext for EmptyToolContext {
    fn workspace(&self) -> &Path {
        Path::new(".")
    }
    fn session_id(&self) -> &str {
        ""
    }
    fn allow_dangerous(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Tool trait — the single canonical tool abstraction
// ---------------------------------------------------------------------------

#[async_trait]
pub trait Tool: Send + Sync {
    /// Tool name (must match ToolCall.name from LLM).
    fn name(&self) -> &str;

    /// Human-readable description for the LLM.
    fn description(&self) -> &str;

    /// JSON Schema for parameters.
    fn parameters_schema(&self) -> serde_json::Value;

    /// Full definition for LLM function calling (convenience).
    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters_schema(),
        }
    }

    /// Execute the tool.
    ///
    /// Both `cancel` and `ctx` are provided. Tools that don't need runtime
    /// context can ignore `ctx`. Tools that don't need cancellation can
    /// ignore `cancel`.
    async fn execute(
        &self,
        input: serde_json::Value,
        cancel: &CancellationToken,
        ctx: &dyn ToolContext,
    ) -> Result<ToolResult, AgentError>;
}

// ---------------------------------------------------------------------------
// ToolRegistry — name → Tool lookup
// ---------------------------------------------------------------------------

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn list(&self) -> Vec<&dyn Tool> {
        self.tools.values().map(|t| t.as_ref()).collect()
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition()).collect()
    }

    pub fn remove(&mut self, name: &str) -> Option<Box<dyn Tool>> {
        self.tools.remove(name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 2: Update agent_types lib.rs re-exports**

In `crates/agent-types/src/lib.rs`, update the tool re-export line:

```rust
pub use tool::{EmptyToolContext, Tool, ToolCall, ToolContext, ToolDefinition, ToolRegistry, ToolResult};
```

- [ ] **Step 3: Verify agent-types compiles**

Run: `cargo check -p agent-types`
Expected: PASS (no other crates depend on old signatures yet — we haven't changed them)

- [ ] **Step 4: Commit**

```bash
git add crates/agent-types/src/tool.rs crates/agent-types/src/lib.rs
git commit -m "feat(agent-types): unify Tool trait with ToolContext, ToolDefinition, expanded API"
```

---

## Task 2: Update agent-core to pass ToolContext through execute()

agent-core's tool_executor currently calls `tool.execute(args, &cancel)`. We need to update it to `tool.execute(args, &cancel, ctx)`.

**Files:**
- Modify: `crates/agent-core/src/types.rs`
- Modify: `crates/agent-core/src/tool_executor.rs`
- Modify: `crates/agent-core/src/agent.rs` (if it constructs AgentState)
- Modify: `crates/agent-core/src/agent_loop.rs` (if it passes tools)

- [ ] **Step 1: Add tool_context to AgentState**

In `crates/agent-core/src/types.rs`, add an import and field:

Add import at top:
```rust
use agent_types::ToolContext;
```

Add field to `AgentState`:
```rust
pub struct AgentState {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub is_streaming: bool,
    pub model_config: ModelConfig,
    pub tool_context: Arc<dyn ToolContext>,  // NEW
}
```

Update `AgentState::new()`:
```rust
impl AgentState {
    pub fn new(system_prompt: String, model_config: ModelConfig) -> Self {
        Self {
            system_prompt,
            messages: Vec::new(),
            tools: Vec::new(),
            is_streaming: false,
            model_config,
            tool_context: Arc::new(agent_types::EmptyToolContext),
        }
    }

    /// Create state with a custom tool context.
    pub fn with_tool_context(
        system_prompt: String,
        model_config: ModelConfig,
        tool_context: Arc<dyn ToolContext>,
    ) -> Self {
        Self {
            system_prompt,
            messages: Vec::new(),
            tools: Vec::new(),
            is_streaming: false,
            model_config,
            tool_context,
        }
    }
}
```

- [ ] **Step 2: Update tool_executor to pass ToolContext**

In `crates/agent-core/src/tool_executor.rs`:

Update the function signature to accept `tool_context`:

```rust
pub(crate) async fn execute_tools(
    tool_calls: &[ToolCall],
    tools: &[Arc<dyn Tool>],
    config: &AgentConfig,
    context: &AgentContext<'_>,
    cancel: &CancellationToken,
    tool_context: &Arc<dyn agent_types::ToolContext>,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Vec<ToolResult> {
```

In `execute_parallel()`, update the spawn block's tool invocation from:
```rust
t.execute(tc_clone.arguments.clone(), &cancel_clone).await
```
to:
```rust
t.execute(tc_clone.arguments.clone(), &cancel_clone, tool_ctx.as_ref()).await
```

(Clone `tool_context` into the spawn as `tool_ctx`.)

Similarly in `execute_sequential()`, update:
```rust
t.execute(tc.arguments.clone(), cancel).await
```
to:
```rust
t.execute(tc.arguments.clone(), cancel, tool_context.as_ref()).await
```

- [ ] **Step 3: Update agent_loop.rs to pass tool_context from AgentState**

Find where `execute_tools` is called in `agent_loop.rs` and add `&state.tool_context` as the new parameter.

- [ ] **Step 4: Verify agent-core compiles**

Run: `cargo check -p agent-core`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/agent-core/
git commit -m "feat(agent-core): pass ToolContext through tool executor"
```

---

## Task 3: Update protocol-model-context McpToolAdapter

The MCP tool adapter in protocol-model-context already implements `agent_types::Tool`. We need to update its `execute()` signature to match the new 3-argument version.

**Files:**
- Modify: `crates/protocol-model-context/src/tool_adapter.rs`

- [ ] **Step 1: Update McpToolAdapter::execute signature**

Change:
```rust
async fn execute(
    &self,
    input: Value,
    _cancel: &CancellationToken,
) -> Result<ToolResult, AgentError>
```

To:
```rust
async fn execute(
    &self,
    input: Value,
    _cancel: &CancellationToken,
    _ctx: &dyn agent_types::ToolContext,
) -> Result<ToolResult, AgentError>
```

Also add `description()` and `parameters_schema()` methods if not present, and remove any now-redundant methods.

- [ ] **Step 2: Verify protocol-model-context compiles**

Run: `cargo check -p protocol-model-context`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add crates/protocol-model-context/src/tool_adapter.rs
git commit -m "refactor(protocol-model-context): update McpToolAdapter to new Tool trait"
```

---

## Task 4: Migrate srow-core domain tool types (NO COMMIT — part of atomic batch)

Replace `srow_core::domain::tool::{ToolCall, ToolResult, ToolDefinition}` with re-exports from agent-types.

**IMPORTANT:** This task deliberately breaks compilation. Do NOT commit until Task 7 completes and the workspace compiles. Tasks 4-7 form a single atomic batch with one commit at the end.

**Breaking changes to document:**
- `ToolResult` fields: `{tool_call_id, tool_name, output, is_error, duration_ms}` → `{content, is_error, details}`
- `ToolCall` field: `.input` → `.arguments` (field rename)
- `ToolDefinition` stays the same shape

**Files:**
- Modify: `crates/srow-core/src/domain/tool.rs`
- Modify: `crates/srow-core/src/types/llm.rs` (re-exports domain types — shape changes)

- [ ] **Step 1: Replace domain/tool.rs with re-exports**

Replace the entire content of `crates/srow-core/src/domain/tool.rs` with:

```rust
// Re-export canonical tool types from agent-types.
// All crates now use a single ToolCall, ToolResult, ToolDefinition.
//
// Breaking field changes:
//   ToolResult: {tool_call_id, tool_name, output, ..} → {content, is_error, details}
//   ToolCall:   .input → .arguments
pub use agent_types::{ToolCall, ToolDefinition, ToolResult};
```

- [ ] **Step 2: Verify types/llm.rs compiles (it re-exports domain types)**

`crates/srow-core/src/types/llm.rs` contains:
```rust
pub use crate::domain::tool::{ToolCall, ToolDefinition, ToolResult};
```
This will still compile since domain::tool now re-exports from agent-types. No change needed, but note that downstream consumers of `srow_core::types::llm::ToolCall` will see the `.input` → `.arguments` rename.

- [ ] **Step 3: DO NOT COMMIT — proceed directly to Task 5**

The workspace is intentionally broken at this point. Continue to Tasks 5-7 before committing.

---

## Task 5: Delete srow_core::ports::Tool, create SrowToolContext, update SecurityGuard (NO COMMIT)

Remove the duplicate `Tool` trait and `ToolRegistry` from srow-core ports. Replace with a concrete `SrowToolContext` implementing `agent_types::ToolContext`. Also update `SecurityGuard` which uses the old concrete `ToolContext` struct.

**Files:**
- Modify: `crates/srow-core/src/ports/tool.rs`
- Modify: `crates/srow-core/src/lib.rs`
- Modify: `crates/srow-core/src/agent/runtime/security/guard.rs`

- [ ] **Step 1: Replace ports/tool.rs with SrowToolContext**

Replace the entire content of `crates/srow-core/src/ports/tool.rs` with:

```rust
// Re-export the canonical Tool trait and ToolRegistry from agent-types.
pub use agent_types::{Tool, ToolContext, ToolDefinition, ToolRegistry, ToolResult};

/// Concrete runtime context for srow-core tool execution.
///
/// Implements `agent_types::ToolContext` so that srow-core tools can
/// access workspace path, session ID, and security flags.
#[derive(Debug, Clone)]
pub struct SrowToolContext {
    pub session_id: String,
    pub workspace: std::path::PathBuf,
    pub allow_dangerous: bool,
}

impl agent_types::ToolContext for SrowToolContext {
    fn workspace(&self) -> &std::path::Path {
        &self.workspace
    }

    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn allow_dangerous(&self) -> bool {
        self.allow_dangerous
    }
}
```

- [ ] **Step 2: Update srow-core lib.rs re-exports**

In `crates/srow-core/src/lib.rs`, change:
```rust
pub use ports::tool::{Tool, ToolContext, ToolRegistry};
```
to:
```rust
pub use ports::tool::{SrowToolContext, Tool, ToolContext, ToolRegistry};
```

- [ ] **Step 3: Update SecurityGuard to use trait instead of concrete struct**

In `crates/srow-core/src/agent/runtime/security/guard.rs`:

Change the import:
```rust
// OLD
use crate::ports::tool::ToolContext;
// NEW
use crate::ports::tool::{SrowToolContext, ToolContext};
```

Change `check_tool_call` signature:
```rust
// OLD
pub fn check_tool_call(&mut self, tool_name: &str, args: &Value, _ctx: &ToolContext) -> SecurityDecision {
// NEW
pub fn check_tool_call(&mut self, tool_name: &str, args: &Value, _ctx: &dyn ToolContext) -> SecurityDecision {
```

Update the test helper function:
```rust
// OLD
fn test_ctx() -> ToolContext {
    ToolContext {
        session_id: "test-session".to_string(),
        workspace: PathBuf::from("/projects/myapp"),
        allow_dangerous: false,
    }
}
// NEW
fn test_ctx() -> SrowToolContext {
    SrowToolContext {
        session_id: "test-session".to_string(),
        workspace: PathBuf::from("/projects/myapp"),
        allow_dangerous: false,
    }
}
```

- [ ] **Step 4: DO NOT COMMIT — proceed to Task 6**

---

## Task 6: Migrate all 9 standard tool implementations

Each tool currently implements `srow_core::ports::tool::Tool` with signature `execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, EngineError>`. We migrate to `agent_types::Tool` with signature `execute(&self, input: Value, cancel: &CancellationToken, ctx: &dyn ToolContext) -> Result<ToolResult, AgentError>`.

**Key changes per tool:**
1. Replace `use crate::ports::tool::{Tool, ToolContext}` → `use agent_types::{Tool, ToolContext, ToolResult, ToolDefinition, CancellationToken, AgentError}`
2. Remove `use crate::domain::tool::{ToolDefinition, ToolResult}`
3. Remove `use crate::error::EngineError`
4. Add `fn description()` and `fn parameters_schema()` methods
5. Change `fn definition()` from required → uses default impl (or remove override)
6. Change `execute()` signature: add `cancel: &CancellationToken` param, change `ctx: &ToolContext` → `ctx: &dyn ToolContext`, return `Result<ToolResult, AgentError>`
7. Change ToolResult construction: `{output, tool_call_id, tool_name, duration_ms, is_error}` → `{content, is_error, details}`
8. Change error type: `EngineError::ToolExecution(msg)` → `AgentError::ToolError { tool_name, message }`

**Template for each tool migration:**

```rust
// OLD:
use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};

#[async_trait]
impl Tool for FooTool {
    fn name(&self) -> &str { "foo" }
    fn definition(&self) -> ToolDefinition { ToolDefinition { name, description, parameters } }
    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        // ... uses ctx.workspace, ctx.session_id, ctx.allow_dangerous
        Ok(ToolResult { tool_call_id: String::new(), tool_name: "foo".into(), output: "...", is_error: false, duration_ms })
    }
}

// NEW:
use agent_types::{AgentError, CancellationToken, Tool, ToolContext, ToolDefinition, ToolResult};

#[async_trait]
impl Tool for FooTool {
    fn name(&self) -> &str { "foo" }
    fn description(&self) -> &str { "..." }
    fn parameters_schema(&self) -> serde_json::Value { json!({...}) }
    // definition() uses the default impl from the trait — no override needed

    async fn execute(
        &self,
        input: Value,
        _cancel: &CancellationToken,
        ctx: &dyn ToolContext,
    ) -> Result<ToolResult, AgentError> {
        // ... uses ctx.workspace(), ctx.session_id(), ctx.allow_dangerous()
        Ok(ToolResult { content: "...", is_error: false, details: None })
    }
}
```

**Files (9 standard tools):**
- Modify: `crates/srow-core/src/agent/runtime/tools/execute_shell.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/create_file.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/file_edit.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/grep_search.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/list_files.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/ask_human.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/internet_search.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/read_url.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/view_image.rs`

- [ ] **Step 1: Migrate execute_shell.rs**

Apply the template above. Key change: `ctx.workspace` → `ctx.workspace()` (method call, not field). Error mapping: `EngineError::ToolExecution(msg)` → `AgentError::ToolError { tool_name: "execute_shell".into(), message: msg }`.

Extract `description` and `parameters` from the old `definition()` into separate `description()` and `parameters_schema()` methods.

- [ ] **Step 2: Migrate create_file.rs**

Same pattern as Step 1.

- [ ] **Step 3: Migrate file_edit.rs**

Same pattern.

- [ ] **Step 4: Migrate grep_search.rs**

Same pattern.

- [ ] **Step 5: Migrate list_files.rs**

Same pattern.

- [ ] **Step 6: Migrate ask_human.rs**

Same pattern.

- [ ] **Step 7: Migrate internet_search.rs**

Same pattern. Note: this tool uses `reqwest` — error mapping may differ.

- [ ] **Step 8: Migrate read_url.rs**

Same pattern.

- [ ] **Step 9: Migrate view_image.rs**

Same pattern.

- [ ] **Step 10: Update tools/mod.rs registry**

In `crates/srow-core/src/agent/runtime/tools/mod.rs`, change:
```rust
use crate::ports::tool::ToolRegistry;
```
to:
```rust
use agent_types::ToolRegistry;
```

(This should work because `ports::tool` now re-exports `agent_types::ToolRegistry`, but using the canonical path is cleaner.)

- [ ] **Step 11: Verify standard tools compile**

Run: `cargo check -p srow-core 2>&1 | grep "error" | head -20`
Expected: Only browser tool and MCP adapter errors remain (migrated in next tasks).

- [ ] **Step 12: DO NOT COMMIT — proceed to Task 7**

Tasks 4-7 form an atomic batch. The commit happens at the end of Task 7.

---

## Task 7: Migrate browser tools, MCP adapters, skill tools, delegate tool — ATOMIC COMMIT for Tasks 4-7

Same migration pattern as Task 6, applied to the remaining tool implementations. Also includes `AcpDelegateTool` which was missed in earlier analysis.

**Files:**
- Modify: `crates/srow-core/src/agent/runtime/tools/browser/browser_start.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/browser/browser_stop.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/browser/browser_navigate.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/browser/browser_action.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/browser/browser_snapshot.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/browser/browser_screenshot.rs`
- Modify: `crates/srow-core/src/agent/runtime/tools/browser/browser_status.rs`
- Modify: `crates/srow-core/src/mcp/tool_adapter.rs`
- Modify: `crates/srow-core/src/mcp/tools.rs`
- Modify: `crates/srow-core/src/skills/tools.rs` (SearchSkillsTool, UseSkillTool)
- Modify: `crates/srow-core/src/agent/agent_client/delegate.rs` (AcpDelegateTool)

- [ ] **Step 1: Migrate 7 browser tools**

Apply the same template from Task 6 to all 7 browser tool files. These tools have a `manager: SharedBrowserManager` field — the migration only changes the trait impl, not the Chrome CDP logic.

- [ ] **Step 2: Migrate srow-core MCP tool_adapter**

In `crates/srow-core/src/mcp/tool_adapter.rs`, change the imports and trait impl:

```rust
// OLD
use crate::domain::tool::ToolResult;
use crate::error::EngineError;
use crate::domain::tool::ToolDefinition;
use crate::ports::tool::{Tool, ToolContext};

// NEW
use agent_types::{AgentError, CancellationToken, Tool, ToolContext, ToolDefinition, ToolResult};
```

Update `execute()` signature to include `_cancel: &CancellationToken, _ctx: &dyn ToolContext`.
Update `ToolResult` construction to use `{content, is_error, details}`.

- [ ] **Step 3: Migrate McpRuntimeTool (mcp/tools.rs)**

Same pattern.

- [ ] **Step 4: Migrate skill tools (skills/tools.rs)**

Migrate `SearchSkillsTool` and `UseSkillTool` to `agent_types::Tool`.

- [ ] **Step 5: Migrate AcpDelegateTool (agent_client/delegate.rs)**

In `crates/srow-core/src/agent/agent_client/delegate.rs`:

Change imports:
```rust
// OLD
use crate::{
    domain::tool::{ToolDefinition, ToolResult},
    error::EngineError,
    ports::tool::{Tool, ToolContext},
};

// NEW
use agent_types::{AgentError, CancellationToken, Tool, ToolContext, ToolDefinition, ToolResult};
```

Update `execute()` signature:
```rust
// OLD
async fn execute(&self, input: serde_json::Value, ctx: &ToolContext) -> Result<ToolResult, EngineError>
// NEW
async fn execute(&self, input: serde_json::Value, _cancel: &CancellationToken, ctx: &dyn ToolContext) -> Result<ToolResult, AgentError>
```

Update field access: `ctx.workspace` → `ctx.workspace().to_path_buf()`.
Update error type: `EngineError::ToolExecution(msg)` → `AgentError::ToolError { tool_name: self.tool_name.clone(), message: msg }`.
Update ToolResult: `{tool_call_id, tool_name, output, is_error, duration_ms}` → `{content: output, is_error, details: None}`.

Add `description()` and `parameters_schema()` methods extracted from the old `definition()`.

Note: `AgentDelegate` trait's own methods still return `EngineError` — only the `Tool` impl changes to `AgentError`. The `?` operator in `execute()` needs a `From<EngineError> for AgentError` conversion OR explicit `.map_err()`.

- [ ] **Step 6: Verify full srow-core compiles**

Run: `cargo check -p srow-core`
Expected: PASS — all tools now implement `agent_types::Tool`

- [ ] **Step 7: Verify full workspace compiles**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 8: ATOMIC COMMIT for Tasks 4-7**

This single commit covers all changes from Tasks 4, 5, 6, and 7. The workspace was non-compiling between these tasks, so they ship as one unit.

```bash
git add crates/srow-core/
git commit -m "refactor(srow-core): unify all tools on agent_types::Tool, delete duplicate traits and types

- Replace domain::tool types with re-exports from agent-types
- Delete ports::tool::Tool trait, add SrowToolContext implementing agent_types::ToolContext
- Migrate all 16 runtime tools + delegate tool + MCP adapters + skill tools
- Update SecurityGuard to use dyn ToolContext
- ToolResult: {output,tool_call_id,duration_ms} → {content,details}
- ToolCall: .input → .arguments"
```

---

## Task 8: Move Provider trait and ProviderError to agent-types

The Provider trait only depends on types already in agent-types (all 8 model capability traits). Moving it eliminates the need for external crates to depend on srow-core just to implement a provider.

**Files:**
- Create: `crates/agent-types/src/provider.rs`
- Modify: `crates/agent-types/src/lib.rs`
- Modify: `crates/srow-core/src/ports/provider/errors.rs`
- Modify: `crates/srow-core/src/ports/provider/provider_registry.rs`
- Modify: `crates/srow-core/src/ports/provider/mod.rs`
- Modify: `crates/srow-core/src/lib.rs`

- [ ] **Step 1: Create agent-types/src/provider.rs**

Create the file with Provider trait, ProviderError, and ProviderRegistry — moved from srow-core:

```rust
use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    EmbeddingModel, ImageModel, LanguageModel, ModerationModel, RerankingModel, SpeechModel,
    TranscriptionModel, VideoModel,
};

// ---------------------------------------------------------------------------
// ProviderError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, thiserror::Error)]
pub enum ProviderError {
    #[error("API call error: {message}")]
    ApiCall {
        message: String,
        url: String,
        status_code: Option<u16>,
        response_body: Option<String>,
        is_retryable: bool,
    },

    #[error("Empty response body")]
    EmptyResponseBody,

    #[error("Invalid argument '{argument}': {message}")]
    InvalidArgument { argument: String, message: String },

    #[error("Invalid prompt: {message}")]
    InvalidPrompt { message: String },

    #[error("Invalid response data: {message}")]
    InvalidResponseData { message: String },

    #[error("JSON parse error: {message}")]
    JsonParse { message: String, text: String },

    #[error("API key error: {message}")]
    LoadApiKey { message: String },

    #[error("Setting error: {message}")]
    LoadSetting { message: String },

    #[error("No content generated")]
    NoContentGenerated,

    #[error("No such {model_type}: {model_id}")]
    NoSuchModel { model_id: String, model_type: String },

    #[error("Too many embedding values: {count} > {max}")]
    TooManyEmbeddingValues { count: usize, max: usize },

    #[error("Type validation error: {message}")]
    TypeValidation { message: String },

    #[error("Unsupported: {0}")]
    UnsupportedFunctionality(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Rate limited")]
    RateLimited { retry_after_ms: Option<u64> },
}

// ---------------------------------------------------------------------------
// Provider trait
// ---------------------------------------------------------------------------

/// Factory for obtaining model instances by provider+model ID.
///
/// Implementations wrap a specific LLM backend (e.g., OpenAI, Anthropic)
/// and produce model instances on demand.
pub trait Provider: Send + Sync {
    /// Unique provider identifier (e.g., "openai", "anthropic").
    fn id(&self) -> &str;

    /// Create a language model instance for the given model ID.
    fn language_model(&self, model_id: &str) -> Result<Arc<dyn LanguageModel>, ProviderError>;

    /// Create an embedding model instance.
    fn embedding_model(&self, _model_id: &str) -> Result<Arc<dyn EmbeddingModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "embedding models are not supported by this provider".to_string(),
        ))
    }

    /// Create a transcription model instance.
    fn transcription_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn TranscriptionModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "transcription models are not supported by this provider".to_string(),
        ))
    }

    /// Create a speech model instance.
    fn speech_model(&self, _model_id: &str) -> Result<Arc<dyn SpeechModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "speech models are not supported by this provider".to_string(),
        ))
    }

    /// Create an image model instance.
    fn image_model(&self, _model_id: &str) -> Result<Arc<dyn ImageModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "image models are not supported by this provider".to_string(),
        ))
    }

    /// Create a video model instance.
    fn video_model(&self, _model_id: &str) -> Result<Arc<dyn VideoModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "video models are not supported by this provider".to_string(),
        ))
    }

    /// Create a reranking model instance.
    fn reranking_model(&self, _model_id: &str) -> Result<Arc<dyn RerankingModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "reranking models are not supported by this provider".to_string(),
        ))
    }

    /// Create a moderation model instance.
    fn moderation_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn ModerationModel>, ProviderError> {
        Err(ProviderError::UnsupportedFunctionality(
            "moderation models are not supported by this provider".to_string(),
        ))
    }
}

// ---------------------------------------------------------------------------
// ProviderRegistry
// ---------------------------------------------------------------------------

/// Central registry of all available providers.
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.insert(provider.id().to_string(), provider);
    }

    pub fn get(&self, provider_id: &str) -> Option<&Arc<dyn Provider>> {
        self.providers.get(provider_id)
    }

    pub fn language_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn LanguageModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "language".to_string(),
            }
        })?;
        provider.language_model(model_id)
    }

    pub fn embedding_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn EmbeddingModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "embedding".to_string(),
            }
        })?;
        provider.embedding_model(model_id)
    }

    pub fn transcription_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn TranscriptionModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "transcription".to_string(),
            }
        })?;
        provider.transcription_model(model_id)
    }

    pub fn speech_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn SpeechModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "speech".to_string(),
            }
        })?;
        provider.speech_model(model_id)
    }

    pub fn image_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn ImageModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "image".to_string(),
            }
        })?;
        provider.image_model(model_id)
    }

    pub fn video_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn VideoModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "video".to_string(),
            }
        })?;
        provider.video_model(model_id)
    }

    pub fn reranking_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn RerankingModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "reranking".to_string(),
            }
        })?;
        provider.reranking_model(model_id)
    }

    pub fn moderation_model(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Arc<dyn ModerationModel>, ProviderError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            ProviderError::NoSuchModel {
                model_id: format!("{provider_id}:{model_id}"),
                model_type: "moderation".to_string(),
            }
        })?;
        provider.moderation_model(model_id)
    }

    pub fn provider_ids(&self) -> Vec<&str> {
        self.providers.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 2: Update agent-types lib.rs**

Add to `crates/agent-types/src/lib.rs`:

```rust
pub mod provider;
pub use provider::{Provider, ProviderError, ProviderRegistry};
```

- [ ] **Step 3: Verify agent-types compiles**

Run: `cargo check -p agent-types`
Expected: PASS

- [ ] **Step 4: Replace srow-core provider files with re-exports**

`crates/srow-core/src/ports/provider/errors.rs`:
```rust
pub use agent_types::ProviderError;
```

`crates/srow-core/src/ports/provider/provider_registry.rs`:
```rust
pub use agent_types::{Provider, ProviderRegistry};

#[cfg(test)]
mod tests {
    use super::*;
    use agent_types::*;
    use async_trait::async_trait;
    use std::pin::Pin;
    use std::sync::Arc;

    struct MockModel {
        id: String,
    }

    #[async_trait]
    impl LanguageModel for MockModel {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Result<Message, AgentError> {
            Ok(Message::system("mock"))
        }

        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Pin<Box<dyn futures::Stream<Item = StreamEvent> + Send>> {
            Box::pin(tokio_stream::empty())
        }

        fn model_id(&self) -> &str {
            &self.id
        }
    }

    struct MockProvider;

    impl Provider for MockProvider {
        fn id(&self) -> &str {
            "mock"
        }

        fn language_model(
            &self,
            model_id: &str,
        ) -> Result<Arc<dyn LanguageModel>, ProviderError> {
            Ok(Arc::new(MockModel {
                id: model_id.to_string(),
            }))
        }
    }

    #[test]
    fn register_and_lookup() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider));
        assert!(registry.get("mock").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn language_model_shorthand() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider));
        let model = registry.language_model("mock", "gpt-4").unwrap();
        assert_eq!(model.model_id(), "gpt-4");
    }

    #[test]
    fn missing_provider_returns_error() {
        let registry = ProviderRegistry::new();
        let result = registry.language_model("nonexistent", "model");
        assert!(result.is_err());
    }
}
```

`crates/srow-core/src/ports/provider/mod.rs`:
```rust
pub mod types;
pub mod errors;
pub mod provider_registry;

pub use errors::*;
pub use provider_registry::{Provider, ProviderRegistry};
```

- [ ] **Step 5: Update srow-core lib.rs**

The existing line:
```rust
pub use ports::provider::provider_registry::{Provider, ProviderRegistry};
```
remains valid since it now re-exports from agent-types through the chain.

- [ ] **Step 6: Verify full workspace compiles**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 7: Run tests**

Run: `cargo test -p srow-core -- provider`
Expected: PASS (3 tests)

- [ ] **Step 8: Commit**

```bash
git add crates/agent-types/src/provider.rs crates/agent-types/src/lib.rs crates/srow-core/src/ports/provider/
git commit -m "feat(agent-types): move Provider trait and ProviderError from srow-core"
```

---

## Task 9: Make srow-core a Facade — re-export agent-core types

srow-app currently imports from `agent_core` and `agent_types` directly. We make srow-core re-export everything srow-app needs, so srow-app only depends on `srow-core` (+ `srow-debug` for debug infra).

**Files:**
- Modify: `crates/srow-core/Cargo.toml` — add `agent-core` dependency
- Modify: `crates/srow-core/src/lib.rs` — add agent-core re-exports

- [ ] **Step 1: Add agent-core to srow-core dependencies**

In `crates/srow-core/Cargo.toml`, under `[dependencies]`:

```toml
agent-core = { path = "../agent-core" }
```

- [ ] **Step 2: Add re-exports to srow-core lib.rs**

Add to `crates/srow-core/src/lib.rs`:

```rust
// Re-export agent-core types for UI layer consumption.
// Note: agent-core's AgentConfig is renamed to avoid collision with domain::agent::AgentConfig.
pub use agent_core::{Agent, AgentConfig as AgentHookConfig, AgentEvent, AgentMessage};
pub use agent_core::types::AgentContext;
```

Note: `srow_core::domain::agent::AgentConfig` already exists (agent instance config), so we alias the hook config as `AgentHookConfig`.

Also ensure agent-types types are re-exported (they already are via `pub use agent_types;`):

```rust
// Already existing, but verify these are accessible:
// pub use agent_types;  ← existing line covers all agent-types types
```

- [ ] **Step 3: Verify srow-core compiles**

Run: `cargo check -p srow-core`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/srow-core/Cargo.toml crates/srow-core/src/lib.rs
git commit -m "feat(srow-core): re-export agent-core types as Facade for UI layer"
```

---

## Task 10: Update srow-app to import through srow-core Facade

Remove direct dependencies on `agent-types`, `agent-core`, and `agent-graph` from srow-app.

**Files:**
- Modify: `crates/srow-app/Cargo.toml`
- Modify: `crates/srow-app/src/chat/gpui_chat.rs`
- Modify: `crates/srow-app/src/views/chat_panel/message_list.rs`

- [ ] **Step 1: Update srow-app Cargo.toml**

Remove these lines from `[dependencies]`:
```toml
agent-types = { path = "../agent-types" }
agent-core = { path = "../agent-core" }
agent-graph = { path = "../agent-graph" }
```

- [ ] **Step 2: Update gpui_chat.rs imports**

Change:
```rust
use agent_types::{
    AgentError, ContentBlock, LanguageModel, Message, MessageRole, ModelConfig, StreamEvent, Tool,
};
use agent_core::{AgentConfig, AgentContext, AgentEvent, AgentMessage};
```

To:
```rust
use srow_core::agent_types::{
    AgentError, ContentBlock, LanguageModel, Message, MessageRole, ModelConfig, StreamEvent, Tool,
};
use srow_core::{AgentHookConfig, AgentContext, AgentEvent, AgentMessage};
```

Also update usages of `AgentConfig` → `AgentHookConfig` in the body:
```rust
let agent_config = AgentHookConfig::new(Arc::new(|ctx: &AgentContext<'_>| { ... }));
```

And update `agent_core::Agent` → `srow_core::Agent`:
```rust
let agent = srow_core::Agent::new(model, "You are a helpful assistant.", agent_config);
```

- [ ] **Step 3: Update message_list.rs imports**

Change:
```rust
use agent_types::MessageRole;
use agent_core::AgentMessage;
```

To:
```rust
use srow_core::agent_types::MessageRole;
use srow_core::AgentMessage;
```

- [ ] **Step 4: Verify srow-app compiles**

Run: `cargo check -p srow-app`
Expected: PASS

- [ ] **Step 5: Verify full workspace compiles**

Run: `cargo check --workspace`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/srow-app/
git commit -m "refactor(srow-app): import through srow-core Facade, remove direct agent-* deps"
```

---

## Task 11: Final verification and cleanup

- [ ] **Step 1: Full workspace build**

Run: `cargo build --workspace`
Expected: PASS

- [ ] **Step 2: Run all tests**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 3: Verify dependency graph is clean**

Run: `cargo tree -p srow-app --depth 1`
Expected: srow-app should NOT show agent-types, agent-core, or agent-graph as direct dependencies (they appear only as transitive deps through srow-core).

- [ ] **Step 4: Remove TODO comments**

Search for and remove old migration TODO comments in:
- `crates/srow-core/src/ports/tool.rs` (old TODO about migrating to agent-types)
- `crates/srow-core/src/domain/tool.rs` (old TODO about migrating)
- Any other files with "TODO: Migrate to agent-types" comments

- [ ] **Step 5: Update AGENTS.md files**

Update the AGENTS.md files in affected crates to reflect the new architecture:
- `crates/agent-types/src/AGENTS.md` — mention Tool, ToolContext, Provider, ProviderError
- `crates/agent-core/src/AGENTS.md` — mention ToolContext passing
- `crates/srow-core/src/AGENTS.md` — mention SrowToolContext, Facade pattern

- [ ] **Step 6: Final commit**

```bash
git add -A
git commit -m "chore: cleanup TODO comments and update AGENTS.md after architecture unification"
```
