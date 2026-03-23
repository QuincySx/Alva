# P1-P3 Architecture Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix MiddlewareContext persistence (P1), integrate alva-graph into alva-runtime (P2), add SecurityMiddleware in alva-runtime (P2), and add Provider conformance test harness (P3).

**Architecture:** P1 threads a single `MiddlewareContext` through the entire agent run so Extensions persist across hooks. P2 adds graph support to the runtime builder. Domain-specific middleware lives in alva-runtime (which already depends on all foundation crates). P3 provides a test macro for Provider implementors.

**Tech Stack:** Rust, async-trait, tokio, alva-core middleware, alva-graph, alva-security

---

## File Structure

### Modified files:
```
crates/alva-core/src/agent_loop.rs           ← P1: thread MiddlewareContext through entire run
crates/alva-core/src/tool_executor.rs         ← P1: accept &mut MiddlewareContext from caller
crates/alva-core/src/middleware/mod.rs         ← P1: MiddlewareContext gains Default, update tests
```

### New files:
```
crates/alva-runtime/src/middleware/mod.rs      ← P2: domain-specific middleware module
crates/alva-runtime/src/middleware/security.rs ← P2: SecurityMiddleware wrapping alva-security
crates/alva-runtime/src/graph.rs              ← P2: graph builder integration
crates/alva-runtime/Cargo.toml               ← P2: add alva-graph dep
crates/alva-types/src/provider_test.rs        ← P3: conformance test helpers
crates/alva-types/src/lib.rs                  ← P3: add provider_test module export
```

---

## Task 1: Persistent MiddlewareContext across agent run (P1)

**Rationale:** Currently `build_mw_ctx()` creates a fresh `MiddlewareContext` (with `Extensions::new()`) at every hook call site. Extensions set in `on_agent_start` are invisible to `before_llm_call`. DeerFlow threads a single context through its entire middleware chain.

**Files:**
- Modify: `crates/alva-core/src/agent_loop.rs`
- Modify: `crates/alva-core/src/tool_executor.rs`

- [ ] **Step 1.1: Create persistent MiddlewareContext in run_agent_loop**

In `crates/alva-core/src/agent_loop.rs`, create a single `MiddlewareContext` at the top of `run_agent_loop` and pass it by `&mut` to all hook call sites:

```rust
pub(crate) async fn run_agent_loop(
    state: &mut AgentState,
    model: &dyn LanguageModel,
    config: &AgentHooks,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), alva_types::AgentError> {
    let _ = event_tx.send(AgentEvent::AgentStart);

    // Create a SINGLE MiddlewareContext that persists across the entire run.
    // Extensions survive between hooks — middleware can store state in on_agent_start
    // and read it in before_llm_call, etc.
    let mut mw_ctx = MiddlewareContext {
        session_id: state.tool_context.session_id().to_string(),
        system_prompt: state.system_prompt.clone(),
        messages: state.messages.clone(),
        extensions: Extensions::new(),
    };

    // Middleware: on_agent_start
    if !config.middleware.is_empty() {
        if let Err(e) = config.middleware.run_on_agent_start(&mut mw_ctx).await {
            warn!(error = %e, "middleware on_agent_start failed");
        }
    }

    let result = run_agent_loop_inner(state, model, config, cancel, event_tx, &mut mw_ctx).await;

    // Middleware: on_agent_end
    if !config.middleware.is_empty() {
        // Sync messages to context before on_agent_end
        mw_ctx.messages = state.messages.clone();
        let err_str = result.as_ref().err().map(|e| e.to_string());
        if let Err(e) = config.middleware.run_on_agent_end(&mut mw_ctx, err_str.as_deref()).await {
            warn!(error = %e, "middleware on_agent_end failed");
        }
    }

    // ... emit AgentEnd event ...
}
```

- [ ] **Step 1.2: Thread mw_ctx into run_agent_loop_inner**

Add `mw_ctx: &mut MiddlewareContext` parameter to `run_agent_loop_inner`. At each hook call site, sync `mw_ctx.messages` from `state.messages` before calling middleware, then use `mw_ctx` directly instead of `build_mw_ctx(state)`:

```rust
async fn run_agent_loop_inner(
    state: &mut AgentState,
    model: &dyn LanguageModel,
    config: &AgentHooks,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    mw_ctx: &mut MiddlewareContext,  // NEW
) -> Result<(), alva_types::AgentError> {
    // ... inside inner loop ...

    // 1b. Middleware: before_llm_call
    if !config.middleware.is_empty() {
        mw_ctx.messages = state.messages.clone(); // sync
        if let Err(e) = config.middleware.run_before_llm_call(mw_ctx, &mut llm_messages).await {
            warn!(error = %e, "middleware before_llm_call failed");
        }
    }

    // ... after model call ...

    // 3b. Middleware: after_llm_call
    if !config.middleware.is_empty() {
        mw_ctx.messages = state.messages.clone(); // sync
        if let Err(e) = config.middleware.run_after_llm_call(mw_ctx, &mut assistant_message).await {
            warn!(error = %e, "middleware after_llm_call failed");
        }
    }
```

- [ ] **Step 1.3: Thread mw_ctx into execute_tools**

In `crates/alva-core/src/tool_executor.rs`, add `mw_ctx: &mut MiddlewareContext` parameter to `execute_tools`, `execute_parallel`, and `execute_sequential`. Remove `build_mw_ctx_from_context` helper. Use the passed-in `mw_ctx` directly.

**Note on execute_parallel:** `&mut MiddlewareContext` is safe here because middleware hooks run on the **main task** only — `before_tool_call` runs before each `join_set.spawn()`, and `after_tool_call` runs after each `join_set.join_next()`. The mw_ctx reference never enters a spawned future, so there are no Send/lifetime issues.

```rust
pub(crate) async fn execute_tools(
    tool_calls: &[ToolCall],
    tools: &[Arc<dyn Tool>],
    config: &AgentHooks,
    context: &AgentContext<'_>,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    tool_context: &Arc<dyn ToolContext>,
    mw_ctx: &mut MiddlewareContext,  // NEW
) -> Vec<ToolResult> {
```

Update the call site in `agent_loop.rs` to pass `mw_ctx`.

- [ ] **Step 1.4: Remove build_mw_ctx and build_mw_ctx_from_context helpers**

Delete `fn build_mw_ctx(state: &AgentState)` from agent_loop.rs and `fn build_mw_ctx_from_context(...)` from tool_executor.rs. They're no longer needed.

- [ ] **Step 1.5: Write test for cross-hook Extensions persistence**

Add to `crates/alva-core/src/agent_loop.rs` tests:

```rust
#[tokio::test]
async fn test_middleware_extensions_persist_across_hooks() {
    // A middleware that sets a value in on_agent_start and reads it in before_llm_call
    struct PersistenceMiddleware {
        observed: Arc<parking_lot::Mutex<Option<u32>>>,
    }

    #[derive(Debug)]
    struct Budget(u32);

    #[async_trait]
    impl Middleware for PersistenceMiddleware {
        async fn on_agent_start(&self, ctx: &mut MiddlewareContext) -> Result<(), MiddlewareError> {
            ctx.extensions.insert(Budget(999));
            Ok(())
        }
        async fn before_llm_call(&self, ctx: &mut MiddlewareContext, _msgs: &mut Vec<Message>) -> Result<(), MiddlewareError> {
            if let Some(b) = ctx.extensions.get::<Budget>() {
                *self.observed.lock() = Some(b.0);
            }
            Ok(())
        }
    }

    let observed = Arc::new(parking_lot::Mutex::new(None));
    let mw = PersistenceMiddleware { observed: observed.clone() };

    let mut config = AgentHooks::new(Arc::new(default_convert_to_llm));
    config.middleware.push(Arc::new(mw));

    let cancel = CancellationToken::new();
    let (event_tx, _event_rx) = mpsc::unbounded_channel();
    let mut state = AgentState::new("test".to_string(), ModelConfig::default());
    state.messages.push(AgentMessage::Standard(Message::user("Hi")));

    let _ = run_agent_loop(&mut state, &MockModel, &config, &cancel, &event_tx).await;

    // on_agent_start set Budget(999), before_llm_call should have seen it
    assert_eq!(*observed.lock(), Some(999), "Extensions should persist across hooks");
}
```

- [ ] **Step 1.6: Verify compilation and tests**

Run: `cargo test -p alva-core`
Expected: all existing tests pass + new persistence test passes.

Run: `cargo check --workspace`
Expected: compiles.

- [ ] **Step 1.7: Commit**

```bash
git add crates/alva-core/
git commit -m "fix(alva-core): persist MiddlewareContext across entire agent run for cross-hook Extensions"
```

---

## Task 2: SecurityMiddleware in alva-runtime (P2)

**Rationale:** alva-security provides `SecurityGuard` but it's not integrated as middleware. A `SecurityMiddleware` wrapping SecurityGuard in the middleware system enables composable security. It lives in alva-runtime (which already depends on alva-security).

**Files:**
- Create: `crates/alva-runtime/src/middleware/mod.rs`
- Create: `crates/alva-runtime/src/middleware/security.rs`
- Modify: `crates/alva-runtime/src/lib.rs`
- Modify: `crates/alva-runtime/Cargo.toml`

- [ ] **Step 2.1: Create middleware module in alva-runtime**

Create `crates/alva-runtime/src/middleware/mod.rs`:
```rust
//! Domain-specific middleware implementations.
//!
//! These live in alva-runtime (not alva-core) because they depend on
//! domain crates (alva-security, alva-memory) that alva-core cannot import.

pub mod security;

pub use security::SecurityMiddleware;
```

- [ ] **Step 2.2: Create SecurityMiddleware**

Create `crates/alva-runtime/src/middleware/security.rs`:

```rust
//! Security middleware — wraps alva-security's SecurityGuard as an async Middleware.

use std::sync::Arc;

use alva_core::middleware::{Middleware, MiddlewareContext, MiddlewareError};
use alva_security::{SecurityGuard, SecurityDecision, SandboxMode};
use alva_types::{ToolCall, ToolContext};
use async_trait::async_trait;
use tokio::sync::Mutex;

/// Middleware that delegates tool-call permission checks to a SecurityGuard.
///
/// Blocks tool execution if the SecurityGuard returns Deny. For NeedHumanApproval,
/// blocks with a message explaining the tool needs approval.
pub struct SecurityMiddleware {
    guard: Arc<Mutex<SecurityGuard>>,
}

impl SecurityMiddleware {
    pub fn new(guard: SecurityGuard) -> Self {
        Self {
            guard: Arc::new(Mutex::new(guard)),
        }
    }

    pub fn for_workspace(workspace: impl Into<std::path::PathBuf>, mode: SandboxMode) -> Self {
        Self::new(SecurityGuard::new(workspace.into(), mode))
    }
}

#[async_trait]
impl Middleware for SecurityMiddleware {
    async fn before_tool_call(
        &self,
        _ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        tool_context: &dyn ToolContext,
    ) -> Result<(), MiddlewareError> {
        let mut guard = self.guard.lock().await;
        match guard.check_tool_call(&tool_call.name, &tool_call.arguments, tool_context) {
            SecurityDecision::Allow => Ok(()),
            SecurityDecision::Deny { reason } => Err(MiddlewareError::Blocked { reason }),
            SecurityDecision::NeedHumanApproval { request_id } => {
                Err(MiddlewareError::Blocked {
                    reason: format!(
                        "tool '{}' requires human approval (request: {})",
                        tool_call.name, request_id
                    ),
                })
            }
        }
    }

    fn name(&self) -> &str {
        "security"
    }
}
```

- [ ] **Step 2.3: Update alva-runtime Cargo.toml**

Ensure `alva-security` is already in deps (it should be). Add `tokio` with `sync` feature if not present.

- [ ] **Step 2.4: Update alva-runtime lib.rs**

Add:
```rust
pub mod middleware;
pub use middleware::SecurityMiddleware;
```

- [ ] **Step 2.5: Write test**

Add test in `security.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alva_core::middleware::{Extensions, MiddlewareContext};

    #[tokio::test]
    async fn blocks_sensitive_path() {
        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen);
        let mut ctx = MiddlewareContext {
            session_id: "test".into(),
            system_prompt: String::new(),
            messages: vec![],
            extensions: Extensions::new(),
        };
        let tool_call = ToolCall {
            id: "1".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({ "path": "/etc/passwd" }),
        };
        let tool_ctx = alva_types::EmptyToolContext;
        let result = mw.before_tool_call(&mut ctx, &tool_call, &tool_ctx).await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2.6: Verify and commit**

Run: `cargo test -p alva-runtime && cargo check --workspace`

```bash
git add crates/alva-runtime/
git commit -m "feat(alva-runtime): add SecurityMiddleware wrapping alva-security's SecurityGuard"
```

---

## Task 3: Integrate alva-graph into alva-runtime (P2)

**Rationale:** alva-graph provides StateGraph/Pregel/AgentSession but alva-runtime's builder doesn't expose graph construction. Users should be able to build graph-based agents through the runtime.

**Files:**
- Create: `crates/alva-runtime/src/graph.rs`
- Modify: `crates/alva-runtime/src/lib.rs`
- Modify: `crates/alva-runtime/Cargo.toml`

- [ ] **Step 3.1: Add alva-graph dependency**

In `crates/alva-runtime/Cargo.toml`:
```toml
alva-graph = { path = "../alva-graph" }
```

- [ ] **Step 3.2: Create graph.rs with re-exports and convenience API**

Create `crates/alva-runtime/src/graph.rs`:

```rust
//! Graph-based agent orchestration — re-exports from alva-graph with runtime integration.

// Re-export core graph types
pub use alva_graph::{
    StateGraph, CompiledGraph, AgentSession,
    CheckpointSaver, InMemoryCheckpointSaver,
    CompactionConfig, RetryConfig,
    SubAgentConfig, SubAgentModel, SubAgentTools,
    ContextTransform, TransformPipeline,
    START, END,
};
```

- [ ] **Step 3.3: Update lib.rs**

Add:
```rust
pub mod graph;
```

- [ ] **Step 3.4: Update CI dependency firewall**

In `scripts/ci-check-deps.sh`, update the alva-runtime rule to also allow `alva-graph`:

The rule currently checks alva-runtime only depends on `agent-{types,core,tools,security,memory}`. Add `alva-graph` to the allowed list.

- [ ] **Step 3.5: Verify and commit**

Run: `cargo check -p alva-runtime && ./scripts/ci-check-deps.sh`

```bash
git add crates/alva-runtime/ scripts/ci-check-deps.sh
git commit -m "feat(alva-runtime): integrate alva-graph for StateGraph/Pregel orchestration"
```

---

## Task 4: Provider Conformance Test Helpers (P3)

**Rationale:** LangChain has `standard-tests/` ensuring all Provider packages implement the same interface. We need a similar test harness so any `Provider` implementation can verify conformance.

**Files:**
- Create: `crates/alva-types/src/provider_test.rs`
- Modify: `crates/alva-types/src/lib.rs`

- [ ] **Step 4.1: Create provider conformance test module**

Create `crates/alva-types/src/provider_test.rs`:

```rust
//! Provider conformance test helpers.
//!
//! Provider implementations can use these functions to verify they correctly
//! implement the Provider trait contract.
//!
//! # Usage (in your provider crate's tests)
//!
//! ```rust,ignore
//! use alva_types::provider_test;
//!
//! #[test]
//! fn conformance() {
//!     let provider = MyProvider::new(api_key);
//!     provider_test::assert_provider_id_non_empty(&provider);
//!     provider_test::assert_language_model_returns_valid_id(&provider, "my-model");
//!     provider_test::assert_unsupported_models_return_error(&provider);
//! }
//! ```

use crate::{Provider, ProviderError};

/// Assert that `provider.id()` returns a non-empty string.
pub fn assert_provider_id_non_empty(provider: &dyn Provider) {
    let id = provider.id();
    assert!(!id.is_empty(), "Provider.id() must return a non-empty string");
    assert!(
        !id.contains(' '),
        "Provider.id() should not contain spaces, got: '{}'", id
    );
}

/// Assert that requesting a known model returns a valid LanguageModel with matching id.
pub fn assert_language_model_returns_valid_id(provider: &dyn Provider, model_id: &str) {
    match provider.language_model(model_id) {
        Ok(model) => {
            let returned_id = model.model_id();
            assert!(
                !returned_id.is_empty(),
                "LanguageModel.model_id() must return a non-empty string"
            );
        }
        Err(e) => {
            panic!(
                "Provider '{}' should support model '{}', but got error: {}",
                provider.id(), model_id, e
            );
        }
    }
}

/// Assert that requesting a nonsense model returns NoSuchModel error.
pub fn assert_unknown_model_returns_error(provider: &dyn Provider) {
    let result = provider.language_model("__nonexistent_model_xyz__");
    assert!(
        matches!(result, Err(ProviderError::NoSuchModel { .. })),
        "Provider '{}' should return NoSuchModel for unknown model, got: {:?}",
        provider.id(), result
    );
}

/// Assert that unsupported capability methods return UnsupportedFunctionality.
pub fn assert_unsupported_models_return_error(provider: &dyn Provider) {
    // Test each optional capability — if the provider doesn't support it,
    // the default impl returns UnsupportedFunctionality.
    let checks = [
        ("embedding", provider.embedding_model("test").err()),
        ("transcription", provider.transcription_model("test").err()),
        ("speech", provider.speech_model("test").err()),
        ("image", provider.image_model("test").err()),
        ("video", provider.video_model("test").err()),
        ("reranking", provider.reranking_model("test").err()),
        ("moderation", provider.moderation_model("test").err()),
    ];

    for (capability, error) in checks {
        if let Some(err) = error {
            // If it errors, it should be either UnsupportedFunctionality or NoSuchModel
            assert!(
                matches!(err, ProviderError::UnsupportedFunctionality(_) | ProviderError::NoSuchModel { .. }),
                "Provider '{}' returned unexpected error for {} capability: {}",
                provider.id(), capability, err
            );
        }
        // If it succeeds, the provider supports that capability — that's fine too.
    }
}

/// Run all basic conformance checks for a provider.
///
/// `known_model_id` should be a model ID that the provider is expected to support.
pub fn assert_provider_conformance(provider: &dyn Provider, known_model_id: &str) {
    assert_provider_id_non_empty(provider);
    assert_language_model_returns_valid_id(provider, known_model_id);
    assert_unknown_model_returns_error(provider);
    assert_unsupported_models_return_error(provider);
}
```

- [ ] **Step 4.2: Update alva-types lib.rs**

Add:
```rust
pub mod provider_test;
```

- [ ] **Step 4.3: Verify and commit**

Run: `cargo check -p alva-types && cargo test -p alva-types`

```bash
git add crates/alva-types/
git commit -m "feat(alva-types): add Provider conformance test helpers"
```

---

## Execution Order

| Task | Name | Priority | Depends On |
|------|------|----------|------------|
| 1 | Persistent MiddlewareContext | P1 | — |
| 2 | SecurityMiddleware | P2 | — |
| 3 | alva-graph integration | P2 | — |
| 4 | Provider conformance tests | P3 | — |

Tasks 1 and 4 are fully independent. **Tasks 2 and 3 both modify `alva-runtime/src/lib.rs`** — run them sequentially (2 before 3, or vice versa) to avoid merge conflicts.

Also consider re-exporting compaction utility functions (`compact_messages`, `estimate_tokens`, `should_compact`) in Task 3's graph.rs, as graph users will likely need them.
