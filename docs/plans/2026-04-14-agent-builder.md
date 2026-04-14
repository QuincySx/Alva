## Agent Core Assembly API — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a clean SDK-level assembly API (`alva_agent_core::Agent` + `AgentBuilder`) that lets users compose their own agent from the SDK without pulling in harness-level decisions (SQLite, SecurityMiddleware, protocol extensions). Then refactor `alva-app-core::BaseAgentBuilder` to use it internally and expose memory/security override points so `BaseAgent` becomes preset-but-not-prescriptive.

**Architecture:**
After the refactor, users have two entry points:
1. **`alva_agent_core::Agent::builder()`** — raw SDK assembly. No presets. Users bring their own model, extensions, middleware. Third-party harnesses target this. Runs on `alva_kernel_core::run_agent` under the hood.
2. **`alva_app_core::BaseAgent::builder()`** — opinionated harness. Pre-wires the 12 built-in Extensions, defaults to `MemorySqlite` + `SecurityMiddleware::for_workspace`, but exposes `.memory_service(...)` and `.security_middleware(...)` overrides. Built on top of `alva_agent_core::AgentBuilder`.

Memory is kept as a harness concern (lives on `BaseAgent` as a separate field, NOT inside the `Agent` loop) — this matches the current code shape and avoids dragging `alva-agent-memory` into `alva-agent-core`'s dependency graph.

Security is a regular `Middleware` — no trait extraction needed.

**Tech Stack:** Rust 2021, async-trait, tokio. No new external deps.

**Non-goals:**
- Moving `BaseAgent` out of `alva-app-core` (keeps harness code where it belongs)
- Deleting `alva-host-native::AgentRuntimeBuilder` (left as-is; may be deprecated in a later cleanup)
- Extracting new `MemoryBackend` / `SecurityBackend` traits (they already exist or aren't needed)

---

## Phase 1 — SDK assembly API in `alva-agent-core`

### Task 1.1: Skeleton files + lib.rs wiring

**Files:**
- Create: `crates/alva-agent-core/src/agent.rs`
- Create: `crates/alva-agent-core/src/agent_builder.rs`
- Modify: `crates/alva-agent-core/src/lib.rs` — declare modules and re-export public API

**Step 1: Write `crates/alva-agent-core/src/agent.rs`** (placeholder for the type)

```rust
//! Agent — SDK-level assembled agent handle.
//!
//! Produced by `AgentBuilder::build()`. Holds the wired-up `AgentState` +
//! `AgentConfig` + bus/extension-host bookkeeping. Runs the agent loop via
//! `alva_kernel_core::run_agent` when `.run()` is called.

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use alva_kernel_abi::{
    AgentError, AgentEvent, AgentMessage, BusHandle, CancellationToken,
};
use alva_kernel_core::state::{AgentConfig, AgentState};
use alva_kernel_core::run_agent;

use crate::extension::ExtensionHost;

/// A fully-assembled, ready-to-run agent.
///
/// Use `Agent::builder()` to construct one.
pub struct Agent {
    pub(crate) state: Mutex<AgentState>,
    pub(crate) config: AgentConfig,
    pub(crate) bus: BusHandle,
    pub(crate) host: Arc<std::sync::RwLock<ExtensionHost>>,
}

impl Agent {
    /// Start building a new agent.
    pub fn builder() -> crate::agent_builder::AgentBuilder {
        crate::agent_builder::AgentBuilder::new()
    }

    /// Run one conversation turn. Returns a channel that streams
    /// `AgentEvent`s until the turn completes.
    ///
    /// `cancel` lets the caller interrupt the loop mid-turn.
    pub async fn run(
        &self,
        input: Vec<AgentMessage>,
        cancel: CancellationToken,
    ) -> Result<mpsc::UnboundedReceiver<AgentEvent>, AgentError> {
        let (tx, rx) = mpsc::unbounded_channel();
        let mut state = self.state.lock().await;
        run_agent(&mut state, &self.config, cancel, input, tx).await?;
        Ok(rx)
    }

    /// Access the bus for out-of-band communication (e.g. injecting
    /// steering messages, reading capability registrations).
    pub fn bus(&self) -> &BusHandle { &self.bus }
}
```

**Step 2: Write `crates/alva-agent-core/src/agent_builder.rs`** (skeleton — body filled in Task 1.2)

```rust
//! AgentBuilder — SDK-level builder that assembles an `Agent` from
//! extensions, tools, middleware, model, and kernel config.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use alva_kernel_abi::{
    Bus, BusHandle, BusWriter, LanguageModel, ModelConfig, Tool, ToolRegistry,
};
use alva_kernel_abi::session::{AgentSession, InMemorySession};
use alva_kernel_core::middleware::{Middleware, MiddlewareStack};
use alva_kernel_core::shared::Extensions;
use alva_kernel_core::state::{AgentConfig, AgentState};
use tokio::sync::Mutex;

use crate::agent::Agent;
use crate::extension::{Extension, ExtensionBridgeMiddleware, ExtensionHost, HostAPI};

/// SDK-level builder for assembling an `Agent`.
///
/// This is the layer at which `alva-agent-core` assembles an agent without
/// any harness-level opinions. Callers (third-party harnesses or tests)
/// compose their own model, extensions, and middleware here. Opinionated
/// wrappers like `alva_app_core::BaseAgentBuilder` delegate to this.
pub struct AgentBuilder {
    model: Option<Arc<dyn LanguageModel>>,
    system_prompt: String,
    workspace: Option<PathBuf>,
    model_config: ModelConfig,
    max_iterations: u32,
    context_window: usize,

    extensions: Vec<Box<dyn Extension>>,
    extra_tools: Vec<Box<dyn Tool>>,
    extra_middleware: Vec<Arc<dyn Middleware>>,

    bus: Option<BusHandle>,
    bus_writer: Option<BusWriter>,
    session: Option<Arc<dyn AgentSession>>,

    context_system: Option<Arc<alva_kernel_abi::scope::context::ContextSystem>>,
    context_token_budget: Option<usize>,
}

impl AgentBuilder {
    pub fn new() -> Self {
        Self {
            model: None,
            system_prompt: String::new(),
            workspace: None,
            model_config: ModelConfig::default(),
            max_iterations: 100,
            context_window: 0,
            extensions: Vec::new(),
            extra_tools: Vec::new(),
            extra_middleware: Vec::new(),
            bus: None,
            bus_writer: None,
            session: None,
            context_system: None,
            context_token_budget: None,
        }
    }

    pub fn model(mut self, m: Arc<dyn LanguageModel>) -> Self {
        self.model = Some(m); self
    }
    pub fn system_prompt(mut self, s: impl Into<String>) -> Self {
        self.system_prompt = s.into(); self
    }
    pub fn workspace(mut self, p: impl Into<PathBuf>) -> Self {
        self.workspace = Some(p.into()); self
    }
    pub fn model_config(mut self, cfg: ModelConfig) -> Self {
        self.model_config = cfg; self
    }
    pub fn max_iterations(mut self, n: u32) -> Self {
        self.max_iterations = n; self
    }
    pub fn context_window(mut self, n: usize) -> Self {
        self.context_window = n; self
    }
    pub fn extension(mut self, e: Box<dyn Extension>) -> Self {
        self.extensions.push(e); self
    }
    pub fn tool(mut self, t: Box<dyn Tool>) -> Self {
        self.extra_tools.push(t); self
    }
    pub fn middleware(mut self, mw: Arc<dyn Middleware>) -> Self {
        self.extra_middleware.push(mw); self
    }
    pub fn with_bus(mut self, bus: BusHandle) -> Self {
        self.bus = Some(bus); self
    }
    pub fn with_bus_writer(mut self, bw: BusWriter) -> Self {
        self.bus = Some(bw.handle()); self.bus_writer = Some(bw); self
    }
    pub fn session(mut self, s: Arc<dyn AgentSession>) -> Self {
        self.session = Some(s); self
    }
    pub fn with_context_system(
        mut self,
        cs: Arc<alva_kernel_abi::scope::context::ContextSystem>,
    ) -> Self {
        self.context_system = Some(cs); self
    }
    pub fn with_context_token_budget(mut self, budget: usize) -> Self {
        self.context_token_budget = Some(budget); self
    }

    /// Build the Agent. Runs the extension lifecycle
    /// (`tools` → `activate` → `configure` → `finalize`), wires middleware,
    /// and produces a ready-to-run `Agent`.
    pub async fn build(self) -> Result<Agent, alva_kernel_abi::AgentError> {
        // Body filled in Task 1.2.
        todo!("Task 1.2")
    }
}

impl Default for AgentBuilder {
    fn default() -> Self { Self::new() }
}
```

**Step 3: Update `crates/alva-agent-core/src/lib.rs`** — add module declarations and re-exports

At the bottom of the existing file, append:

```rust
pub mod agent;
pub mod agent_builder;

pub use agent::Agent;
pub use agent_builder::AgentBuilder;
```

**Step 4: Verify compilation**

```bash
cargo check -p alva-agent-core
```

Expected: PASS (the `todo!()` in `build()` is fine at check-time). If any `use` statements can't be resolved, fix them. The only imports that might not work are kernel types — check the actual re-export path. Run:

```bash
rg 'pub use' crates/alva-kernel-abi/src/lib.rs
rg 'pub use' crates/alva-kernel-core/src/lib.rs
```

and adjust import paths to match what those crates actually re-export.

**Step 5: Commit**

```bash
git add crates/alva-agent-core
git commit -m "feat(agent-core): scaffold Agent + AgentBuilder skeleton"
```

---

### Task 1.2: Implement `AgentBuilder::build()`

**Files:** Modify `crates/alva-agent-core/src/agent_builder.rs` only.

**Step 1: Reference the existing build logic**

The closest existing implementation is in `crates/alva-host-native/src/builder.rs::AgentRuntimeBuilder::build()` (around lines 191–304). Read that function carefully — our new `AgentBuilder::build()` is a **simplified, sync-less-heavy** version of the same logic, minus:
- No `SandboxMode` / `SecurityMiddleware` wiring (those are harness concerns)
- No `PendingMessageQueue` / `ToolTimeoutMiddleware` / etc (those are harness middleware, added by BaseAgent downstream)
- No approval notifier, no bus plugins, no context token counter default

Keep only the **generic extension lifecycle + tool collection + middleware assembly**.

**Step 2: Replace `todo!("Task 1.2")` with the real body**

The body must:

1. **Validate required inputs**: `model` must be set. Error if not.
2. **Set up bus**: if `self.bus_writer` or `self.bus` is set, use it. Otherwise create a fresh `Bus::new()` and derive both handle and writer.
3. **Create ExtensionHost**: `let host = Arc::new(RwLock::new(ExtensionHost::new()));`
4. **Run `Extension::tools()`** for every extension — collect all tools into a single `Vec<Box<dyn Tool>>`. Append `self.extra_tools` to the collected list.
5. **Run `Extension::activate(&host_api)`** for every extension, where `host_api` is a `HostAPI::new(host.clone(), ext_name)`. This lets extensions register middleware and commands via the HostAPI.
6. **Drain middlewares from the host**: call `host.write().unwrap().take_middlewares()` and collect into a `MiddlewareStack`. Also append `self.extra_middleware`.
7. **Append an `ExtensionBridgeMiddleware::new(host.clone())`** at the end of the middleware stack (so kernel loop events reach the extension host).
8. **Run `Extension::configure(ctx)`** — build an `ExtensionContext` with workspace/bus/host references and call it on each extension. (Note: the existing `ExtensionContext` type may take specific fields — copy the construction pattern from `alva-app-core/src/base_agent/builder.rs` where it's already done.)
9. **Register all tools into a `ToolRegistry`**: iterate the collected tool list, call `.register(tool)` on each.
10. **Run `Extension::finalize(ctx)`** — build a `FinalizeContext` with the final tool registry visible, call on each extension, collect any additional tools returned, and add them to the registry.
11. **Extract `Vec<Arc<dyn Tool>>` from the registry** for `AgentState`: `registry.list_arc()`.
12. **Create `AgentSession`**: use `self.session.unwrap_or_else(|| Arc::new(InMemorySession::new()))`.
13. **Construct `AgentState`** with:
    - `model: self.model.expect(...)`
    - `tools: the arc list`
    - `session`
    - `extensions: Extensions::new()`
14. **Construct `AgentConfig`** with the builder's fields.
15. **Return** `Ok(Agent { state: Mutex::new(state), config, bus, host })`.

**Step 3: Fix imports as needed**

You will likely need additional imports — `ExtensionContext`, `FinalizeContext`, `Extensions`, etc. Add them as the compiler complains.

**Step 4: Verify compilation**

```bash
cargo check -p alva-agent-core 2>&1 | tail -20
```

Expected: PASS. If there are errors about missing methods (`ExtensionHost::take_middlewares`, `HostAPI::new`, etc), trace them: these methods DO exist per Task 1.3 of the previous refactor — confirm by `rg 'fn take_middlewares' crates/alva-agent-core/src/extension/`.

**Step 5: Run workspace check**

```bash
cargo check --workspace 2>&1 | tail -10
```

Expected: PASS. Nothing else should have broken.

**Step 6: Commit**

```bash
git add crates/alva-agent-core/src/agent_builder.rs
git commit -m "feat(agent-core): implement AgentBuilder::build() lifecycle"
```

---

### Task 1.3: Tests for AgentBuilder

**Files:** Create `crates/alva-agent-core/tests/agent_builder.rs`

**Step 1: Write minimal tests**

```rust
//! Integration tests for `alva_agent_core::Agent` + `AgentBuilder`.

use std::sync::Arc;
use async_trait::async_trait;

use alva_agent_core::{Agent, AgentBuilder, Extension};
use alva_kernel_abi::{
    AgentError, CompletionResponse, LanguageModel, Message, ModelConfig, StreamEvent, Tool,
    CancellationToken, AgentMessage,
};
use futures_core::Stream;
use std::pin::Pin;
use tokio_stream::empty;

struct DummyModel;

#[async_trait]
impl LanguageModel for DummyModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        Ok(CompletionResponse {
            content: "ok".into(),
            tool_calls: vec![],
            finish_reason: None,
            usage: Default::default(),
        })
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        Box::pin(empty())
    }

    fn model_id(&self) -> &str { "dummy-model" }
}

#[tokio::test]
async fn build_minimal_agent_no_extensions() {
    let agent = AgentBuilder::new()
        .model(Arc::new(DummyModel))
        .system_prompt("you are a test agent")
        .max_iterations(1)
        .build()
        .await
        .expect("build should succeed");

    // The agent should expose a bus handle
    assert!(!agent.bus().has::<u32>());
}

#[tokio::test]
async fn builder_requires_model() {
    let result = AgentBuilder::new()
        .system_prompt("no model set")
        .build()
        .await;
    assert!(result.is_err(), "build without model must fail");
}
```

The `CompletionResponse` struct may have different fields than what I wrote above — check `alva-kernel-abi/src/lib.rs` or wherever `CompletionResponse` is defined, and fix the literal to match. Same for `LanguageModel`'s exact trait signature.

**Step 2: Run the tests**

```bash
cargo test -p alva-agent-core --test agent_builder 2>&1 | tail -20
```

Expected: both tests pass. If compilation fails because `CompletionResponse` fields differ, inspect the real type and adjust the literal.

**Step 3: Commit**

```bash
git add crates/alva-agent-core/tests/agent_builder.rs
git commit -m "test(agent-core): cover AgentBuilder minimal build + required-model path"
```

---

## Phase 2 — Refactor `BaseAgentBuilder` to delegate to `AgentBuilder`

### Task 2.1: Add memory override setter

**Files:** Modify `crates/alva-app-core/src/base_agent/builder.rs`.

**Step 1: Add a new field to `BaseAgentBuilder`**

At the top of the struct definition, add:

```rust
pub(crate) memory_service_override: Option<alva_agent_memory::MemoryService>,
```

Initialize in `BaseAgentBuilder::new()`:

```rust
memory_service_override: None,
```

**Step 2: Add the override setter**

Somewhere near `with_memory`, add:

```rust
/// Inject a pre-constructed `MemoryService` (overrides the default
/// `MemorySqlite`-backed construction). Implies `enable_memory = true`.
pub fn memory_service(mut self, service: alva_agent_memory::MemoryService) -> Self {
    self.memory_service_override = Some(service);
    self.enable_memory = true;
    self
}
```

**Step 3: Modify the memory construction in `build()`**

Find the block around line 322–332 (the one that currently unconditionally constructs `MemorySqlite`). Change it to:

```rust
let memory = if let Some(service) = self.memory_service_override {
    Some(service)
} else if self.enable_memory {
    let db_dir = workspace.join(".srow");
    tokio::fs::create_dir_all(&db_dir).await?;
    let db_path = db_dir.join("memory.db");
    let store = alva_app_extension_memory::MemorySqlite::open(&db_path).await?;
    let embedder = Box::new(alva_agent_memory::NoopEmbeddingProvider::new());
    Some(alva_agent_memory::MemoryService::with_backend(
        std::sync::Arc::new(store),
        embedder,
    ))
} else {
    None
};
```

**Step 4: Verify and test**

```bash
cargo check -p alva-app-core 2>&1 | tail -10
cargo test -p alva-app-core --tests 2>&1 | grep "^test result" | head -5
```

Expected: PASS. Test count should still be 102.

**Step 5: Commit**

```bash
git add crates/alva-app-core/src/base_agent/builder.rs
git commit -m "feat(base-agent): add memory_service override setter"
```

---

### Task 2.2: Add security middleware override setter

**Files:** Modify `crates/alva-app-core/src/base_agent/builder.rs`.

**Step 1: Add a new field**

```rust
pub(crate) security_middleware_override: Option<Arc<dyn Middleware>>,
```

Initialize to `None` in `new()`.

**Step 2: Add the setter**

```rust
/// Inject a custom middleware in place of the default
/// `SecurityMiddleware::for_workspace(workspace, sandbox_mode)`. Use this
/// when you want fine-grained control over sandboxing or are running in
/// an environment where the built-in sandbox doesn't apply (tests,
/// in-process harness, etc).
pub fn security_middleware(mut self, mw: Arc<dyn Middleware>) -> Self {
    self.security_middleware_override = Some(mw);
    self
}
```

**Step 3: Modify the security block in `build()`**

Find the block around lines 215–218 (where `SecurityMiddleware::for_workspace(...)` is constructed). Change it to:

```rust
let security_mw: Arc<dyn Middleware> = match self.security_middleware_override.take() {
    Some(mw) => mw,
    None => {
        let default = SecurityMiddleware::for_workspace(&workspace, self.sandbox_mode.clone())
            .with_bus(bus_handle.clone());
        security_guard = Some(default.guard());
        Arc::new(default)
    }
};
middleware_stack.push_sorted(security_mw);
```

Note: if the user provided their own middleware, `security_guard` stays `None`. That's fine — it's an opt-out contract.

**Step 4: Verify**

```bash
cargo check -p alva-app-core 2>&1 | tail -10
cargo test -p alva-app-core --tests 2>&1 | grep "^test result" | head -5
```

Expected: PASS. Tests still 102.

**Step 5: Commit**

```bash
git add crates/alva-app-core/src/base_agent/builder.rs
git commit -m "feat(base-agent): add security_middleware override setter"
```

---

### Task 2.3: Delegate BaseAgentBuilder core assembly to AgentBuilder

**Files:** Modify `crates/alva-app-core/src/base_agent/builder.rs` and possibly `agent.rs`.

**Goal:** The bulk of extension collection, tool-registry assembly, and middleware wiring currently in `BaseAgentBuilder::build()` should be **delegated to `alva_agent_core::AgentBuilder::build()`**. BaseAgentBuilder keeps only:
- The preset extension list
- The memory/security wiring (from Tasks 2.1/2.2)
- The workspace/sandbox_mode/enable_memory setters
- Wrapping the resulting `Agent` into a `BaseAgent` with memory/guard fields

**Step 1: Add `alva-agent-core` dep to `alva-app-core/Cargo.toml`**

It should already be there from the earlier refactor (Rule 17's scan confirmed no app/host deps in agent-core — but app-core depending DOWN into agent-core is allowed). Verify with:

```bash
rg '^alva-agent-core' crates/alva-app-core/Cargo.toml
```

If missing, add:

```toml
alva-agent-core = { path = "../alva-agent-core" }
```

**Step 2: Rewrite `BaseAgentBuilder::build()`**

This is the biggest edit in the plan. The new shape:

```rust
pub async fn build(
    self,
    model: Arc<dyn LanguageModel>,
) -> Result<BaseAgent, EngineError> {
    let workspace = self.workspace.ok_or(EngineError::MissingWorkspace)?;

    // --- Preset extensions (our opinionated harness) ---
    let mut agent_builder = alva_agent_core::AgentBuilder::new()
        .model(model)
        .workspace(&workspace)
        .system_prompt(self.system_prompt)
        .max_iterations(self.max_iterations)
        .context_window(self.context_window);

    // Pass through the user's explicit additions
    for ext in self.extensions {
        agent_builder = agent_builder.extension(ext);
    }
    for tool in self.extra_tools {
        agent_builder = agent_builder.tool(tool);
    }
    for mw in self.extra_middleware {
        agent_builder = agent_builder.middleware(mw);
    }

    // --- Harness-level memory wiring (preset with override) ---
    let memory = if let Some(service) = self.memory_service_override {
        Some(service)
    } else if self.enable_memory {
        let db_dir = workspace.join(".srow");
        tokio::fs::create_dir_all(&db_dir).await?;
        let db_path = db_dir.join("memory.db");
        let store = alva_app_extension_memory::MemorySqlite::open(&db_path).await?;
        let embedder = Box::new(alva_agent_memory::NoopEmbeddingProvider::new());
        Some(alva_agent_memory::MemoryService::with_backend(
            std::sync::Arc::new(store),
            embedder,
        ))
    } else {
        None
    };

    // --- Harness-level security wiring (preset with override) ---
    let mut security_guard = None;
    let security_mw: Arc<dyn Middleware> = match self.security_middleware_override {
        Some(mw) => mw,
        None => {
            let default = SecurityMiddleware::for_workspace(&workspace, self.sandbox_mode)
                .with_bus(agent_builder_bus_handle.clone()); // TBD: how to pass bus
            security_guard = Some(default.guard());
            Arc::new(default)
        }
    };
    agent_builder = agent_builder.middleware(security_mw);

    // --- Delegate assembly to AgentBuilder ---
    let inner_agent = agent_builder.build().await
        .map_err(|e| EngineError::AgentBuildFailed(e.to_string()))?;

    Ok(BaseAgent {
        inner: inner_agent,
        memory,
        security_guard,
        workspace,
        // ... other fields as needed
    })
}
```

**Note on the bus handle**: The inner `AgentBuilder` owns the bus. `SecurityMiddleware::with_bus(...)` needs the bus handle BEFORE the builder is built. Two approaches:
- **(A)** Create the bus up-front in `BaseAgentBuilder::build`, pass it to `agent_builder.with_bus(...)` and also to `SecurityMiddleware::with_bus(...)`.
- **(B)** Don't call `with_bus` on the security middleware (let it work without the bus handle).

Prefer (A). Before constructing `agent_builder`, do:

```rust
let bus = Bus::new();
let bus_handle = bus.handle();
let bus_writer = bus.writer();

let mut agent_builder = alva_agent_core::AgentBuilder::new()
    .model(model)
    .with_bus_writer(bus_writer)
    .workspace(&workspace)
    // ...
```

Then `SecurityMiddleware::with_bus(bus_handle.clone())` works.

**Step 3: Rewrite `BaseAgent` struct**

`BaseAgent` in `agent.rs` should become a thin wrapper:

```rust
pub struct BaseAgent {
    pub(super) inner: alva_agent_core::Agent,
    pub(super) memory: Option<alva_agent_memory::MemoryService>,
    pub(super) security_guard: Option<Arc<Mutex<SecurityGuard>>>,
    pub(super) workspace: PathBuf,
    // ... other harness-specific fields (pending_messages, plan_mode, etc.
    //     that used to live here, if still needed)
}

impl BaseAgent {
    pub async fn prompt(
        &self,
        input: Vec<AgentMessage>,
    ) -> Result<mpsc::UnboundedReceiver<AgentEvent>, EngineError> {
        let cancel = CancellationToken::new();
        self.inner.run(input, cancel).await.map_err(Into::into)
    }

    pub fn memory(&self) -> Option<&alva_agent_memory::MemoryService> {
        self.memory.as_ref()
    }

    pub fn security_guard(&self) -> Option<&Arc<Mutex<SecurityGuard>>> {
        self.security_guard.as_ref()
    }

    pub fn workspace(&self) -> &Path { &self.workspace }
}
```

**Be careful**: the current `BaseAgent` has more fields (bus_writer, pending_messages, plan_mode_middleware, bus_plugins, approval_notifier, etc.). Decide for each:
- Fields that are GENERIC — move into `alva_agent_core::Agent` (may need to extend its struct)
- Fields that are HARNESS-specific — keep on `BaseAgent`

If the delegation gets complicated, **STOP and report as DONE_WITH_CONCERNS**. A clean delegation is more important than a tidy-looking refactor.

**Step 4: Fix all callers of `BaseAgent`**

Search for direct field access or method calls on BaseAgent:

```bash
rg 'BaseAgent' crates/alva-app-core/src --type rust
rg 'BaseAgent' crates/alva-app/src --type rust
rg 'BaseAgent' crates/alva-app-cli/src --type rust
rg 'BaseAgent' crates/alva-app-eval/src --type rust
```

For each hit, verify the new API still satisfies it. Expected pain points:
- Code that reads `agent.bus_writer` — may need `agent.inner.bus()` or similar
- Code that calls `agent.prompt_text("...")` — preserve this method on BaseAgent

**Step 5: Verify**

```bash
cargo check --workspace 2>&1 | tail -20
cargo test -p alva-app-core --tests 2>&1 | grep "^test result" | head -10
cargo test --workspace --no-run 2>&1 | tail -20
```

Expected: all pass. If there are 1-2 broken test files in downstream crates, fix them directly.

**Step 6: Commit**

```bash
git add -A
git commit -m "refactor(base-agent): delegate assembly to alva_agent_core::AgentBuilder"
```

---

### Task 2.4: Integration tests for override paths

**Files:** Add test cases to an existing test file in `crates/alva-app-core/tests/` (e.g. `e2e_agent_test.rs`), OR create a new file `crates/alva-app-core/tests/base_agent_overrides.rs`.

**Step 1: Write two tests**

Test A — `memory_service_override_is_used`:
- Construct a `BaseAgentBuilder` with `.workspace(tmp)` and `.memory_service(custom_service)`
- The custom service wraps a fake in-memory backend
- Build the agent
- Call `agent.memory()` — verify it returns the custom service (identity check via a known sentinel value)

Test B — `security_middleware_override_is_used`:
- Define a no-op `Middleware` impl called `NoopSecurity` that counts how many times `on_agent_start` is called
- Construct a `BaseAgentBuilder` with `.workspace(tmp)` and `.security_middleware(Arc::new(NoopSecurity { ... }))`
- Build the agent, prompt it once
- Verify the NoopSecurity counter was incremented (meaning it was actually wired into the middleware stack)

**Step 2: Run**

```bash
cargo test -p alva-app-core --tests 2>&1 | grep "^test result" | head -10
```

Expected: all existing tests pass + 2 new passing tests.

**Step 3: Commit**

```bash
git add -A
git commit -m "test(base-agent): cover memory_service + security_middleware overrides"
```

---

## Phase 3 — Final verification

### Task 3.1: Run the full verification matrix

**Step 1: Workspace build and test**

```bash
cargo check --workspace 2>&1 | tail -10
cargo test --workspace 2>&1 | grep "^test result" | head -30
```

All must pass. Aggregate test counts should equal (previous total) + 4 (2 from Task 1.3, 2 from Task 2.4).

**Step 2: CI script**

```bash
./scripts/ci-check-deps.sh 2>&1 | tail -30
```

Rule 17 must still pass — `alva-agent-core` is forbidden from depending on app/host crates, so the new `AgentBuilder` must not have pulled in anything forbidden.

**Step 3: Wasm invariant**

The ci-check-deps.sh script runs wasm checks for 19 crates. All must still pass.

**Step 4: Commit if anything was patched during verification**

If steps 1–3 revealed a small fix, commit it as a cleanup. Otherwise no commit.

---

## Rollback strategy

Each commit is self-contained and green. To roll back:
- Revert in reverse order (3.x → 2.x → 1.x).
- Phase 1 (Tasks 1.1–1.3) is purely additive — `alva-agent-core` gains new files. Reverting just removes them.
- Phase 2 (Tasks 2.1–2.4) modifies `alva-app-core::BaseAgent`. Reverting Task 2.3 is the most disruptive operation (it's the large refactor).

Between Phase 1 and Phase 2, the workspace is valid: `alva-agent-core::AgentBuilder` exists but nothing uses it yet.

---

## Out-of-scope follow-ups

1. **Deprecate `alva-host-native::AgentRuntimeBuilder`**. It now overlaps with `alva_agent_core::AgentBuilder`. Decide which stays; the other gets `#[deprecated]` annotations pointing at the survivor.
2. **Expose user-facing extension hook for memory**. Right now memory is a `MemoryService` field on `BaseAgent` accessed out-of-band. A cleaner design would expose it to tools and middleware via `ExtensionContext` so extensions that need memory can read/write it uniformly. Future work.
