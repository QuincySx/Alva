# AI SDK React Port Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port Vercel AI SDK (`ai` core + `@ai-sdk/react`) architecture to Rust, creating a three-crate structure (alva-app-core data/protocol, srow-ai business logic, alva-app GPUI binding).

**Architecture:** Bottom-up build: data models first (alva-app-core), then framework-agnostic business logic (srow-ai), then GPUI integration (alva-app). Each layer is independently testable. The existing AgentEngine is migrated last to minimize disruption.

**Tech Stack:** Rust, tokio, serde, reqwest, futures, GPUI (gpui-ce), tokio-stream

**Spec:** `docs/superpowers/specs/2026-03-21-ai-sdk-port-design.md`

---

## File Map

### New files — alva-app-core
| File | Responsibility |
|------|---------------|
| `src/alva-app-core/src/ui_message/mod.rs` | UIMessage struct, UIMessageRole enum, re-exports |
| `src/alva-app-core/src/ui_message/parts.rs` | UIMessagePart enum, TextPartState, ToolState |
| `src/alva-app-core/src/ui_message/convert.rs` | UIMessage ↔ LLMMessage conversion |
| `src/alva-app-core/src/ui_message_stream/mod.rs` | UIMessageChunk enum, FinishReason, ChatStatus, TokenUsage, re-exports |
| `src/alva-app-core/src/ui_message_stream/state.rs` | StreamingUIMessageState, PartialToolCall |
| `src/alva-app-core/src/ui_message_stream/processor.rs` | process_ui_message_stream function |
| `src/alva-app-core/src/ui_message_stream/sse.rs` | SSE parse/serialize utilities |
| `src/alva-app-core/src/ui_message_stream/writer.rs` | UIMessageStreamWriter |
| `src/alva-app-core/tests/ui_message_test.rs` | UIMessage serde + convert tests |
| `src/alva-app-core/tests/ui_message_stream_test.rs` | Stream processor + SSE tests |

### New files — srow-ai
| File | Responsibility |
|------|---------------|
| `src/srow-ai/Cargo.toml` | Crate manifest |
| `src/srow-ai/src/lib.rs` | Crate root, re-exports |
| `src/srow-ai/src/util/mod.rs` | Utility module |
| `src/srow-ai/src/util/abort.rs` | AbortController + AbortHandle |
| `src/srow-ai/src/util/serial_job_executor.rs` | SerialJobExecutor |
| `src/srow-ai/src/util/throttle.rs` | Throttle utility |
| `src/srow-ai/src/chat/mod.rs` | Chat module |
| `src/srow-ai/src/chat/chat_state.rs` | ChatState trait |
| `src/srow-ai/src/chat/chat_options.rs` | ChatInit, SendOptions, callbacks |
| `src/srow-ai/src/chat/abstract_chat.rs` | AbstractChat core logic |
| `src/srow-ai/src/transport/mod.rs` | Transport module |
| `src/srow-ai/src/transport/traits.rs` | ChatTransport trait, ChatRequest |
| `src/srow-ai/src/transport/direct.rs` | DirectChatTransport |
| `src/srow-ai/src/transport/http_sse.rs` | HttpSseChatTransport |
| `src/srow-ai/src/transport/text_stream.rs` | TextStreamChatTransport |
| `src/srow-ai/src/completion/mod.rs` | Completion module |
| `src/srow-ai/src/completion/completion.rs` | Completion logic |
| `src/srow-ai/src/object/mod.rs` | Object module |
| `src/srow-ai/src/object/object_generation.rs` | ObjectGeneration logic |
| `src/srow-ai/tests/abstract_chat_test.rs` | AbstractChat integration tests |
| `src/srow-ai/tests/transport_test.rs` | Transport tests |

### New files — alva-app
| File | Responsibility |
|------|---------------|
| `src/alva-app/src/chat/mod.rs` | Chat module |
| `src/alva-app/src/chat/gpui_chat_state.rs` | GpuiChatState (ChatState impl) |
| `src/alva-app/src/chat/gpui_chat.rs` | GpuiChat GPUI Entity |
| `src/alva-app/src/views/chat_panel/tool_approval.rs` | Inline tool approval UI |

### Modified files
| File | Change |
|------|--------|
| `Cargo.toml` | Add srow-ai to workspace members |
| `src/alva-app-core/src/lib.rs` | Add ui_message, ui_message_stream modules; add re-exports |
| `src/alva-app-core/src/error.rs` | Add ChatError enum |
| `src/alva-app-core/src/agent/runtime/engine/engine.rs` | EngineEvent → UIMessageChunk |
| `src/alva-app-core/src/bin/cli.rs` | Update to use UIMessageChunk |
| `src/alva-app/Cargo.toml` | Add srow-ai dependency |
| `src/alva-app/src/lib.rs` | Add chat module, remove engine_bridge |
| `src/alva-app/src/types/mod.rs` | Remove message re-export |
| `src/alva-app/src/models/mod.rs` | Update ChatModel import |
| `src/alva-app/src/models/chat_model.rs` | Full rewrite |
| `src/alva-app/src/views/chat_panel/chat_panel.rs` | Use new model |
| `src/alva-app/src/views/chat_panel/message_list.rs` | Render UIMessage parts |
| `src/alva-app/src/views/chat_panel/input_box.rs` | Use GpuiChat |
| `src/alva-app/src/views/chat_panel/mod.rs` | Add tool_approval |

### Deleted files
| File | Reason |
|------|--------|
| `src/alva-app/src/types/message.rs` | Replaced by alva-app-core::ui_message |
| `src/alva-app/src/engine_bridge/bridge.rs` | Replaced by DirectChatTransport |
| `src/alva-app/src/engine_bridge/mod.rs` | Replaced by DirectChatTransport |

---

## Task 1: UIMessage data model (alva-app-core)

**Files:**
- Create: `src/alva-app-core/src/ui_message/mod.rs`
- Create: `src/alva-app-core/src/ui_message/parts.rs`
- Modify: `src/alva-app-core/src/lib.rs`
- Test: `src/alva-app-core/tests/ui_message_test.rs`

- [ ] **Step 1: Create parts.rs with all enums**

Create `src/alva-app-core/src/ui_message/parts.rs` with `UIMessagePart`, `TextPartState`, `ToolState` enums. All derive `Clone, Debug, Serialize, Deserialize`. Use `#[serde(tag = "type", rename_all = "kebab-case")]` on UIMessagePart. See spec section 2.2 for exact fields.

- [ ] **Step 2: Create mod.rs with UIMessage and UIMessageRole**

Create `src/alva-app-core/src/ui_message/mod.rs` with `UIMessage` struct (id, role, parts, metadata), `UIMessageRole` enum (System, User, Assistant). Re-export everything from `parts.rs`.

- [ ] **Step 3: Register module in lib.rs**

Add `pub mod ui_message;` to `src/alva-app-core/src/lib.rs`. Add convenience re-exports for `UIMessage`, `UIMessagePart`, `UIMessageRole`, `TextPartState`, `ToolState`.

- [ ] **Step 4: Write serde round-trip tests**

Create `src/alva-app-core/tests/ui_message_test.rs`. Test:
- Serialize/deserialize `UIMessage` with Text part
- Serialize/deserialize `UIMessage` with Tool part (all ToolState variants)
- Serialize/deserialize `UIMessage` with mixed parts (Text + Reasoning + Tool + File + StepStart)
- Verify `#[serde(tag = "type")]` produces correct JSON shape (e.g. `{"type": "text", "text": "hello", "state": "streaming"}`)

- [ ] **Step 5: Run tests**

Run: `cargo test -p alva-app-core --test ui_message_test`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/alva-app-core/src/ui_message/ src/alva-app-core/src/lib.rs src/alva-app-core/tests/ui_message_test.rs
git commit -m "feat(core): add UIMessage + UIMessagePart data model"
```

---

## Task 2: UIMessageChunk stream protocol (alva-app-core)

**Files:**
- Create: `src/alva-app-core/src/ui_message_stream/mod.rs`
- Create: `src/alva-app-core/src/ui_message_stream/state.rs`
- Modify: `src/alva-app-core/src/lib.rs`
- Modify: `src/alva-app-core/src/error.rs`
- Test: `src/alva-app-core/tests/ui_message_stream_test.rs`

- [ ] **Step 1: Add ChatError to error.rs**

Add `ChatError` enum to `src/alva-app-core/src/error.rs` (Clone + Debug + thiserror::Error). Variants: `Transport(String)`, `Stream(String)`, `Serialization(String)`, `Engine(String)`.

Also add `StreamError` enum: `InvalidSse(String)`, `InvalidChunk(String)`, `Interrupted`, `Aborted`.

- [ ] **Step 2: Create mod.rs with UIMessageChunk, FinishReason, ChatStatus, TokenUsage**

Create `src/alva-app-core/src/ui_message_stream/mod.rs`. All 28+ UIMessageChunk variants as specified in spec section 2.3. Include `FinishReason`, `ChatStatus`, `TokenUsage`. All derive `Clone, Debug, Serialize, Deserialize`.

- [ ] **Step 3: Create state.rs with StreamingUIMessageState**

Create `src/alva-app-core/src/ui_message_stream/state.rs` with `StreamingUIMessageState` and `PartialToolCall` structs as specified in spec section 2.4.

- [ ] **Step 4: Register module in lib.rs**

Add `pub mod ui_message_stream;` to `src/alva-app-core/src/lib.rs`. Add re-exports for `UIMessageChunk`, `FinishReason`, `ChatStatus`, `TokenUsage`, `ChatError`, `StreamError`.

- [ ] **Step 5: Write serde tests for UIMessageChunk**

Add to `src/alva-app-core/tests/ui_message_stream_test.rs`:
- Round-trip test for each major chunk category (Start, TextDelta, ToolInputStart, ToolApprovalRequest, Finish, Error, TokenUsage, Data)
- Verify JSON shape matches AI SDK SSE protocol (e.g. `{"type": "text-delta", "id": "t1", "delta": "hello"}`)

- [ ] **Step 6: Run tests**

Run: `cargo test -p alva-app-core --test ui_message_stream_test`
Expected: All pass.

- [ ] **Step 7: Commit**

```bash
git add src/alva-app-core/src/ui_message_stream/ src/alva-app-core/src/error.rs src/alva-app-core/src/lib.rs src/alva-app-core/tests/ui_message_stream_test.rs
git commit -m "feat(core): add UIMessageChunk stream protocol + ChatStatus + errors"
```

---

## Task 3: Stream processor (alva-app-core)

**Files:**
- Create: `src/alva-app-core/src/ui_message_stream/processor.rs`
- Modify: `src/alva-app-core/src/ui_message_stream/mod.rs`
- Test: `src/alva-app-core/tests/ui_message_stream_test.rs` (append)

- [ ] **Step 1: Define UIMessageStreamUpdate enum**

In `processor.rs`, define:
```rust
pub enum UIMessageStreamUpdate {
    MessageChanged(UIMessage),
    FirstWrite(UIMessage),
    Finished(StreamingUIMessageState),
}
```

- [ ] **Step 2: Implement process_ui_message_stream**

Implement the async function that consumes a `Stream<Item = Result<UIMessageChunk, StreamError>>` and sends `UIMessageStreamUpdate` through an `mpsc::UnboundedSender`. Process each chunk type per spec section 2.5:
- `Start` → set message_id on state
- `TextStart` → push new Text part with state=Streaming, record in active_text_parts
- `TextDelta` → append delta to active text part's text
- `TextEnd` → set text part state=Done, remove from active
- `ReasoningStart/Delta/End` → same pattern for Reasoning parts
- `ToolInputStart` → push Tool part with state=InputStreaming, record partial_tool_call
- `ToolInputDelta` → append to partial_tool_call.text
- `ToolInputAvailable` → parse JSON input, set Tool input + state=InputAvailable
- `ToolApprovalRequest` → set Tool state=ApprovalRequested
- `ToolOutputAvailable` → set Tool output + state=OutputAvailable
- `ToolOutputError` → set Tool error + state=OutputError
- `ToolOutputDenied` → set Tool state=OutputDenied
- `StartStep/FinishStep` → reset active_text_parts and active_reasoning_parts
- `Data` → push Data part
- `Finish` → set finish_reason on state
- `Error` → return StreamError
- First write sends `FirstWrite`, subsequent sends `MessageChanged`

- [ ] **Step 3: Re-export from mod.rs**

Add `pub mod processor;` and re-export `process_ui_message_stream`, `UIMessageStreamUpdate`.

- [ ] **Step 4: Write processor tests**

Add to `ui_message_stream_test.rs`:
- Test: text-only stream (Start → TextStart → TextDelta × 3 → TextEnd → Finish) produces correct UIMessage with single Text part
- Test: tool call stream (Start → TextStart → TextEnd → ToolInputStart → ToolInputDelta × 2 → ToolInputAvailable → ToolOutputAvailable → Finish) produces UIMessage with Text + Tool parts
- Test: multi-step stream (chunks with FinishStep between steps) resets active parts correctly
- Test: approval flow (ToolApprovalRequest chunk sets ToolState::ApprovalRequested)
- Test: FirstWrite is sent on first chunk, MessageChanged on subsequent

- [ ] **Step 5: Run tests**

Run: `cargo test -p alva-app-core --test ui_message_stream_test`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/alva-app-core/src/ui_message_stream/processor.rs src/alva-app-core/src/ui_message_stream/mod.rs src/alva-app-core/tests/ui_message_stream_test.rs
git commit -m "feat(core): implement process_ui_message_stream chunk processor"
```

---

## Task 4: SSE parser + UIMessage ↔ LLMMessage conversion (alva-app-core)

**Files:**
- Create: `src/alva-app-core/src/ui_message_stream/sse.rs`
- Create: `src/alva-app-core/src/ui_message/convert.rs`
- Modify: `src/alva-app-core/src/ui_message/mod.rs`
- Modify: `src/alva-app-core/src/ui_message_stream/mod.rs`
- Test: append to existing test files

- [ ] **Step 1: Implement SSE parser**

In `sse.rs`, implement `parse_sse_stream` that takes `impl Stream<Item = Result<Bytes, ...>> + Send` and returns `impl Stream<Item = Result<UIMessageChunk, StreamError>> + Send`. Parse `data: {json}\n\n` lines, skip `data: [DONE]\n\n`, deserialize JSON via serde.

Also implement `chunk_to_sse(chunk: &UIMessageChunk) -> String`.

- [ ] **Step 2: Implement UIMessage ↔ LLMMessage conversion**

In `convert.rs`:
- `ui_messages_to_llm_messages(&[UIMessage]) -> Vec<LLMMessage>`: Convert User messages directly. For Assistant messages, split Tool parts into ToolUse content blocks in the Assistant message, then generate separate `Role::Tool` messages for each Tool part that has output.
- `llm_stream_to_ui_chunks(impl Stream<Item = StreamChunk> + Send) -> impl Stream<Item = UIMessageChunk> + Send`: Track whether TextStart/ReasoningStart have been emitted, auto-emit start/end chunks around delta sequences. On `StreamChunk::Done`, emit remaining end chunks + ToolInputAvailable (from accumulated tool calls) + Finish.

- [ ] **Step 3: Re-export from mod.rs files**

- [ ] **Step 4: Write tests**

SSE tests: parse a multi-line SSE string into UIMessageChunk vec, verify round-trip (chunk_to_sse → parse_sse_stream).
Convert tests: UIMessage with Text+Tool parts → LLMMessage vec → verify Role::Assistant + Role::Tool structure. LLM StreamChunk sequence → UIMessageChunk sequence verification.

- [ ] **Step 5: Run tests**

Run: `cargo test -p alva-app-core -- ui_message`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/alva-app-core/src/ui_message_stream/sse.rs src/alva-app-core/src/ui_message/convert.rs
git commit -m "feat(core): add SSE parser and UIMessage ↔ LLMMessage conversion"
```

---

## Task 5: srow-ai crate scaffold + utilities

**Files:**
- Create: `src/srow-ai/Cargo.toml`
- Create: `src/srow-ai/src/lib.rs`
- Create: `src/srow-ai/src/util/mod.rs`
- Create: `src/srow-ai/src/util/abort.rs`
- Create: `src/srow-ai/src/util/serial_job_executor.rs`
- Create: `src/srow-ai/src/util/throttle.rs`
- Modify: `Cargo.toml` (workspace)

- [ ] **Step 1: Create Cargo.toml**

Create `src/srow-ai/Cargo.toml` with dependencies as specified in spec section 9. Package name: `srow-ai`, depend on `alva-app-core = { path = "../alva-app-core" }`.

- [ ] **Step 2: Add to workspace**

Add `"src/srow-ai"` to workspace members in root `Cargo.toml`.

- [ ] **Step 3: Create lib.rs with module declarations**

```rust
pub mod util;
pub mod chat;
pub mod transport;
pub mod completion;
pub mod object;
```

Create empty mod.rs files for chat, transport, completion, object.

- [ ] **Step 4: Implement AbortController + AbortHandle**

In `util/abort.rs`: `AbortController` wraps `watch::Sender<bool>`, `AbortHandle` wraps `watch::Receiver<bool>`. Methods: `AbortController::new() -> (Self, AbortHandle)`, `abort(&self)`, `AbortHandle::is_aborted()`, `AbortHandle::cancelled() async`.

- [ ] **Step 5: Implement SerialJobExecutor**

In `util/serial_job_executor.rs`: Takes `&tokio::runtime::Handle` in `new()`, spawns consumer loop via `handle.spawn()`. Method `run(job: impl Future) async` sends job to queue and awaits completion via oneshot.

- [ ] **Step 6: Implement throttle**

In `util/throttle.rs`: Simple `Throttle` struct that wraps `tokio::time::Instant` and a duration, provides `fn should_emit(&mut self) -> bool`.

- [ ] **Step 7: Verify crate compiles**

Run: `cargo check -p srow-ai`
Expected: Compiles with no errors.

- [ ] **Step 8: Write utility tests**

Add tests inline or in a test file:
- AbortController: abort signal is received by handle, `is_aborted()` returns true after abort
- SerialJobExecutor: two concurrent jobs execute sequentially (second starts after first finishes)
- Throttle: rapid calls are throttled, call after duration passes through

- [ ] **Step 9: Run tests**

Run: `cargo test -p srow-ai`
Expected: All pass.

- [ ] **Step 10: Commit**

```bash
git add src/srow-ai/ Cargo.toml
git commit -m "feat(ai): scaffold srow-ai crate with AbortHandle, SerialJobExecutor, Throttle"
```

---

## Task 6: ChatState trait + ChatTransport trait (srow-ai)

**Files:**
- Create: `src/srow-ai/src/chat/mod.rs`
- Create: `src/srow-ai/src/chat/chat_state.rs`
- Create: `src/srow-ai/src/chat/chat_options.rs`
- Create: `src/srow-ai/src/transport/mod.rs`
- Create: `src/srow-ai/src/transport/traits.rs`

- [ ] **Step 1: Define ChatState trait**

In `chat/chat_state.rs`: Define trait as in spec section 3.3. Methods: `messages() -> Vec<UIMessage>`, `set_messages`, `push_message`, `pop_message`, `replace_message`, `status() -> ChatStatus`, `set_status`, `error() -> Option<ChatError>`, `set_error`, `notify_messages_changed(&mut self)`, `notify_status_changed(&mut self)`, `notify_error_changed(&mut self)`. **No** Send + Sync bound.

- [ ] **Step 2: Define ChatInit and option types**

In `chat/chat_options.rs`: Define `ChatInit<S>`, `SendOptions`, `RegenerateOptions`, `ToolCallInfo`, `ToolCallResult`, `FinishInfo`, `AsyncToolCallHandler` type alias. See spec section 3.4.

- [ ] **Step 3: Define ChatTransport trait**

In `transport/traits.rs`: Define `ChatTransport` trait (Send + Sync, async_trait), `ChatRequest` struct. See spec section 3.5. Also define `TransportError` enum.

- [ ] **Step 4: Wire up mod.rs files**

`chat/mod.rs` re-exports from chat_state, chat_options. `transport/mod.rs` re-exports from traits.

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p srow-ai`
Expected: Compiles.

- [ ] **Step 6: Commit**

```bash
git add src/srow-ai/src/chat/ src/srow-ai/src/transport/
git commit -m "feat(ai): define ChatState trait, ChatTransport trait, and option types"
```

---

## Task 7: AbstractChat core logic (srow-ai)

**Files:**
- Create: `src/srow-ai/src/chat/abstract_chat.rs`
- Modify: `src/srow-ai/src/chat/mod.rs`
- Test: `src/srow-ai/tests/abstract_chat_test.rs`

- [ ] **Step 1: Define AbstractChat struct + ChatInner**

In `abstract_chat.rs`: `AbstractChat<S: ChatState>` with `inner: Arc<Mutex<ChatInner<S>>>`, `transport: Arc<dyn ChatTransport>`, `job_executor: SerialJobExecutor`, callbacks. `ChatInner<S>` holds `id`, `state: S`, `active_abort: Option<AbortController>`. See spec section 3.9.

- [ ] **Step 2: Implement AbstractChat::new()**

Constructor takes `ChatInit<S>`, initializes `SerialJobExecutor` with `runtime_handle`, sets initial messages in state.

- [ ] **Step 3: Implement send_message**

Builds user `UIMessage`, pushes to state, calls `make_request`. All through `job_executor.run()`.

- [ ] **Step 4: Implement make_request (core loop)**

1. Create AbortController, store in inner
2. Set status = Submitted, notify
3. Call transport.send_messages()
4. Call consume_stream()
5. Set status = Ready, notify
6. Call on_finish callback

- [ ] **Step 5: Implement consume_stream**

1. Create update channel
2. Spawn `process_ui_message_stream` on runtime
3. Consume `UIMessageStreamUpdate`:
   - `FirstWrite` → set status=Streaming, push assistant message, notify
   - `MessageChanged` → replace last message, notify
   - `Finished` → handle tool callbacks, check send_automatically_when

- [ ] **Step 6: Implement stop, regenerate, resume_stream, add_tool_output, add_tool_approval_response, clear_error**

Each method locks inner, mutates state, notifies. See spec section 3.9 for logic.

- [ ] **Step 7: Add lock_state helper**

```rust
pub fn lock_state<R>(&self, f: impl FnOnce(&S) -> R) -> R {
    let inner = self.inner.lock().unwrap();
    f(&inner.state)
}
```

- [ ] **Step 8: Re-export from chat/mod.rs**

- [ ] **Step 9: Write MockTransport for testing**

In test file, create `MockTransport` that implements `ChatTransport`, returns a predefined sequence of chunks.

- [ ] **Step 10: Write AbstractChat tests**

- Test: send_message → status goes Submitted → Streaming → Ready
- Test: send_message → messages contains user message + assistant message with correct parts
- Test: stop → aborts active stream, status goes to Ready
- Test: tool call flow → ToolApprovalRequested state set correctly
- Test: add_tool_output → Tool part updated with output

- [ ] **Step 11: Run tests**

Run: `cargo test -p srow-ai --test abstract_chat_test`
Expected: All pass.

- [ ] **Step 12: Commit**

```bash
git add src/srow-ai/src/chat/abstract_chat.rs src/srow-ai/src/chat/mod.rs src/srow-ai/tests/
git commit -m "feat(ai): implement AbstractChat with send, stop, tool approval, auto-send"
```

---

## Task 8: DirectChatTransport (srow-ai)

**Files:**
- Create: `src/srow-ai/src/transport/direct.rs`
- Modify: `src/srow-ai/src/transport/mod.rs`

- [ ] **Step 1: Implement DirectChatTransport struct**

Fields: `llm: Arc<dyn LLMProvider>`, `tools: Arc<ToolRegistry>`, `storage: Arc<dyn SessionStorage>`, `config: Arc<AgentConfig>`, `approval_tx/rx` channels. Constructor: `new(llm, tools, storage, config) -> Self`.

- [ ] **Step 2: Implement ChatTransport::send_messages**

1. Convert `UIMessage → LLMMessage` using `ui_messages_to_llm_messages`
2. Create `mpsc::unbounded_channel::<UIMessageChunk>()`
3. Spawn tokio task that runs AgentEngine loop:
   - Engine emits `UIMessageChunk` directly to chunk_tx
   - For tool approval: check SecurityGuard, emit ToolApprovalRequest, wait on approval_rx
4. Return `UnboundedReceiverStream::new(chunk_rx)` as the Stream

- [ ] **Step 3: Implement reconnect (stub)**

Return `Ok(None)` — DirectTransport doesn't support reconnection.

- [ ] **Step 4: Add approval response method**

```rust
pub fn send_approval(&self, response: ApprovalResponse) {
    let _ = self.approval_tx.send(response);
}
```

- [ ] **Step 5: Re-export from transport/mod.rs**

- [ ] **Step 6: Verify compilation**

Run: `cargo check -p srow-ai`
Expected: Compiles.

- [ ] **Step 7: Commit**

```bash
git add src/srow-ai/src/transport/direct.rs src/srow-ai/src/transport/mod.rs
git commit -m "feat(ai): implement DirectChatTransport for in-process engine calls"
```

---

## Task 9: HttpSseChatTransport + TextStreamChatTransport (srow-ai)

**Files:**
- Create: `src/srow-ai/src/transport/http_sse.rs`
- Create: `src/srow-ai/src/transport/text_stream.rs`
- Modify: `src/srow-ai/src/transport/mod.rs`

- [ ] **Step 1: Implement HttpSseChatTransport**

Fields: `api_url`, `headers`, `client: reqwest::Client`. `send_messages`: POST JSON body `{ messages }`, get response body stream, pipe through `parse_sse_stream`, return as UIMessageChunk stream. `reconnect`: POST to `{api_url}/reconnect?chatId={id}`, same SSE parsing.

- [ ] **Step 2: Implement TextStreamChatTransport**

Fields: `api_url`, `client`. `send_messages`: POST, get text stream, wrap each text chunk as `Start → TextStart → TextDelta* → TextEnd → Finish` chunk sequence.

- [ ] **Step 3: Re-export from mod.rs**

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p srow-ai`

- [ ] **Step 5: Commit**

```bash
git add src/srow-ai/src/transport/
git commit -m "feat(ai): implement HttpSse and TextStream chat transports"
```

---

## Task 10: Completion + ObjectGeneration (srow-ai)

**Files:**
- Create: `src/srow-ai/src/completion/completion.rs`
- Create: `src/srow-ai/src/completion/mod.rs`
- Create: `src/srow-ai/src/object/object_generation.rs`
- Create: `src/srow-ai/src/object/mod.rs`

- [ ] **Step 1: Implement Completion**

As spec section 3.11. `complete(&mut self, prompt) -> Result<String>`: POST to api_url, stream SSE, extract text-delta deltas, accumulate. `stop()`, `completion()`, `is_loading()`.

- [ ] **Step 2: Implement ObjectGeneration**

As spec section 3.12. `submit(&mut self, input)`: POST, stream JSON text, parse partial JSON with `serde_json::from_str` (attempt parse on each chunk, update on success). `object()`, `typed_object()`, `stop()`, `clear()`.

- [ ] **Step 3: Wire up mod.rs files**

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p srow-ai`

- [ ] **Step 5: Commit**

```bash
git add src/srow-ai/src/completion/ src/srow-ai/src/object/
git commit -m "feat(ai): implement Completion and ObjectGeneration"
```

---

## Task 11: Migrate AgentEngine from EngineEvent to UIMessageChunk (alva-app-core)

**Files:**
- Modify: `src/alva-app-core/src/agent/runtime/engine/engine.rs`
- Modify: `src/alva-app-core/src/bin/cli.rs`

- [ ] **Step 1: Change engine event_tx type**

In `engine.rs`: Change `event_tx: mpsc::Sender<EngineEvent>` to `event_tx: mpsc::Sender<UIMessageChunk>`. Update `AgentEngine::new()` signature.

- [ ] **Step 2: Rewrite the streaming event emission in run()**

Replace all `self.event_tx.send(EngineEvent::TextDelta { .. })` calls:
- At loop start: emit `Start { message_id }`
- Track text/reasoning start state with booleans, emit `TextStart`/`ReasoningStart` on first delta
- `TextDelta` → `UIMessageChunk::TextDelta`
- `ThinkingDelta` → `UIMessageChunk::ReasoningDelta`
- On `StreamChunk::Done`: emit `TextEnd`/`ReasoningEnd` for active parts, then `ToolInputStart`/`ToolInputAvailable` for each tool call
- `TokenUsage` → `UIMessageChunk::TokenUsage`
- Completion → `Finish { finish_reason: Stop }`
- Error → `UIMessageChunk::Error`

- [ ] **Step 3: Rewrite execute_tools to emit tool chunks**

For each tool call:
- Emit `ToolInputStart { id, tool_name }`
- Emit `ToolInputAvailable { id, input }`
- After execution: emit `ToolOutputAvailable { id, output }` or `ToolOutputError { id, error }`

- [ ] **Step 4: Delete EngineEvent enum**

Remove `EngineEvent` from engine.rs. Remove the old re-export from lib.rs. Update lib.rs re-exports to include `UIMessageChunk` instead.

- [ ] **Step 5: Update cli.rs**

Update `src/alva-app-core/src/bin/cli.rs` to match on `UIMessageChunk` variants instead of `EngineEvent`. Map:
- `TextDelta` → print text
- `ReasoningDelta` → print dim text
- `ToolInputStart` → print `[tool] calling: {name}`
- `ToolOutputAvailable/ToolOutputError` → print result
- `Finish` → println
- `Error` → eprintln

- [ ] **Step 6: Verify both cli and lib compile**

Run: `cargo check -p alva-app-core`
Expected: Compiles.

- [ ] **Step 7: Run existing tests**

Run: `cargo test -p alva-app-core`
Expected: Existing tests still pass (some may need minor updates if they reference EngineEvent).

- [ ] **Step 8: Commit**

```bash
git add src/alva-app-core/src/agent/runtime/engine/engine.rs src/alva-app-core/src/bin/cli.rs src/alva-app-core/src/lib.rs
git commit -m "feat(core): migrate AgentEngine from EngineEvent to UIMessageChunk"
```

---

## Task 12: GPUI Chat binding (alva-app)

**Files:**
- Create: `src/alva-app/src/chat/mod.rs`
- Create: `src/alva-app/src/chat/gpui_chat_state.rs`
- Create: `src/alva-app/src/chat/gpui_chat.rs`
- Modify: `src/alva-app/Cargo.toml`
- Modify: `src/alva-app/src/lib.rs`

- [ ] **Step 1: Add srow-ai dependency**

Add `srow-ai = { path = "../srow-ai" }` to `src/alva-app/Cargo.toml`.

- [ ] **Step 2: Implement GpuiChatState**

In `gpui_chat_state.rs`: Struct with `messages: Vec<UIMessage>`, `status: ChatStatus`, `error: Option<ChatError>`, `notify_tx: futures::channel::mpsc::UnboundedSender<NotifyKind>`. Implement `ChatState` trait. `notify_*` methods send to `notify_tx`. See spec section 4.3.

- [ ] **Step 3: Implement GpuiChat**

In `gpui_chat.rs`: Struct with `inner: AbstractChat<GpuiChatState>`, `runtime: tokio::runtime::Runtime`. Constructor creates runtime, notify channel, state, transport, AbstractChat. Spawns GPUI foreground task consuming notify_rx → `cx.notify()`. Delegate methods: `send_message`, `stop`, `add_tool_approval_response`, `messages`, `status`. See spec section 4.4.

- [ ] **Step 4: Create chat/mod.rs and register in lib.rs**

Add `pub mod chat;` to `src/alva-app/src/lib.rs`.

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p alva-app`
Expected: Compiles (existing views will still reference old ChatModel, that's ok for now).

- [ ] **Step 6: Commit**

```bash
git add src/alva-app/src/chat/ src/alva-app/Cargo.toml src/alva-app/src/lib.rs
git commit -m "feat(app): add GpuiChat + GpuiChatState GPUI binding layer"
```

---

## Task 13: Rewrite ChatModel + delete old files (alva-app)

**Files:**
- Modify: `src/alva-app/src/models/chat_model.rs`
- Modify: `src/alva-app/src/models/mod.rs`
- Modify: `src/alva-app/src/types/mod.rs`
- Delete: `src/alva-app/src/types/message.rs`
- Delete: `src/alva-app/src/engine_bridge/bridge.rs`
- Delete: `src/alva-app/src/engine_bridge/mod.rs`
- Modify: `src/alva-app/src/lib.rs`

- [ ] **Step 1: Rewrite ChatModel**

Replace `src/alva-app/src/models/chat_model.rs` entirely. New ChatModel: `chats: HashMap<String, Entity<GpuiChat>>`, `drafts: HashMap<String, String>`. Methods: `get_or_create_chat()`, `send_message()`. Event: `ChatModelEvent::ChatCreated`. See spec section 4.5.

- [ ] **Step 2: Delete old message types**

Delete `src/alva-app/src/types/message.rs`. Remove `pub mod message;` from `src/alva-app/src/types/mod.rs`. Remove `Message`, `MessageContent`, `MessageRole` references.

- [ ] **Step 3: Delete engine_bridge**

Delete `src/alva-app/src/engine_bridge/` directory. Remove `pub mod engine_bridge;` from `src/alva-app/src/lib.rs`.

- [ ] **Step 4: Update models/mod.rs**

Remove old ChatModel re-exports, add new ones. Remove `ChatModelEvent::MessageAppended`, `StreamDelta`, `StreamCompleted`. Replace with `ChatModelEvent::ChatCreated`.

- [ ] **Step 5: Fix all compilation errors**

Update all files that reference old `ChatModel`, `Message`, `MessageContent`, `EngineBridge` to use new types. This will cascade through views — fix type signatures but leave rendering logic for next task.

- [ ] **Step 6: Verify compilation**

Run: `cargo check -p alva-app`
Expected: Compiles.

- [ ] **Step 7: Commit**

```bash
git add -A src/alva-app/
git commit -m "refactor(app): rewrite ChatModel, delete old Message types and EngineBridge"
```

---

## Task 14: Rewrite chat panel views (alva-app)

**Files:**
- Modify: `src/alva-app/src/views/chat_panel/message_list.rs`
- Modify: `src/alva-app/src/views/chat_panel/input_box.rs`
- Modify: `src/alva-app/src/views/chat_panel/chat_panel.rs`
- Create: `src/alva-app/src/views/chat_panel/tool_approval.rs`
- Modify: `src/alva-app/src/views/chat_panel/mod.rs`

- [ ] **Step 1: Rewrite MessageList**

Read current file first. Rewrite render to iterate `chat.messages()`, then for each message iterate `message.parts`. Render each part type:
- `Text` → text div, dim if state=Streaming
- `Reasoning` → collapsible thinking block
- `Tool` → tool name + state badge + approval buttons if ApprovalRequested + output/error
- `File` → placeholder (show media_type + size)
- `StepStart` → horizontal divider
- Others → skip

- [ ] **Step 2: Create tool_approval.rs**

Inline approval component. Takes `tool_call_id`, `tool_name`, `input`, `Entity<GpuiChat>`. Renders "Allow" / "Deny" buttons. On click: `chat.update(cx, |c, _| c.add_tool_approval_response(id, approved))`.

- [ ] **Step 3: Rewrite InputBox**

Update to call `chat_model.send_message(session_id, text, cx)` which delegates to `GpuiChat::send_message`. Remove references to `EngineBridge::send_message`.

- [ ] **Step 4: Update ChatPanel**

Pass `Entity<GpuiChat>` to MessageList and InputBox instead of separate `Entity<ChatModel>` + `Entity<AgentModel>`. Simplify props.

- [ ] **Step 5: Add tool_approval to chat_panel/mod.rs**

Add `pub mod tool_approval;`.

- [ ] **Step 6: Verify full project compilation**

Run: `cargo check --workspace`
Expected: Entire workspace compiles.

- [ ] **Step 7: Commit**

```bash
git add src/alva-app/src/views/chat_panel/
git commit -m "feat(app): rewrite chat panel views with UIMessage parts rendering + tool approval"
```

---

## Task 15: Final integration + cleanup

**Files:**
- Modify: `src/alva-app/src/main.rs`
- Various cleanup

- [ ] **Step 1: Update main.rs if needed**

Ensure `main.rs` still creates models correctly with the new ChatModel. The GpuiChat instances are created lazily in `get_or_create_chat`, so main.rs should need minimal changes.

- [ ] **Step 2: Run full workspace check**

Run: `cargo check --workspace`
Expected: No errors.

- [ ] **Step 3: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 4: Update AGENTS.md files**

Run `/fractal-docs update` to update documentation for changed/new files.

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "feat: complete AI SDK React port — three-crate architecture with UIMessage streaming"
```
