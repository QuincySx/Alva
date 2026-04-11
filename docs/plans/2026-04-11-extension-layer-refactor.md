# Extension Layer Refactor — Move to App-Core

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Move Extension system to the correct architectural layer (app-core), making it a rich "participant" API inspired by Pi's design — not just a passive provider.

**Architecture:**
```
alva-agent-core   → Tool trait, Middleware trait (引擎层，不知道 Extension)
alva-agent-tools  → Tool 实现 (不知道 Extension)
alva-agent-runtime → Middleware 实现 (不知道 Extension)
alva-app-core     → Extension trait + ExtensionContext + 所有 Extension 实现 + BaseAgent
```

**Key design (inspired by Pi):**
- Extension is a "participant" — it registers tools/middleware AND receives context
- Extension trait + ExtensionContext live in alva-app-core
- Tool presets (file_io, shell, etc.) stay in alva-agent-tools as plain functions
- Extension implementations wrap presets, live in alva-app-core

---

### Task 1: Remove Extension from alva-agent-core

**Files:**
- Delete: `crates/alva-agent-core/src/extension.rs`
- Modify: `crates/alva-agent-core/src/lib.rs` — remove `pub mod extension` and `pub use extension::Extension`

**Step 1:** Delete `crates/alva-agent-core/src/extension.rs`

**Step 2:** Remove from `crates/alva-agent-core/src/lib.rs`:
```rust
// Remove these lines:
pub mod extension;
pub use extension::Extension;
```

**Step 3:** Verify: `cargo check -p alva-agent-core`
Expected: PASS (nothing in core uses Extension)

---

### Task 2: Remove extensions.rs from alva-agent-tools

**Files:**
- Delete: `crates/alva-agent-tools/src/extensions.rs`
- Modify: `crates/alva-agent-tools/src/lib.rs` — remove `pub mod extensions;`
- Modify: `crates/alva-agent-tools/Cargo.toml` — remove `alva-agent-core` dependency (tools don't need core anymore, they only need alva-types)

**Step 1:** Delete `crates/alva-agent-tools/src/extensions.rs`

**Step 2:** In `crates/alva-agent-tools/src/lib.rs`, remove:
```rust
pub mod extensions;
```

**Step 3:** In `crates/alva-agent-tools/Cargo.toml`, remove:
```toml
alva-agent-core = { path = "../alva-agent-core" }
```

**Step 4:** Verify: `cargo check -p alva-agent-tools`

---

### Task 3: Remove extensions.rs from alva-agent-runtime

**Files:**
- Delete: `crates/alva-agent-runtime/src/extensions.rs`
- Modify: `crates/alva-agent-runtime/src/lib.rs` — remove `pub mod extensions;`

**Step 1:** Delete `crates/alva-agent-runtime/src/extensions.rs`

**Step 2:** In `crates/alva-agent-runtime/src/lib.rs`, remove:
```rust
pub mod extensions;
```

**Step 3:** Verify: `cargo check -p alva-agent-runtime`

---

### Task 4: Create Extension system in alva-app-core

**Files:**
- Create: `crates/alva-app-core/src/extension/mod.rs`
- Create: `crates/alva-app-core/src/extension/context.rs`
- Create: `crates/alva-app-core/src/extension/builtins.rs`
- Modify: `crates/alva-app-core/src/lib.rs` — add `pub mod extension;` and re-exports

**Step 1:** Create `crates/alva-app-core/src/extension/mod.rs`:

```rust
//! Extension system — the primary extensibility point for agents.
//!
//! Extensions are "participants" that register tools and middleware,
//! receive agent context, and can interact through the bus.

mod context;
mod builtins;

pub use context::ExtensionContext;
pub use builtins::*;

use std::sync::Arc;
use alva_types::tool::Tool;
use alva_agent_core::middleware::Middleware;

/// A capability package that participates in agent construction.
///
/// Extensions are activated during `BaseAgent::build()`. They receive
/// an `ExtensionContext` with access to bus, workspace, and other
/// registered tools — enabling context-aware composition.
///
/// This is the **only** public extensibility point for BaseAgent users.
pub trait Extension: Send + Sync {
    /// Unique name for this extension.
    fn name(&self) -> &str;

    /// Human-readable description.
    fn description(&self) -> &str { "" }

    /// Tools this extension provides. Called during build().
    fn tools(&self) -> Vec<Box<dyn Tool>> { vec![] }

    /// Middleware this extension provides. Called during build().
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> { vec![] }

    /// Called after all extensions are collected and bus/workspace are ready.
    /// Use this to subscribe to bus events, check other registered tools,
    /// or perform context-dependent setup.
    fn configure(&self, _ctx: &ExtensionContext) {}
}
```

**Step 2:** Create `crates/alva-app-core/src/extension/context.rs`:

```rust
//! ExtensionContext — what extensions can see and do after activation.

use std::path::PathBuf;
use alva_types::BusHandle;

/// Context provided to extensions during the configure phase.
///
/// At this point, all extensions have registered their tools and middleware.
/// Extensions can use this context to:
/// - Subscribe to bus events for cross-extension communication
/// - Check which tools are registered (to conditionally adjust behavior)
/// - Access the workspace path
pub struct ExtensionContext {
    /// Cross-layer coordination bus (event pub/sub + service discovery).
    pub bus: BusHandle,
    /// Workspace root directory.
    pub workspace: PathBuf,
    /// Names of all tools registered so far (from all extensions).
    pub tool_names: Vec<String>,
}
```

**Step 3:** Create `crates/alva-app-core/src/extension/builtins.rs`:

```rust
//! Built-in extensions that wrap tool_presets and middleware_presets.

use std::sync::Arc;

use alva_types::tool::Tool;
use alva_agent_core::middleware::Middleware;
use alva_agent_tools::tool_presets;

use super::Extension;

// ── Tool extensions ──

pub struct CoreExtension;
impl Extension for CoreExtension {
    fn name(&self) -> &str { "core" }
    fn description(&self) -> &str { "Core file I/O tools" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::file_io() }
}

pub struct ShellExtension;
impl Extension for ShellExtension {
    fn name(&self) -> &str { "shell" }
    fn description(&self) -> &str { "Shell command execution" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::shell() }
}

pub struct TaskExtension;
impl Extension for TaskExtension {
    fn name(&self) -> &str { "tasks" }
    fn description(&self) -> &str { "Task tracking and management" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::task_management() }
}

pub struct TeamExtension;
impl Extension for TeamExtension {
    fn name(&self) -> &str { "team" }
    fn description(&self) -> &str { "Multi-agent team coordination" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::team() }
}

pub struct PlanningExtension;
impl Extension for PlanningExtension {
    fn name(&self) -> &str { "planning" }
    fn description(&self) -> &str { "Planning mode, worktree, TODO" }
    fn tools(&self) -> Vec<Box<dyn Tool>> {
        let mut t = tool_presets::planning();
        t.extend(tool_presets::worktree());
        t
    }
}

pub struct UtilityExtension;
impl Extension for UtilityExtension {
    fn name(&self) -> &str { "utility" }
    fn description(&self) -> &str { "Utility tools" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::utility() }
}

pub struct WebExtension;
impl Extension for WebExtension {
    fn name(&self) -> &str { "web" }
    fn description(&self) -> &str { "Internet search and URL fetching" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::web() }
}

pub struct BrowserExtension;
impl Extension for BrowserExtension {
    fn name(&self) -> &str { "browser" }
    fn description(&self) -> &str { "Browser automation" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::browser_tools() }
}

pub struct InteractionExtension;
impl Extension for InteractionExtension {
    fn name(&self) -> &str { "interaction" }
    fn description(&self) -> &str { "Human interaction" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::interaction() }
}

pub struct AllStandardExtension;
impl Extension for AllStandardExtension {
    fn name(&self) -> &str { "all-standard" }
    fn description(&self) -> &str { "All standard tools" }
    fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::all_standard() }
}

// ── Middleware extensions ──

pub struct GuardrailsExtension;
impl Extension for GuardrailsExtension {
    fn name(&self) -> &str { "guardrails" }
    fn description(&self) -> &str { "Loop detection, dangling tool check, timeout" }
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![
            Arc::new(alva_agent_core::builtins::LoopDetectionMiddleware::new()),
            Arc::new(alva_agent_core::builtins::DanglingToolCallMiddleware::new()),
            Arc::new(alva_agent_core::builtins::ToolTimeoutMiddleware::default()),
        ]
    }
}

pub struct CompactionExtension;
impl Extension for CompactionExtension {
    fn name(&self) -> &str { "compaction" }
    fn description(&self) -> &str { "Auto-summarize when context is full" }
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![Arc::new(alva_agent_runtime::middleware::CompactionMiddleware::default())]
    }
}

pub struct CheckpointExtension;
impl Extension for CheckpointExtension {
    fn name(&self) -> &str { "checkpoint" }
    fn description(&self) -> &str { "File backups before writes" }
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![Arc::new(alva_agent_runtime::middleware::CheckpointMiddleware::new())]
    }
}

pub struct PlanModeExtension;
impl Extension for PlanModeExtension {
    fn name(&self) -> &str { "plan-mode" }
    fn description(&self) -> &str { "Block write tools in plan mode" }
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![Arc::new(alva_agent_runtime::middleware::PlanModeMiddleware::new(false))]
    }
}

pub struct ProductionExtension;
impl Extension for ProductionExtension {
    fn name(&self) -> &str { "production" }
    fn description(&self) -> &str { "Full production middleware stack" }
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![
            Arc::new(alva_agent_core::builtins::LoopDetectionMiddleware::new()),
            Arc::new(alva_agent_core::builtins::DanglingToolCallMiddleware::new()),
            Arc::new(alva_agent_core::builtins::ToolTimeoutMiddleware::default()),
            Arc::new(alva_agent_runtime::middleware::CompactionMiddleware::default()),
            Arc::new(alva_agent_runtime::middleware::CheckpointMiddleware::new()),
            Arc::new(alva_agent_runtime::middleware::PlanModeMiddleware::new(false)),
        ]
    }
}
```

**Step 4:** In `crates/alva-app-core/src/lib.rs`, add:
```rust
pub mod extension;
pub use extension::Extension;
```

Remove old re-exports:
```rust
// Remove these:
pub use alva_agent_tools::extensions as tool_extensions;
pub use alva_agent_runtime::extensions as runtime_extensions;
```

**Step 5:** Verify: `cargo check -p alva-app-core`

---

### Task 5: Update BaseAgent builder to use app-core Extension

**Files:**
- Modify: `crates/alva-app-core/src/base_agent/builder.rs`

**Step 1:** Change the `extensions` field type:
```rust
// From:
pub(crate) extensions: Vec<Box<dyn alva_agent_core::Extension>>,
// To:
pub(crate) extensions: Vec<Box<dyn crate::extension::Extension>>,
```

**Step 2:** Update `.extension()` method to use local trait.

**Step 3:** In `build()`, after collecting tools/middleware from extensions and creating bus, call `ext.configure(ctx)` on each extension with the full context:

```rust
// After middleware_stack.configure_all() and tool_registry is built:
let ext_ctx = ExtensionContext {
    bus: bus_handle.clone(),
    workspace: workspace.clone(),
    tool_names: tool_registry.definitions().iter().map(|d| d.name.clone()).collect(),
};
for ext in &self.extensions {
    ext.configure(&ext_ctx);
}
```

**Step 4:** Verify: `cargo check -p alva-app-core`

---

### Task 6: Update all callers

**Files:**
- Modify: `crates/alva-app-cli/src/agent_setup.rs`
- Modify: `crates/alva-agent-eval/src/main.rs`
- Modify: `crates/alva-app-core/tests/e2e_agent_test.rs`

**Step 1:** CLI — use `alva_app_core::extension::*`:
```rust
.extension(Box::new(alva_app_core::extension::AllStandardExtension))
.extension(Box::new(alva_app_core::extension::ProductionExtension))
```

**Step 2:** Eval — already uses `.tools()` / `.middlewares()` directly (no change needed for dynamic selection). For browser, use:
```rust
.extension(Box::new(alva_app_core::extension::BrowserExtension))
```

**Step 3:** Tests — update any test that references old extension paths.

**Step 4:** Verify: `cargo check` (full workspace)

**Step 5:** Run tests: `cargo test -p alva-app-core -p alva-agent-core`

---

### Task 7: Clean up and commit

**Step 1:** Remove old `middleware_presets` and `tool_presets` re-exports from `alva-app-core/src/lib.rs` if no longer needed.

**Step 2:** Verify clean: `cargo check` (0 errors, check warnings)

**Step 3:** Run full test suite: `cargo test`

**Step 4:** Commit with message:
```
refactor: move Extension to app-core — correct architectural layer

Extension is a composition concept that lives ABOVE Tool/Middleware:
- alva-agent-core: Tool trait + Middleware trait (engine primitives)
- alva-agent-tools: Tool implementations (no Extension knowledge)
- alva-agent-runtime: Middleware implementations (no Extension knowledge)
- alva-app-core: Extension trait + ExtensionContext + builtin Extensions

Extension is now a "participant" (Pi-inspired):
- tools() → register tools
- middleware() → register middleware
- configure(ctx) → receive bus, workspace, tool_names for context-aware setup

Builtin extensions: AllStandard, Production, Core, Shell, Web, Browser,
Task, Team, Planning, Utility, Guardrails, Compaction, Checkpoint, PlanMode
```
