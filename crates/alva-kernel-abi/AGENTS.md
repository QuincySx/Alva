# alva-kernel-abi
> Shared type vocabulary for the agent ecosystem

## Role
`alva-kernel-abi` defines the foundational traits and data types shared across all
agent crates. It has no runtime logic — only trait definitions, enums, and
structs.

## Architecture
- **Tool** (`tool/`) — `Tool` trait (async execute), `ToolContext` / `LocalToolContext`
  split (base context vs. filesystem-aware context), `EmptyToolContext`, `ToolRegistry`,
  wire types (`ToolCall`, `ToolResult`, `ToolDefinition`). `ToolSchemaContext` + the
  opt-in `Tool::parameters_schema_with` / `apply_schema_overrides_with` methods let
  a tool emit a JSON Schema that depends on live runtime state (e.g. a bus-registered
  `SpawnCommunicationRegistry`) — see `tool/schema.rs`.
- **Sessions** (`agent_session.rs`, `session_store.rs`, `conversation_log.rs`,
  `session_record.rs`) — `AgentSession` / `ScopedSession`, the `PersistentSessions`
  capability the kernel loop and tool path consume (`Arc<dyn PersistentSessions>`),
  the `SessionStore` harness superset, the `ConversationLog` kernel minimum, and
  versioned `SessionRecord` durable records. `StoreSessionCell` is the concrete
  typed-store cell assembly layers wrap (the kernel itself never sees it).
- **Message** (`base/message.rs`) — `Message`, `MessageRole`, `UsageMetadata`.
- **Content** (`base/content.rs`) — `ContentBlock` enum (Text, Reasoning, ToolUse,
  ToolResult, Image).
- **Model** (`model/`) — `LanguageModel` trait, `ModelConfig`.
- **Stream** (`base/stream.rs`) — `StreamEvent` enum for streaming LLM responses.
- **Error** (`base/error.rs`) — `AgentError` enum.
- **Cancel** (`base/cancel.rs`) — `CancellationToken`.
- **Multi-modal** — `EmbeddingModel`, `TranscriptionModel`, `SpeechModel`,
  `ImageModel`, `VideoModel`, `RerankingModel`, `ModerationModel`.
- **Scope** (`scope/`) — spawn communication contracts and
  `scope::context` (context hook lifecycle + `ContextApply`).
- **Runtime** (`runtime.rs`) — `Sleeper` / `timeout` without a tokio dependency.
  (`Provider` / `ProviderRegistry` moved to `alva-agent-abi` — agent-product
  vocabulary, not kernel mechanism.)

## Constraints
- Prefer pure contracts: traits, enums, value types. Runtime apply helpers
  (`apply_injections` / `apply_compressions`) live in `alva-agent-context::context_apply`.
- Concrete session backends (`InMemoryAgentSession`, `CellBackedSessions`) live
  in `alva-agent-core::agent_session`; this crate owns the session *contracts*
  (`AgentSession`, `PersistentSessions`, `SessionStore`, `ConversationLog`,
  `SessionRecord`) and the `StoreSessionCell` concrete cell wrapper.
- Scheduling **types** (`ResourceKey` / `LockMode` / `ExecutionMode`) stay
  here; the `ToolLockRegistry` runtime lives in `alva-agent-core::tool_lock`.
- `ToolRegistry` preserves first-registration order and reports replacement;
  `AgentSession::supports_atomic_records()` advertises whether tool batches can
  be committed without partial durable state.
- All traits are `Send + Sync`
- `ToolContext` is a trait (not a struct) to stay generic across deployments

## Module Map
| File | Public API | Role |
|------|-----------|------|
| `src/lib.rs` | re-exports | Crate root |
| `src/agent_session.rs` | `AgentSession`, `ScopedSession`, `PersistentSessions`, `SessionStore`-facing event/query types, `StoreSessionCell`, `EmitterKind`/`EventEmitter` | Session contracts: the kernel loop and tool path only see `Arc<dyn PersistentSessions>`; `StoreSessionCell` wraps a concrete typed store for assembly layers |
| `src/session_record.rs` | `SessionRecord` and typed payloads, `AppendReceipt` | Versioned durable record vocabulary |
| `src/session_store.rs` | `SessionStore` | Harness/UI superset session contract: query, count, rollback, snapshot, subscribe, compact, persist, spawn |
| `src/conversation_log.rs` | `ConversationLog` | The kernel's entire "history" requirement — read/append messages; `SessionStore` is the harness superset |
| `src/scope/` | `SpawnCommunication*`, `scope::context::{ContextHooks, ContextHandle, ContextSystem, ContextApply}` | Spawn communication contracts and the context hook lifecycle + apply executor |
| `src/runtime.rs` | `Sleeper`, `timeout`, `TimeoutError` | Runtime-agnostic sleep/timeout (kernel does not depend on tokio::time) |
| `src/tool.rs` (dir) | `Tool`, `ToolContext`, `LocalToolContext`, `EmptyToolContext`, `ToolCall`, `ToolResult`, `ToolDefinition`, `ToolRegistry`, `Scheduling` (`scheduler.rs`), schema hooks (`schema.rs`) | Tool abstraction, context hierarchy, scheduling descriptor, schema overrides |
| `src/base/` | `message.rs`, `content.rs`, `stream.rs`, `error.rs`, `cancel.rs` re-exports | L0 wire types: `Message`, `ContentBlock`, `StreamEvent`, `AgentError`, `CancellationToken` |
| `src/model/` | `LanguageModel`, `ModelConfig` (`config.rs`) | LLM model trait + config |
| `src/constants.rs` | tool/agent/UI constants | Shared limits |
