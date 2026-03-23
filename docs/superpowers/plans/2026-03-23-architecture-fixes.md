# Architecture Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 5 architectural issues identified in the architecture review: Pregel state merging, Provider registry, LLMMessage unification, streaming placeholder, and tool_executor comment cleanup.

**Architecture:** Each fix is independent and targets a specific crate. Fixes are ordered by priority (P0→P1→P2). All changes maintain backward compatibility through defaults.

**Tech Stack:** Rust, tokio, async-trait, serde, alva-types, alva-core, alva-graph, srow-core

---

## File Structure

### Fix 1 (P0): Pregel State Merging
- Modify: `crates/alva-graph/src/graph.rs` — add `set_merge()` to StateGraph, pass merge_fn to CompiledGraph
- Modify: `crates/alva-graph/src/pregel.rs` — use merge_fn in parallel superstep, add tests

### Fix 2 (P1): Provider Registry
- Modify: `crates/srow-core/src/ports/provider/provider_registry.rs` — rebuild with alva_types::LanguageModel
- Modify: `crates/srow-core/src/ports/provider/mod.rs` — re-export new Provider + ProviderRegistry
- Modify: `crates/srow-core/src/lib.rs` — add convenience re-exports

### Fix 3 (P1): LLMMessage → alva_types::Message
- Modify: `crates/srow-core/src/ports/storage.rs` — change trait to use alva_types::Message
- Modify: `crates/srow-core/src/adapters/storage/memory.rs` — use alva_types::Message
- Modify: `crates/srow-core/src/agent/persistence/sqlite.rs` — migrate serialization to alva_types types
- Modify: `crates/srow-core/src/types/llm.rs` — remove LLMMessage re-exports
- Modify: `crates/srow-core/src/lib.rs` — remove LLMMessage-based re-exports
- Modify: `crates/srow-core/src/domain/message.rs` — reduce to ImageSource shim (LLMMessage/LLMContent/Role removed)
- Modify: `crates/alva-types/src/content.rs` — add serde alias for backward-compatible SQLite deserialization

### Fix 4 (P2): Streaming Placeholder
- Modify: `crates/alva-core/src/agent_loop.rs` — build partial Message from accumulated state

### Fix 5 (P2): tool_executor Comment Cleanup
- Modify: `crates/alva-core/src/tool_executor.rs` — clarify the comment as intentional design

---

## Task 1: Pregel Parallel State Merging (P0)

**Files:**
- Modify: `crates/alva-graph/src/graph.rs`
- Modify: `crates/alva-graph/src/pregel.rs`

- [ ] **Step 1: Write failing test for parallel merge**

In `crates/alva-graph/src/pregel.rs`, add this test at the end of `mod tests`:

```rust
#[tokio::test]
async fn parallel_fan_out_with_merge() {
    let mut graph = StateGraph::<serde_json::Value>::new();

    graph.add_node("entry", |s| Box::pin(async { s }));

    graph.add_node("add_a", |s: serde_json::Value| {
        Box::pin(async move {
            let mut s = s;
            s["a"] = serde_json::json!(true);
            s
        })
    });

    graph.add_node("add_b", |s: serde_json::Value| {
        Box::pin(async move {
            let mut s = s;
            s["b"] = serde_json::json!(true);
            s
        })
    });

    graph.add_node("final", |s| Box::pin(async { s }));

    graph.set_entry_point("entry");
    graph.add_edge("entry", "add_a");
    graph.add_edge("entry", "add_b");
    graph.add_edge("add_a", "final");
    graph.add_edge("add_b", "final");
    graph.add_edge("final", END);

    // Merge function: deep-merge JSON objects
    graph.set_merge(|base, outputs| {
        let mut merged = base;
        for output in outputs {
            if let (Some(m), Some(o)) = (merged.as_object_mut(), output.as_object()) {
                for (k, v) in o {
                    m.insert(k.clone(), v.clone());
                }
            }
        }
        merged
    });

    let compiled = graph.compile().unwrap();
    let result = compiled.invoke(serde_json::json!({})).await.unwrap();

    // Both keys must be present — merge combines all node outputs
    assert_eq!(result["a"], true, "key 'a' from parallel node must survive merge");
    assert_eq!(result["b"], true, "key 'b' from parallel node must survive merge");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo test -p alva-graph parallel_fan_out_with_merge`

Expected: Compilation error — `set_merge` method does not exist.

- [ ] **Step 3: Add merge_fn to StateGraph and CompiledGraph**

In `crates/alva-graph/src/graph.rs`, add `merge_fn` field to `StateGraph`:

```rust
pub(crate) type MergeFn<S> = Box<dyn Fn(S, Vec<S>) -> S + Send + Sync>;
```

Add field to `StateGraph<S>`:
```rust
pub struct StateGraph<S> {
    nodes: HashMap<String, NodeFn<S>>,
    edges: Vec<Edge<S>>,
    entry_point: Option<String>,
    merge_fn: Option<MergeFn<S>>,
}
```

Update `new()` to initialize `merge_fn: None`.

Add builder method:
```rust
/// Set a merge function for combining parallel node outputs.
///
/// When multiple nodes execute in a parallel superstep, each receives a
/// clone of the current state. The merge function receives the original
/// base state and a `Vec` of all node outputs, and must return a single
/// combined state.
///
/// If no merge function is set, "last result wins" (nondeterministic).
pub fn set_merge(
    &mut self,
    merge: impl Fn(S, Vec<S>) -> S + Send + Sync + 'static,
) {
    self.merge_fn = Some(Box::new(merge));
}
```

Pass it through in `compile()`:
```rust
Ok(CompiledGraph {
    nodes: self.nodes,
    edges: self.edges,
    entry_point,
    merge_fn: self.merge_fn,
})
```

In `crates/alva-graph/src/pregel.rs`, add field to `CompiledGraph<S>`:
```rust
pub struct CompiledGraph<S> {
    pub(crate) nodes: HashMap<String, NodeFn<S>>,
    pub(crate) edges: Vec<Edge<S>>,
    pub(crate) entry_point: String,
    pub(crate) merge_fn: Option<MergeFn<S>>,
}
```

Update the `Debug` impl to include `has_merge_fn`:
```rust
.field("has_merge_fn", &self.merge_fn.is_some())
```

- [ ] **Step 4: Implement merge logic in invoke()**

In `crates/alva-graph/src/pregel.rs`, replace the parallel superstep block (lines 63-90) with:

```rust
} else {
    // Parallel superstep — execute all nodes concurrently
    let base_state = state.clone();
    let mut join_set = tokio::task::JoinSet::new();
    for node_name in &current_nodes {
        let node_fn = self.nodes.get(node_name).ok_or_else(|| {
            AgentError::ConfigError(format!("Node not found: {}", node_name))
        })?;
        let s = state.clone();
        let fut = node_fn(s);
        let name = node_name.clone();
        join_set.spawn(async move { (name, fut.await) });
    }

    // Collect all results
    let mut outputs = Vec::with_capacity(current_nodes.len());
    while let Some(Ok((_name, result))) = join_set.join_next().await {
        outputs.push(result);
    }

    // Merge results
    state = match &self.merge_fn {
        Some(merge) => merge(base_state, outputs),
        None => {
            // Fallback: last result wins (backward compatible)
            outputs.into_iter().last().unwrap_or(base_state)
        }
    };

    // Collect next nodes from all executed nodes
    let mut next = Vec::new();
    for node in &current_nodes {
        next.extend(self.resolve_next_nodes(node, &state)?);
    }
    next.sort();
    next.dedup();
    current_nodes = next;
}
```

- [ ] **Step 5: Update pregel.rs imports**

In `crates/alva-graph/src/pregel.rs`, update the existing import line 7 to include `MergeFn`:

```rust
use crate::graph::{Edge, MergeFn, NodeFn, END};
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo test -p alva-graph parallel_fan_out_with_merge`

Expected: PASS

- [ ] **Step 7: Run all alva-graph tests**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo test -p alva-graph`

Expected: All tests pass (existing tests should still pass since merge_fn defaults to None).

- [ ] **Step 8: Run workspace check**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo check 2>&1 | head -30`

Expected: No new errors (alva-graph is used by srow-app, verify no downstream breakage).

- [ ] **Step 9: Commit**

```bash
git add crates/alva-graph/src/graph.rs crates/alva-graph/src/pregel.rs
git commit -m "feat(alva-graph): implement merge_fn for parallel superstep state merging

Adds set_merge() to StateGraph, enabling proper state combination when
multiple nodes execute in a parallel BSP superstep. Without a merge
function, behavior falls back to last-result-wins (backward compatible)."
```

---

## Task 2: Provider Registry Rebuild (P1)

**Files:**
- Modify: `crates/srow-core/src/ports/provider/provider_registry.rs`
- Modify: `crates/srow-core/src/ports/provider/mod.rs`
- Modify: `crates/srow-core/src/lib.rs`

- [ ] **Step 1: Add tokio-stream dev-dependency**

Check if `tokio-stream` is in `crates/srow-core/Cargo.toml`. If not, add to `[dev-dependencies]`:

```toml
tokio-stream = "0.1"
```

- [ ] **Step 2: Write the Provider trait and ProviderRegistry**

Replace the contents of `crates/srow-core/src/ports/provider/provider_registry.rs` with:

```rust
use std::collections::HashMap;
use std::sync::Arc;

use alva_types::LanguageModel;

use super::errors::ProviderError;

/// Factory for obtaining model instances by provider+model ID.
///
/// Implementations wrap a specific LLM backend (e.g., OpenAI, Anthropic)
/// and produce `LanguageModel` instances on demand.
pub trait Provider: Send + Sync {
    /// Unique provider identifier (e.g., "openai", "anthropic").
    fn id(&self) -> &str;

    /// Create a language model instance for the given model ID.
    fn language_model(
        &self,
        model_id: &str,
    ) -> Result<Arc<dyn LanguageModel>, ProviderError>;
}

/// Central registry of all available providers.
///
/// Supports lookup by provider ID and a convenience method for
/// `provider_id:model_id` shorthand strings.
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Register a provider. Replaces any existing provider with the same ID.
    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.insert(provider.id().to_string(), provider);
    }

    /// Get a provider by ID.
    pub fn get(&self, provider_id: &str) -> Option<&Arc<dyn Provider>> {
        self.providers.get(provider_id)
    }

    /// Shorthand: obtain a language model from `provider_id:model_id`.
    ///
    /// Returns `ProviderError::NoSuchModel` if the provider is not registered.
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

    /// List all registered provider IDs.
    pub fn provider_ids(&self) -> Vec<&str> {
        self.providers.keys().map(|s| s.as_str()).collect()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::*;
    use async_trait::async_trait;
    use std::pin::Pin;

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
        ) -> Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
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

- [ ] **Step 3: Update mod.rs to re-export**

In `crates/srow-core/src/ports/provider/mod.rs`, add the re-export:

```rust
pub use provider_registry::{Provider, ProviderRegistry};
```

- [ ] **Step 4: Add convenience re-exports in lib.rs**

In `crates/srow-core/src/lib.rs`, add after the existing provider-area re-exports:

```rust
pub use ports::provider::provider_registry::{Provider, ProviderRegistry};
```

- [ ] **Step 5: Run tests**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo test -p srow-core provider_registry`

Expected: All 3 tests pass.

- [ ] **Step 6: Run workspace check**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo check 2>&1 | head -30`

Expected: No new errors.

- [ ] **Step 7: Commit**

```bash
git add crates/srow-core/src/ports/provider/provider_registry.rs crates/srow-core/src/ports/provider/mod.rs crates/srow-core/src/lib.rs
git commit -m "feat(srow-core): rebuild Provider registry on alva_types::LanguageModel

Replaces the commented-out V4 Provider trait with a minimal registry
that produces Arc<dyn LanguageModel> instances. Embedding/image/speech
model factories can be added later as needed."
```

---

## Task 3: Unify LLMMessage → alva_types::Message (P1)

**Files:**
- Modify: `crates/srow-core/src/ports/storage.rs`
- Modify: `crates/srow-core/src/adapters/storage/memory.rs`
- Modify: `crates/srow-core/src/agent/persistence/sqlite.rs`
- Modify: `crates/srow-core/src/types/llm.rs`
- Modify: `crates/srow-core/src/domain/message.rs`
- Modify: `crates/srow-core/src/lib.rs`

- [ ] **Step 1: Add serde alias for backward-compatible deserialization**

The old `LLMContent::ToolResult` serialized as `{"tool_use_id": "..."}` while `ContentBlock::ToolResult` uses `{"id": "..."}`. Existing SQLite data would silently lose content without this alias.

In `crates/alva-types/src/content.rs`, add the serde alias to `ToolResult::id`:

```rust
    #[serde(rename = "tool_result")]
    ToolResult {
        #[serde(alias = "tool_use_id")]
        id: String,
        content: String,
        is_error: bool,
    },
```

This ensures both `"id"` and `"tool_use_id"` are accepted during deserialization, while serialization always uses `"id"`.

- [ ] **Step 2: Update SessionStorage trait**

Replace `crates/srow-core/src/ports/storage.rs` with:

```rust
use alva_types::Message;
use crate::domain::session::{Session, SessionStatus};
use crate::error::EngineError;
use async_trait::async_trait;

/// Abstract session storage interface
#[async_trait]
pub trait SessionStorage: Send + Sync {
    async fn create_session(&self, session: &Session) -> Result<(), EngineError>;
    async fn get_session(&self, id: &str) -> Result<Option<Session>, EngineError>;
    async fn update_session_status(
        &self,
        id: &str,
        status: SessionStatus,
    ) -> Result<(), EngineError>;
    async fn list_sessions(&self, workspace: &str) -> Result<Vec<Session>, EngineError>;
    async fn delete_session(&self, id: &str) -> Result<(), EngineError>;

    async fn append_message(&self, session_id: &str, msg: &Message) -> Result<(), EngineError>;
    async fn get_messages(&self, session_id: &str) -> Result<Vec<Message>, EngineError>;
}
```

- [ ] **Step 3: Update MemoryStorage**

Replace `crates/srow-core/src/adapters/storage/memory.rs` with:

```rust
use alva_types::Message;
use crate::domain::session::{Session, SessionStatus};
use crate::error::EngineError;
use crate::ports::storage::SessionStorage;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::RwLock;

/// In-memory storage backed by HashMap + RwLock
pub struct MemoryStorage {
    sessions: RwLock<HashMap<String, Session>>,
    messages: RwLock<HashMap<String, Vec<Message>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            messages: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionStorage for MemoryStorage {
    async fn create_session(&self, session: &Session) -> Result<(), EngineError> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), session.clone());
        Ok(())
    }

    async fn get_session(&self, id: &str) -> Result<Option<Session>, EngineError> {
        let sessions = self.sessions.read().await;
        Ok(sessions.get(id).cloned())
    }

    async fn update_session_status(
        &self,
        id: &str,
        status: SessionStatus,
    ) -> Result<(), EngineError> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(id) {
            session.status = status;
            Ok(())
        } else {
            Err(EngineError::SessionNotFound(id.to_string()))
        }
    }

    async fn list_sessions(&self, workspace: &str) -> Result<Vec<Session>, EngineError> {
        let sessions = self.sessions.read().await;
        Ok(sessions
            .values()
            .filter(|s| s.workspace == workspace)
            .cloned()
            .collect())
    }

    async fn delete_session(&self, id: &str) -> Result<(), EngineError> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(id);
        let mut messages = self.messages.write().await;
        messages.remove(id);
        Ok(())
    }

    async fn append_message(&self, session_id: &str, msg: &Message) -> Result<(), EngineError> {
        let mut messages = self.messages.write().await;
        messages
            .entry(session_id.to_string())
            .or_default()
            .push(msg.clone());
        Ok(())
    }

    async fn get_messages(&self, session_id: &str) -> Result<Vec<Message>, EngineError> {
        let messages = self.messages.read().await;
        Ok(messages.get(session_id).cloned().unwrap_or_default())
    }
}
```

- [ ] **Step 4: Update SqliteStorage**

In `crates/srow-core/src/agent/persistence/sqlite.rs`:

Replace imports:
```rust
use alva_types::{ContentBlock, Message, MessageRole, UsageMetadata};
use crate::domain::session::{Session, SessionStatus};
use crate::error::EngineError;
use crate::ports::storage::SessionStorage;
```

Remove the old helper functions `role_to_str`, `str_to_role`, `extract_tool_call_id` and replace with:

```rust
fn role_to_str(r: &MessageRole) -> &'static str {
    match r {
        MessageRole::System => "system",
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Tool => "tool",
    }
}

fn str_to_role(s: &str) -> MessageRole {
    match s {
        "system" => MessageRole::System,
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        "tool" => MessageRole::Tool,
        _ => MessageRole::User,
    }
}

fn extract_tool_call_id(content: &[ContentBlock]) -> Option<String> {
    content.iter().find_map(|c| match c {
        ContentBlock::ToolResult { id, .. } => Some(id.clone()),
        _ => None,
    })
}
```

Update `append_message` to accept `&Message`:
```rust
async fn append_message(
    &self,
    session_id: &str,
    msg: &Message,
) -> Result<(), EngineError> {
    let session_id = session_id.to_string();
    let msg_id = msg.id.clone();
    let role = role_to_str(&msg.role).to_string();
    let content_json = serde_json::to_string(&msg.content)
        .map_err(|e| EngineError::Serialization(e.to_string()))?;
    let timestamp = msg.timestamp;
    let token_count = msg.usage.as_ref().map(|u| u.total_tokens as i64);
    let tool_call_id = msg.tool_call_id.clone()
        .or_else(|| extract_tool_call_id(&msg.content));

    self.conn
        .call(move |conn| {
            conn.execute(
                "INSERT INTO messages (session_id, msg_id, role, content_json, turn_index, token_count, tool_call_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![session_id, msg_id, role, content_json, timestamp, token_count, tool_call_id],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| EngineError::Storage(format!("append_message: {e}")))?;
    Ok(())
}
```

Update `get_messages` to return `Vec<Message>`:
```rust
async fn get_messages(&self, session_id: &str) -> Result<Vec<Message>, EngineError> {
    let session_id = session_id.to_string();
    self.conn
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT msg_id, role, content_json, turn_index, token_count, tool_call_id
                 FROM messages WHERE session_id = ?1 ORDER BY id ASC",
            )?;
            let mut rows = stmt.query(rusqlite::params![session_id])?;
            let mut result = Vec::new();
            while let Some(row) = rows.next()? {
                let role_str: String = row.get(1)?;
                let content_str: String = row.get(2)?;
                let timestamp: i64 = row.get(3)?;
                let token_count: Option<i64> = row.get(4)?;
                let tool_call_id: Option<String> = row.get(5)?;
                let content: Vec<ContentBlock> = serde_json::from_str(&content_str)
                    .unwrap_or_default();
                let usage = token_count.map(|t| UsageMetadata {
                    input_tokens: 0,
                    output_tokens: 0,
                    total_tokens: t as u32,
                });
                result.push(Message {
                    id: row.get(0)?,
                    role: str_to_role(&role_str),
                    content,
                    tool_call_id,
                    usage,
                    timestamp,
                });
            }
            Ok(result)
        })
        .await
        .map_err(|e| EngineError::Storage(format!("get_messages: {e}")))
}
```

- [ ] **Step 5: Clean up domain/message.rs**

Replace `crates/srow-core/src/domain/message.rs` with a re-export shim that avoids breaking downstream code during gradual migration:

```rust
// Migrated to alva_types::Message.
// This module is kept temporarily for ImageSource which has no equivalent in alva-types yet.

use serde::{Deserialize, Serialize};

/// Image source type — retained until alva-types adds Image support with source metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageSource {
    Base64,
    Url,
}
```

- [ ] **Step 6: Clean up types/llm.rs**

Replace `crates/srow-core/src/types/llm.rs` with:

```rust
//! LLM-related types — re-exported from alva-types and domain
pub use crate::domain::message::ImageSource;
pub use crate::domain::tool::{ToolCall, ToolDefinition, ToolResult};
```

- [ ] **Step 7: Update lib.rs re-exports**

In `crates/srow-core/src/lib.rs`, the line:
```rust
pub use domain::agent::{AgentConfig, LLMConfig, LLMProviderKind};
```
stays as-is.

No LLMMessage was re-exported from lib.rs, so no change needed there.

- [ ] **Step 8: Update SQLite tests**

Replace the test functions in `crates/srow-core/src/agent/persistence/sqlite.rs` `mod tests`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::{ContentBlock, Message, MessageRole};
    use crate::domain::session::{Session, SessionStatus};

    fn sample_session() -> Session {
        Session {
            id: "sess-001".into(),
            workspace: "/tmp/test".into(),
            agent_config_snapshot: serde_json::json!({"model": "test"}),
            status: SessionStatus::Idle,
            total_tokens: 0,
            iteration_count: 0,
        }
    }

    #[tokio::test]
    async fn test_session_crud() {
        let storage = SqliteStorage::open_in_memory().await.unwrap();
        let session = sample_session();

        storage.create_session(&session).await.unwrap();

        let fetched = storage.get_session("sess-001").await.unwrap().unwrap();
        assert_eq!(fetched.id, "sess-001");
        assert_eq!(fetched.status, SessionStatus::Idle);
        assert_eq!(fetched.workspace, "/tmp/test");

        storage
            .update_session_status("sess-001", SessionStatus::Running)
            .await
            .unwrap();
        let fetched = storage.get_session("sess-001").await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Running);

        let list = storage.list_sessions("/tmp/test").await.unwrap();
        assert_eq!(list.len(), 1);

        storage.delete_session("sess-001").await.unwrap();
        assert!(storage.get_session("sess-001").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_message_append_and_get() {
        let storage = SqliteStorage::open_in_memory().await.unwrap();
        let session = sample_session();
        storage.create_session(&session).await.unwrap();

        let msg1 = Message::user("Hello agent");
        let msg2 = Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text {
                text: "Hi there!".into(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        storage.append_message("sess-001", &msg1).await.unwrap();
        storage.append_message("sess-001", &msg2).await.unwrap();

        let messages = storage.get_messages("sess-001").await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, MessageRole::User);
        assert_eq!(messages[0].text_content(), "Hello agent");
        assert_eq!(messages[1].role, MessageRole::Assistant);
        assert_eq!(messages[1].text_content(), "Hi there!");
    }

    #[tokio::test]
    async fn test_cascade_delete() {
        let storage = SqliteStorage::open_in_memory().await.unwrap();
        let session = sample_session();
        storage.create_session(&session).await.unwrap();

        let msg = Message::user("test");
        storage.append_message("sess-001", &msg).await.unwrap();

        storage.delete_session("sess-001").await.unwrap();
        let messages = storage.get_messages("sess-001").await.unwrap();
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_update_nonexistent_session() {
        let storage = SqliteStorage::open_in_memory().await.unwrap();
        let result = storage
            .update_session_status("nonexistent", SessionStatus::Running)
            .await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 9: Run all srow-core storage tests**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo test -p srow-core -- storage sqlite`

Expected: All tests pass.

- [ ] **Step 10: Build full workspace to check no breakage**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo build 2>&1 | head -50`

Fix any remaining compilation errors from callers of the old `LLMMessage` type.

- [ ] **Step 11: Commit**

```bash
git add crates/alva-types/src/content.rs crates/srow-core/src/ports/storage.rs crates/srow-core/src/adapters/storage/memory.rs crates/srow-core/src/agent/persistence/sqlite.rs crates/srow-core/src/domain/message.rs crates/srow-core/src/types/llm.rs
git commit -m "refactor(srow-core): migrate SessionStorage from LLMMessage to alva_types::Message

Unifies the message type across the stack. The storage layer now uses
alva_types::Message directly, eliminating the dual LLMMessage/Message
representation. Adds serde alias on ContentBlock::ToolResult::id to
accept legacy 'tool_use_id' from existing SQLite data. SQLite schema
is unchanged (turn_index column now stores timestamp as tech debt,
token_count maps to usage.total_tokens)."
```

---

## Task 4: Fix Streaming Placeholder (P2)

**Files:**
- Modify: `crates/alva-core/src/agent_loop.rs`

- [ ] **Step 1: Write test for partial message in MessageUpdate**

In `crates/alva-core/src/agent_loop.rs` `mod tests`, add:

```rust
#[tokio::test]
async fn test_streaming_emits_partial_message() {
    struct PartialStreamModel;

    #[async_trait]
    impl LanguageModel for PartialStreamModel {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Result<Message, AgentError> {
            panic!("should use stream path")
        }

        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
            Box::pin(tokio_stream::iter(vec![
                StreamEvent::Start,
                StreamEvent::TextDelta { text: "Hello ".into() },
                StreamEvent::TextDelta { text: "world!".into() },
                StreamEvent::Done,
            ]))
        }

        fn model_id(&self) -> &str {
            "partial-stream-mock"
        }
    }

    let config = AgentConfig::new(Arc::new(default_convert_to_llm));
    let cancel = CancellationToken::new();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    let mut state = AgentState::new("test".into(), ModelConfig::default());
    state.is_streaming = true;
    state.messages.push(AgentMessage::Standard(Message::user("Hi")));

    let result = run_agent_loop(
        &mut state,
        &PartialStreamModel,
        &config,
        &cancel,
        &event_tx,
    )
    .await;
    assert!(result.is_ok());

    drop(event_tx);
    let mut partial_texts = Vec::new();
    while let Some(ev) = event_rx.recv().await {
        if let AgentEvent::MessageUpdate {
            message: AgentMessage::Standard(m),
            ..
        } = &ev
        {
            partial_texts.push(m.text_content());
        }
    }

    // After "Hello " delta, partial should contain "Hello "
    // After "world!" delta, partial should contain "Hello world!"
    assert!(
        partial_texts.iter().any(|t| t == "Hello "),
        "should have partial with 'Hello '"
    );
    assert!(
        partial_texts.iter().any(|t| t == "Hello world!"),
        "should have partial with 'Hello world!'"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo test -p alva-core test_streaming_emits_partial_message`

Expected: FAIL — partial messages are `Custom { type_name: "stream_placeholder" }`, not `Standard(Message)`.

- [ ] **Step 3: Fix stream_llm_response to emit partial messages**

In `crates/alva-core/src/agent_loop.rs`, replace the `MessageUpdate` emit block (around line 279-286) with:

```rust
        // Build partial message from accumulated state so far
        let mut partial_content = Vec::new();
        if !text.is_empty() {
            partial_content.push(ContentBlock::Text { text: text.clone() });
        }
        if !reasoning.is_empty() {
            partial_content.push(ContentBlock::Reasoning { text: reasoning.clone() });
        }
        for acc in &tool_call_accumulators {
            let input: serde_json::Value = serde_json::from_str(&acc.arguments_json)
                .unwrap_or(serde_json::Value::String(acc.arguments_json.clone()));
            partial_content.push(ContentBlock::ToolUse {
                id: acc.id.clone(),
                name: acc.name.clone(),
                input,
            });
        }

        let partial_message = Message {
            id: String::new(),
            role: MessageRole::Assistant,
            content: partial_content,
            tool_call_id: None,
            usage: usage.clone(),
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        let _ = event_tx.send(AgentEvent::MessageUpdate {
            message: AgentMessage::Standard(partial_message),
            delta: event,
        });
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo test -p alva-core test_streaming_emits_partial_message`

Expected: PASS

- [ ] **Step 5: Run all alva-core tests**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo test -p alva-core`

Expected: All tests pass.

- [ ] **Step 6: Run workspace check**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo check 2>&1 | head -30`

Expected: No new errors.

- [ ] **Step 7: Commit**

```bash
git add crates/alva-core/src/agent_loop.rs
git commit -m "fix(alva-core): emit partial Message in streaming MessageUpdate events

Replaces the stream_placeholder with a real partial Message built from
accumulated text/reasoning/tool_call deltas. UI can now reconstruct
message state from streaming events without waiting for MessageEnd."
```

---

## Task 5: Clean Up tool_executor Comment (P2)

**Files:**
- Modify: `crates/alva-core/src/tool_executor.rs`

- [ ] **Step 1: Replace the misleading comment**

In `crates/alva-core/src/tool_executor.rs`, replace lines 116-121:

```rust
            // Note: after_tool_call hook cannot access AgentContext in the
            // spawned task (not Send). We apply it here with a captured clone
            // if the closure is Send+Sync (which it is by our type alias).
            // However, the context reference cannot be sent across tasks, so
            // we skip the after_hook in the spawned task and apply it after
            // the join below. For now, we return a pair.
```

with:

```rust
            // Design note: after_tool_call hooks run on the main task after
            // all parallel tools complete (see loop below). This is intentional —
            // hooks process individual tool results and don't need to see
            // results from sibling tools. AgentContext stays on the main task
            // to avoid Send requirements on borrowed references.
```

- [ ] **Step 2: Run alva-core tests**

Run: `cd /Users/smallraw/Development/QuincyWork/srow-agent && cargo test -p alva-core`

Expected: All tests pass (comment-only change).

- [ ] **Step 3: Commit**

```bash
git add crates/alva-core/src/tool_executor.rs
git commit -m "docs(alva-core): clarify parallel tool hook design as intentional

The after_tool_call hooks running after join is a deliberate design
choice, not a limitation to be fixed. Updated comment to reflect this."
```
