# Drop rig-core, Self-built Providers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace rig-core dependency with direct HTTP providers for OpenAI and Anthropic, aligned with AI SDK's provider design.

**Architecture:** Three files replace one: shared HTTP layer (`http.rs`) + OpenAI provider (`openai.rs`) + Anthropic provider (`anthropic.rs`). Each provider converts LLMRequest → provider-specific JSON, sends HTTP POST, parses response/SSE back into LLMResponse/StreamChunk. No intermediate abstraction layers.

**Tech Stack:** reqwest (already have), serde/serde_json (already have), existing SSE concepts

**Spec:** `docs/superpowers/specs/2026-03-21-drop-rig-providers-design.md`

---

## File Map

### New files
| File | Responsibility |
|------|---------------|
| `src/alva-app-core/src/adapters/llm/http.rs` | Raw SSE parser, retry logic, ProviderError |
| `src/alva-app-core/src/adapters/llm/openai.rs` | OpenAI Chat Completions provider |
| `src/alva-app-core/src/adapters/llm/anthropic.rs` | Anthropic Messages provider |
| `src/alva-app-core/tests/openai_provider_test.rs` | OpenAI format conversion + SSE parse tests |
| `src/alva-app-core/tests/anthropic_provider_test.rs` | Anthropic format conversion + SSE parse tests |

### Delete
| File | Reason |
|------|--------|
| `src/alva-app-core/src/adapters/llm/openai_compat.rs` | Replaced by openai.rs |

### Modify
| File | Change |
|------|--------|
| `src/alva-app-core/src/adapters/llm/mod.rs` | Replace openai_compat with openai + anthropic + http |
| `src/alva-app-core/Cargo.toml` | Remove rig-core |
| `src/alva-app-core/src/bin/cli.rs` | Use new OpenAIProvider |
| `src/alva-app/src/views/chat_panel/input_box.rs` | Use new provider constructors |

---

## Task 1: HTTP shared layer + raw SSE parser

**Files:**
- Create: `src/alva-app-core/src/adapters/llm/http.rs`
- Modify: `src/alva-app-core/src/adapters/llm/mod.rs`

- [ ] **Step 1: Implement SseEvent + parse_raw_sse**

Raw SSE parser that handles both OpenAI style (`data: {json}`) and Anthropic style (`event: type\ndata: {json}`).

```rust
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}
```

Parse logic: buffer bytes, split on `\n\n`, for each block parse `event:` and `data:` lines. Skip `:` comment lines. End on `data: [DONE]`.

- [ ] **Step 2: Implement ProviderError**

- [ ] **Step 3: Implement post_json_with_retry**

POST JSON body, handle rate limiting (429), retry with backoff.

- [ ] **Step 4: Register module in mod.rs**

Add `pub mod http;` to adapters/llm/mod.rs.

- [ ] **Step 5: Write SSE parser tests**

Test OpenAI-style SSE (no event field), Anthropic-style SSE (with event field), split across chunk boundaries, `[DONE]` termination.

- [ ] **Step 6: Commit**

```
git commit -m "feat(core): add shared HTTP layer with raw SSE parser and retry"
```

---

## Task 2: OpenAI Provider

**Files:**
- Create: `src/alva-app-core/src/adapters/llm/openai.rs`
- Test: `src/alva-app-core/tests/openai_provider_test.rs`

- [ ] **Step 1: Implement request conversion (LLMRequest → OpenAI JSON)**

Functions:
- `build_request_body(request: &LLMRequest, model: &str) -> serde_json::Value`
- `convert_messages(system: &Option<String>, messages: &[LLMMessage]) -> Vec<serde_json::Value>`
- `convert_content(content: &LLMContent) -> serde_json::Value`
- `convert_tool_defs(tools: &[ToolDefinition]) -> Vec<serde_json::Value>`
- `convert_tool_choice(tc: &ToolChoice) -> serde_json::Value`
- `convert_response_format(rf: &ResponseFormat) -> serde_json::Value`

- [ ] **Step 2: Implement response parsing (OpenAI JSON → LLMResponse)**

Functions:
- `parse_response(body: &serde_json::Value) -> Result<LLMResponse, ProviderError>`
- `parse_finish_reason(reason: &str) -> StopReason`

- [ ] **Step 3: Implement streaming SSE parsing**

Process OpenAI SSE events into StreamChunk:
- `data.choices[0].delta.content` → TextDelta
- `data.choices[0].delta.tool_calls` → track tool calls, emit ToolCallDelta
- `data.choices[0].finish_reason` → build Done
- `data.usage` → extract TokenUsage (from stream_options: include_usage)

- [ ] **Step 4: Implement LLMProvider trait**

```rust
impl LLMProvider for OpenAIProvider {
    fn model_id(&self) -> &str { &self.model }
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse, EngineError> { ... }
    async fn complete_stream(&self, request: LLMRequest, tx: mpsc::Sender<StreamChunk>) -> Result<(), EngineError> { ... }
}
```

- [ ] **Step 5: Write conversion tests**

- LLMRequest with text → correct OpenAI JSON structure
- LLMRequest with Image (base64) → correct image_url format
- LLMRequest with tools + tool_choice → correct tools array
- LLMRequest with ResponseFormat::Json → correct response_format
- OpenAI response JSON → correct LLMResponse (text + tool calls + usage)
- OpenAI SSE stream → correct StreamChunk sequence (text deltas, tool call deltas, done with usage)

- [ ] **Step 6: Commit**

```
git commit -m "feat(core): implement OpenAI provider with direct HTTP"
```

---

## Task 3: Anthropic Provider

**Files:**
- Create: `src/alva-app-core/src/adapters/llm/anthropic.rs`
- Test: `src/alva-app-core/tests/anthropic_provider_test.rs`

- [ ] **Step 1: Implement message merging logic**

Anthropic requires strict user/assistant alternation. Implement `merge_messages()`:
- Consecutive user messages → merge content blocks into one user message
- Tool result messages (Role::Tool) → merge into preceding user message as tool_result blocks
- Consecutive assistant messages → merge content blocks

- [ ] **Step 2: Implement request conversion (LLMRequest → Anthropic JSON)**

Key differences from OpenAI:
- `system` is top-level content block array (not in messages)
- Tool schema field is `input_schema` (not `parameters`)
- Image format: `{"type": "image", "source": {"type": "base64", "media_type": "...", "data": "..."}}`
- Tool choice: `auto`/`any`/`tool` (not `required`/`none`)
- No frequency_penalty/presence_penalty/seed
- Add `anthropic-version: 2023-06-01` header
- Add `stream: true` for streaming

- [ ] **Step 3: Implement response parsing (Anthropic JSON → LLMResponse)**

Parse content blocks:
- `text` → LLMContent::Text
- `tool_use` → LLMContent::ToolUse
- `thinking` → LLMContent::Reasoning

Map stop_reason: `end_turn` → EndTurn, `tool_use` → ToolUse, `max_tokens` → MaxTokens

Extract usage: input_tokens, output_tokens, cache_creation_input_tokens, cache_read_input_tokens

- [ ] **Step 4: Implement streaming SSE parsing**

Anthropic uses named events:
- `message_start` → extract input_tokens from usage
- `content_block_start` → track new block (text/thinking/tool_use)
- `content_block_delta` + `text_delta` → StreamChunk::TextDelta
- `content_block_delta` + `thinking_delta` → StreamChunk::ThinkingDelta
- `content_block_delta` + `input_json_delta` → StreamChunk::ToolCallDelta
- `content_block_stop` → finalize block
- `message_delta` → extract stop_reason + output_tokens
- `message_stop` → StreamChunk::Done with full LLMResponse

- [ ] **Step 5: Implement thinking support**

If `provider_options` contains `{"thinking": {"type": "enabled", "budget_tokens": N}}`:
- Add to request body
- Adjust max_tokens = max_tokens + budget_tokens
- Parse thinking blocks in response

- [ ] **Step 6: Implement LLMProvider trait**

Same interface as OpenAI, different internals.

- [ ] **Step 7: Write tests**

- Message merging: user + tool_result → single user message with content blocks
- LLMRequest → Anthropic JSON structure (system as array, input_schema, image format)
- Anthropic response JSON → LLMResponse (text + tool_use + thinking)
- Anthropic SSE stream → StreamChunk sequence (with named events)
- Thinking enabled → correct request body + reasoning in response

- [ ] **Step 8: Commit**

```
git commit -m "feat(core): implement Anthropic provider with direct HTTP"
```

---

## Task 4: Remove rig-core + wire up

**Files:**
- Delete: `src/alva-app-core/src/adapters/llm/openai_compat.rs`
- Modify: `src/alva-app-core/src/adapters/llm/mod.rs`
- Modify: `src/alva-app-core/Cargo.toml`
- Modify: `src/alva-app-core/src/lib.rs`
- Modify: `src/alva-app-core/src/bin/cli.rs`
- Modify: `src/alva-app/src/views/chat_panel/input_box.rs`

- [ ] **Step 1: Update adapters/llm/mod.rs**

```rust
pub mod http;
pub mod openai;
pub mod anthropic;
// DELETE: pub mod openai_compat;
```

- [ ] **Step 2: Delete openai_compat.rs**

- [ ] **Step 3: Remove rig-core from Cargo.toml**

Remove `rig-core = { version = "0.33", features = ["all"] }` from alva-app-core's Cargo.toml.

- [ ] **Step 4: Update lib.rs re-exports**

Replace `OpenAICompatProvider` re-export with `OpenAIProvider` + `AnthropicProvider`.

- [ ] **Step 5: Update cli.rs**

Replace `OpenAICompatProvider::new(api_key, model)` / `with_base_url(...)` with `OpenAIProvider::new(api_key, model)` / `OpenAIProvider::with_base_url(...)`.

- [ ] **Step 6: Update input_box.rs**

Replace provider construction in the GPUI view.

- [ ] **Step 7: Fix ALL compilation errors**

Search for any remaining references to `openai_compat`, `OpenAICompatProvider`, or `rig::`.

- [ ] **Step 8: Verify**

```bash
cargo check --workspace
cargo test --workspace
```

- [ ] **Step 9: Commit**

```
git commit -m "refactor(core): remove rig-core dependency, use direct HTTP providers"
```
