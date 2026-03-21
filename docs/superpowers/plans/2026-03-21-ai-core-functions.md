# AI Core Functions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add AI SDK-style top-level functions (`generate_text`, `stream_text`, `agent`) and `Output` abstraction to srow-ai, wrapping the existing `AgentEngine`.

**Architecture:** Three new modules in srow-ai: `generate` (top-level API functions), `output` (structured output), `stop_condition` (loop control). These wrap `AgentEngine` from srow-core, providing a clean functional API. `generateObject`/`streamObject` are skipped (deprecated in AI SDK — use `generate_text` + `Output::object()` instead). `Agent` is a thin config wrapper around `generate_text`/`stream_text`.

**Tech Stack:** Rust, tokio, serde, serde_json, futures, async-trait

---

## File Map

### New files — srow-ai

| File | Responsibility |
|------|---------------|
| `src/srow-ai/src/generate/mod.rs` | Module declarations + re-exports |
| `src/srow-ai/src/generate/types.rs` | CallSettings, Prompt, GenerateTextResult, StepResult, StreamTextResult |
| `src/srow-ai/src/generate/generate_text.rs` | `generate_text()` — non-streaming top-level API |
| `src/srow-ai/src/generate/stream_text.rs` | `stream_text()` — streaming top-level API |
| `src/srow-ai/src/generate/agent.rs` | `Agent` struct — config wrapper |
| `src/srow-ai/src/generate/stop_condition.rs` | `StopCondition` trait + `step_count_is()`, `has_tool_call()` |
| `src/srow-ai/src/generate/output.rs` | `Output` trait + `TextOutput`, `ObjectOutput` |
| `src/srow-ai/tests/generate_text_test.rs` | generate_text tests with mock LLM |
| `src/srow-ai/tests/stream_text_test.rs` | stream_text tests |
| `src/srow-ai/tests/agent_test.rs` | Agent tests |

### Modified files

| File | Change |
|------|--------|
| `src/srow-ai/src/lib.rs` | Add `pub mod generate;` |

---

## Task 1: StopCondition + Output abstractions

**Files:**
- Create: `src/srow-ai/src/generate/mod.rs`
- Create: `src/srow-ai/src/generate/stop_condition.rs`
- Create: `src/srow-ai/src/generate/output.rs`
- Modify: `src/srow-ai/src/lib.rs`

- [ ] **Step 1: Create stop_condition.rs**

```rust
// src/srow-ai/src/generate/stop_condition.rs

use super::types::StepResult;

/// Determines when the agentic loop should stop.
pub trait StopCondition: Send + Sync {
    fn should_stop(&self, steps: &[StepResult]) -> bool;
}

/// Stop after exactly N steps.
pub struct StepCountIs(pub u32);

impl StopCondition for StepCountIs {
    fn should_stop(&self, steps: &[StepResult]) -> bool {
        steps.len() as u32 >= self.0
    }
}

/// Stop when the last step contains a call to the named tool.
pub struct HasToolCall(pub String);

impl StopCondition for HasToolCall {
    fn should_stop(&self, steps: &[StepResult]) -> bool {
        steps.last().map_or(false, |step| {
            step.tool_calls.iter().any(|tc| tc.name == self.0)
        })
    }
}

/// Convenience constructors
pub fn step_count_is(n: u32) -> Box<dyn StopCondition> {
    Box::new(StepCountIs(n))
}

pub fn has_tool_call(name: impl Into<String>) -> Box<dyn StopCondition> {
    Box::new(HasToolCall(name.into()))
}
```

- [ ] **Step 2: Create output.rs**

```rust
// src/srow-ai/src/generate/output.rs

use serde::de::DeserializeOwned;
use srow_core::error::ChatError;

/// Output format specification — controls how LLM output is parsed.
/// Equivalent to AI SDK's `Output` class.
pub trait Output: Send + Sync {
    type Complete;
    type Partial: Clone;

    /// Name of this output format (for logging/debugging)
    fn name(&self) -> &str;

    /// JSON Schema to send to the LLM as `response_format`, or None for plain text.
    fn json_schema(&self) -> Option<serde_json::Value> { None }

    /// Parse complete text output into the target type.
    fn parse_complete(&self, text: &str) -> Result<Self::Complete, ChatError>;

    /// Parse partial (streaming) text into a partial value. Returns None if not enough data yet.
    fn parse_partial(&self, text: &str) -> Option<Self::Partial>;
}

/// Plain text output — no parsing, returns the raw text.
pub struct TextOutput;

impl Output for TextOutput {
    type Complete = String;
    type Partial = String;

    fn name(&self) -> &str { "text" }

    fn parse_complete(&self, text: &str) -> Result<String, ChatError> {
        Ok(text.to_string())
    }

    fn parse_partial(&self, text: &str) -> Option<String> {
        Some(text.to_string())
    }
}

/// JSON object output — parses text as JSON, validates against expected type.
pub struct ObjectOutput<T: DeserializeOwned + Clone + Send + Sync> {
    schema: Option<serde_json::Value>,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: DeserializeOwned + Clone + Send + Sync> ObjectOutput<T> {
    pub fn new() -> Self {
        Self { schema: None, _phantom: std::marker::PhantomData }
    }

    pub fn with_schema(mut self, schema: serde_json::Value) -> Self {
        self.schema = Some(schema);
        self
    }
}

impl<T: DeserializeOwned + Clone + Send + Sync + 'static> Output for ObjectOutput<T> {
    type Complete = T;
    type Partial = serde_json::Value;

    fn name(&self) -> &str { "object" }

    fn json_schema(&self) -> Option<serde_json::Value> {
        self.schema.clone()
    }

    fn parse_complete(&self, text: &str) -> Result<T, ChatError> {
        serde_json::from_str(text).map_err(|e| ChatError::Serialization(e.to_string()))
    }

    fn parse_partial(&self, text: &str) -> Option<serde_json::Value> {
        serde_json::from_str(text).ok()
    }
}
```

- [ ] **Step 3: Create types.rs (shared types)**

```rust
// src/srow-ai/src/generate/types.rs

use std::sync::Arc;
use srow_core::domain::message::LLMMessage;
use srow_core::domain::tool::{ToolCall, ToolDefinition, ToolResult};
use srow_core::ports::llm_provider::{LLMProvider, StopReason, TokenUsage as LLMTokenUsage};
use srow_core::ports::tool::ToolRegistry;
use srow_core::ui_message::{UIMessage, UIMessagePart};
use srow_core::ui_message_stream::{UIMessageChunk, FinishReason, TokenUsage};
use super::stop_condition::StopCondition;

/// Common call settings for generate_text / stream_text
pub struct CallSettings {
    pub model: Arc<dyn LLMProvider>,
    pub system: Option<String>,
    pub tools: Option<Arc<ToolRegistry>>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stop_when: Option<Box<dyn StopCondition>>,
    pub max_retries: u32,
    pub workspace: std::path::PathBuf,
}

impl Default for CallSettings {
    fn default() -> Self {
        Self {
            model: panic!("model is required"),  // Will be overridden by builder
            system: None,
            tools: None,
            max_output_tokens: Some(8192),
            temperature: None,
            stop_when: None,
            max_retries: 2,
            workspace: std::path::PathBuf::from("."),
        }
    }
}

/// Input prompt — can be a simple string or message array
pub enum Prompt {
    Text(String),
    Messages(Vec<UIMessage>),
}

/// Result of a single step in the agentic loop
#[derive(Debug, Clone)]
pub struct StepResult {
    pub text: String,
    pub reasoning: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub tool_results: Vec<ToolResult>,
    pub finish_reason: FinishReason,
    pub usage: TokenUsage,
}

/// Result of generate_text
pub struct GenerateTextResult<T = String> {
    /// Final generated text
    pub text: String,
    /// Final reasoning text (if model supports thinking)
    pub reasoning: Option<String>,
    /// Tool calls from the final step
    pub tool_calls: Vec<ToolCall>,
    /// Tool results from the final step
    pub tool_results: Vec<ToolResult>,
    /// Why generation stopped
    pub finish_reason: FinishReason,
    /// Token usage for the final step
    pub usage: TokenUsage,
    /// Cumulative token usage across all steps
    pub total_usage: TokenUsage,
    /// All steps executed
    pub steps: Vec<StepResult>,
    /// Complete message history
    pub response_messages: Vec<LLMMessage>,
    /// Structured output (if Output was specified)
    pub output: Option<T>,
}

/// Result of stream_text (returned synchronously, fields resolve as stream progresses)
pub struct StreamTextResult {
    /// Receive UIMessageChunk events
    pub chunk_rx: tokio::sync::mpsc::UnboundedReceiver<UIMessageChunk>,
    /// Resolves to final text when stream completes
    pub text: tokio::sync::oneshot::Receiver<String>,
    /// Resolves to all steps when stream completes
    pub steps: tokio::sync::oneshot::Receiver<Vec<StepResult>>,
    /// Resolves to total usage when stream completes
    pub total_usage: tokio::sync::oneshot::Receiver<TokenUsage>,
    /// Resolves to finish reason when stream completes
    pub finish_reason: tokio::sync::oneshot::Receiver<FinishReason>,
}
```

- [ ] **Step 4: Create generate/mod.rs**

```rust
pub mod types;
pub mod stop_condition;
pub mod output;
pub mod generate_text;
pub mod stream_text;
pub mod agent;

pub use types::*;
pub use stop_condition::{StopCondition, step_count_is, has_tool_call};
pub use output::{Output, TextOutput, ObjectOutput};
pub use generate_text::generate_text;
pub use stream_text::stream_text;
pub use agent::Agent;
```

- [ ] **Step 5: Add to lib.rs**

Add `pub mod generate;` to `src/srow-ai/src/lib.rs`.

- [ ] **Step 6: Create stub files for generate_text.rs, stream_text.rs, agent.rs**

Create empty placeholder functions so the module compiles:

```rust
// generate_text.rs
pub async fn generate_text(_settings: super::CallSettings, _prompt: super::Prompt)
    -> Result<super::GenerateTextResult, srow_core::error::ChatError> {
    todo!()
}

// stream_text.rs
pub fn stream_text(_settings: super::CallSettings, _prompt: super::Prompt)
    -> super::StreamTextResult {
    todo!()
}

// agent.rs
pub struct Agent;
```

- [ ] **Step 7: Verify compilation**

Run: `cargo check -p srow-ai`

- [ ] **Step 8: Write tests for StopCondition and Output**

In inline `#[cfg(test)]` modules:
- `step_count_is(3)` stops at 3 steps, not at 2
- `has_tool_call("search")` stops when last step has "search" tool call
- `TextOutput::parse_complete` returns text as-is
- `ObjectOutput::<MyStruct>::parse_complete` deserializes JSON
- `ObjectOutput::parse_partial` returns None on invalid JSON, Some on valid

- [ ] **Step 9: Run tests**

Run: `cargo test -p srow-ai`

- [ ] **Step 10: Commit**

```bash
git add src/srow-ai/
git commit -m "feat(ai): add StopCondition, Output, and generate module types"
```

---

## Task 2: generate_text — non-streaming top-level API

**Files:**
- Create: `src/srow-ai/src/generate/generate_text.rs` (replace stub)
- Test: `src/srow-ai/tests/generate_text_test.rs`

- [ ] **Step 1: Implement generate_text**

The function signature:

```rust
pub async fn generate_text(
    settings: CallSettings,
    prompt: Prompt,
) -> Result<GenerateTextResult<String>, ChatError>
```

Internal flow (mirrors AI SDK's generateText):

1. **Convert prompt** — `Prompt::Text(s)` → single user LLMMessage; `Prompt::Messages(msgs)` → convert via `ui_messages_to_llm_messages`
2. **Agent loop** (do-while):
   a. Build `LLMRequest` from history + system prompt + tool definitions
   b. Call `model.complete(request)` (non-streaming, with retry on failure)
   c. Parse response: extract text, reasoning, tool calls
   d. Build `StepResult`
   e. If `stop_reason == ToolUse` and tools exist:
      - Execute all tool calls in parallel (`futures::future::join_all`)
      - Append assistant message + tool result messages to history
      - Check `stop_when` — if met, break
      - Otherwise continue loop
   f. If `stop_reason != ToolUse`, break
3. **Build result** — aggregate all steps, compute total_usage, extract final text

Key: This does NOT use AgentEngine. It directly uses `LLMProvider::complete()` and `ToolRegistry`. This is a clean functional wrapper, independent of session/storage.

- [ ] **Step 2: Implement retry logic**

```rust
async fn with_retry<F, Fut, T, E>(max_retries: u32, f: F) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut last_err = None;
    for _ in 0..=max_retries {
        match f().await {
            Ok(v) => return Ok(v),
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err.unwrap())
}
```

- [ ] **Step 3: Implement tool execution helper**

```rust
async fn execute_tools(
    tools: &ToolRegistry,
    calls: &[ToolCall],
    workspace: &std::path::Path,
    session_id: &str,
) -> Vec<ToolResult> {
    // Execute all tool calls in parallel using join_all
    // Return results (never error — errors become ToolResult with is_error=true)
}
```

- [ ] **Step 4: Write MockLLMProvider for tests**

In test file, create a `MockLLMProvider` that returns predefined responses for each `complete()` call (use a counter to return different responses per call).

- [ ] **Step 5: Write tests**

```
test_generate_text_simple — single step, no tools, returns text
test_generate_text_with_tool_call — model calls tool, tool returns result, model generates final text (2 steps)
test_generate_text_stop_condition — stop_when: step_count_is(2), verify stops at 2 even with more tool calls
test_generate_text_total_usage — verify usage is accumulated across steps
test_generate_text_retry — model fails first call, succeeds second (max_retries=1)
```

- [ ] **Step 6: Run tests**

Run: `cargo test -p srow-ai --test generate_text_test`

- [ ] **Step 7: Commit**

```bash
git add src/srow-ai/
git commit -m "feat(ai): implement generate_text with agentic loop, tool execution, retry"
```

---

## Task 3: stream_text — streaming top-level API

**Files:**
- Create: `src/srow-ai/src/generate/stream_text.rs` (replace stub)
- Test: `src/srow-ai/tests/stream_text_test.rs`

- [ ] **Step 1: Implement stream_text**

```rust
pub fn stream_text(
    settings: CallSettings,
    prompt: Prompt,
) -> StreamTextResult
```

This returns **synchronously** (like AI SDK's streamText). The actual work happens in a background task.

1. Create channels: `chunk_tx/rx`, `text_tx/rx`, `steps_tx/rx`, etc.
2. Spawn background task on tokio:
   a. Same agent loop as `generate_text`, but use `model.complete_stream()` instead of `complete()`
   b. Forward all `StreamChunk` as `UIMessageChunk` through `chunk_tx` (use `llm_stream_to_ui_chunks`)
   c. Between steps: emit `StartStep`/`FinishStep` chunks
   d. On completion: send final values through oneshot channels
3. Return `StreamTextResult` immediately

- [ ] **Step 2: Write tests**

```
test_stream_text_emits_chunks — collect all chunks from chunk_rx, verify Start/TextStart/TextDelta/TextEnd/Finish sequence
test_stream_text_multi_step — model calls tool → tool result → model generates text (verify chunks include ToolInputStart/Available/OutputAvailable + FinishStep + TextDelta)
test_stream_text_final_values — await text/steps/total_usage oneshot receivers, verify they resolve correctly
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p srow-ai --test stream_text_test`

- [ ] **Step 4: Commit**

```bash
git add src/srow-ai/
git commit -m "feat(ai): implement stream_text with background streaming and chunk emission"
```

---

## Task 4: Agent wrapper

**Files:**
- Create: `src/srow-ai/src/generate/agent.rs` (replace stub)
- Test: `src/srow-ai/tests/agent_test.rs`

- [ ] **Step 1: Implement Agent struct**

```rust
// src/srow-ai/src/generate/agent.rs

use std::sync::Arc;
use srow_core::ports::llm_provider::LLMProvider;
use srow_core::ports::tool::ToolRegistry;
use super::types::*;
use super::stop_condition::{StopCondition, step_count_is};
use super::generate_text::generate_text;
use super::stream_text::stream_text;
use srow_core::error::ChatError;

/// Agent — a configured wrapper around generate_text/stream_text.
/// Sets default stop_when to step_count_is(20).
/// Equivalent to AI SDK's ToolLoopAgent.
pub struct Agent {
    pub id: Option<String>,
    pub instructions: Option<String>,
    pub model: Arc<dyn LLMProvider>,
    pub tools: Option<Arc<ToolRegistry>>,
    pub stop_when: Option<Box<dyn StopCondition>>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub workspace: std::path::PathBuf,
}

impl Agent {
    pub fn new(model: Arc<dyn LLMProvider>) -> Self {
        Self {
            id: None,
            instructions: None,
            model,
            tools: None,
            stop_when: None,
            max_output_tokens: Some(8192),
            temperature: None,
            workspace: std::path::PathBuf::from("."),
        }
    }

    // Builder methods
    pub fn with_id(mut self, id: impl Into<String>) -> Self { self.id = Some(id.into()); self }
    pub fn with_instructions(mut self, s: impl Into<String>) -> Self { self.instructions = Some(s.into()); self }
    pub fn with_tools(mut self, t: Arc<ToolRegistry>) -> Self { self.tools = Some(t); self }
    pub fn with_stop_when(mut self, s: Box<dyn StopCondition>) -> Self { self.stop_when = Some(s); self }
    pub fn with_max_output_tokens(mut self, n: u32) -> Self { self.max_output_tokens = Some(n); self }
    pub fn with_temperature(mut self, t: f32) -> Self { self.temperature = Some(t); self }
    pub fn with_workspace(mut self, w: std::path::PathBuf) -> Self { self.workspace = w; self }

    /// Non-streaming generation. Default stop_when: step_count_is(20).
    pub async fn generate(&self, prompt: Prompt) -> Result<GenerateTextResult, ChatError> {
        generate_text(self.to_call_settings(), prompt).await
    }

    /// Streaming generation. Default stop_when: step_count_is(20).
    pub fn stream(&self, prompt: Prompt) -> StreamTextResult {
        stream_text(self.to_call_settings(), prompt)
    }

    fn to_call_settings(&self) -> CallSettings {
        CallSettings {
            model: self.model.clone(),
            system: self.instructions.clone(),
            tools: self.tools.clone(),
            max_output_tokens: self.max_output_tokens,
            temperature: self.temperature,
            stop_when: self.stop_when.take_or_default(), // need to handle this
            max_retries: 2,
            workspace: self.workspace.clone(),
        }
    }
}
```

Note: `stop_when` is `Option<Box<dyn StopCondition>>` which isn't Clone. Handle this by either:
- Making StopCondition Clone (via `dyn_clone` crate or manual)
- Or storing as `Arc<dyn StopCondition>` in both Agent and CallSettings

Choose `Arc<dyn StopCondition>` — change the field type in both `CallSettings` and `Agent` to `Option<Arc<dyn StopCondition>>`. Update `stop_condition.rs` convenience functions to return `Arc<dyn StopCondition>`.

- [ ] **Step 2: Fix StopCondition to use Arc**

In `stop_condition.rs`, change convenience functions:
```rust
pub fn step_count_is(n: u32) -> Arc<dyn StopCondition> {
    Arc::new(StepCountIs(n))
}
```

Update `CallSettings.stop_when` to `Option<Arc<dyn StopCondition>>`.

- [ ] **Step 3: Implement Agent.to_call_settings()**

The Agent defaults `stop_when` to `step_count_is(20)` if not set:
```rust
fn to_call_settings(&self) -> CallSettings {
    CallSettings {
        stop_when: Some(self.stop_when.clone().unwrap_or_else(|| step_count_is(20))),
        // ...
    }
}
```

- [ ] **Step 4: Write tests**

```
test_agent_generate_simple — Agent with mock model, no tools, returns text
test_agent_default_stop_at_20 — verify agent stops after 20 steps by default
test_agent_custom_stop_condition — Agent with step_count_is(3), verify stops at 3
test_agent_stream — Agent.stream() returns StreamTextResult with chunks
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p srow-ai --test agent_test`

- [ ] **Step 6: Commit**

```bash
git add src/srow-ai/
git commit -m "feat(ai): implement Agent wrapper with default stop_when and builder pattern"
```

---

## Task 5: Re-exports + integration verification

**Files:**
- Modify: `src/srow-ai/src/lib.rs`
- Modify: `src/srow-ai/src/generate/mod.rs`

- [ ] **Step 1: Clean up re-exports**

Ensure `src/srow-ai/src/lib.rs` re-exports key types:
```rust
pub use generate::{
    generate_text, stream_text, Agent,
    CallSettings, Prompt, GenerateTextResult, StreamTextResult, StepResult,
    Output, TextOutput, ObjectOutput,
    StopCondition, step_count_is, has_tool_call,
};
```

- [ ] **Step 2: Full workspace check**

Run: `cargo check --workspace`

- [ ] **Step 3: Full test suite**

Run: `cargo test --workspace`

- [ ] **Step 4: Commit**

```bash
git add src/srow-ai/
git commit -m "feat(ai): finalize generate module re-exports and integration"
```
