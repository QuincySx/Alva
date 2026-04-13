# alva-kernel-abi
> Shared type vocabulary for the agent ecosystem

## Role
`alva-kernel-abi` defines the foundational traits and data types shared across all
agent crates. It has no runtime logic — only trait definitions, enums, and
structs.

## Architecture
- **Tool** (`tool.rs`) — `Tool` trait (async execute), `ToolContext` / `LocalToolContext`
  split (base context vs. filesystem-aware context), `EmptyToolContext`, `ToolRegistry`,
  wire types (`ToolCall`, `ToolResult`, `ToolDefinition`).
- **Message** (`message.rs`) — `Message`, `MessageRole`, `UsageMetadata`.
- **Content** (`content.rs`) — `ContentBlock` enum (Text, Reasoning, ToolUse,
  ToolResult, Image).
- **Model** (`model.rs`) — `LanguageModel` trait, `ModelConfig`.
- **Stream** (`stream.rs`) — `StreamEvent` enum for streaming LLM responses.
- **Error** (`error.rs`) — `AgentError` enum.
- **Cancel** (`cancel.rs`) — `CancellationToken`.
- **Multi-modal** — `EmbeddingModel`, `TranscriptionModel`, `SpeechModel`,
  `ImageModel`, `VideoModel`, `RerankingModel`, `ModerationModel`.
- **Provider** (`provider.rs`) — `Provider` trait, `ProviderError`, `ProviderRegistry`.

## Constraints
- Zero runtime dependencies (no tokio, no IO) beyond async-trait and serde
- All traits are `Send + Sync`
- `ToolContext` is a trait (not a struct) to stay generic across deployments

## Module Map
| File | Public API | Role |
|------|-----------|------|
| `src/lib.rs` | re-exports | Crate root |
| `src/tool.rs` | `Tool`, `ToolContext`, `LocalToolContext`, `EmptyToolContext`, `ToolCall`, `ToolResult`, `ToolDefinition`, `ToolRegistry` | Tool abstraction and context hierarchy |
| `src/message.rs` | `Message`, `MessageRole`, `UsageMetadata` | LLM message types |
| `src/content.rs` | `ContentBlock` | Message content blocks |
| `src/model.rs` | `LanguageModel`, `ModelConfig` | LLM model trait |
| `src/stream.rs` | `StreamEvent` | Streaming response events |
| `src/error.rs` | `AgentError` | Error types |
| `src/cancel.rs` | `CancellationToken` | Cooperative cancellation |
| `src/provider.rs` | `Provider`, `ProviderError`, `ProviderRegistry` | Provider trait and registry |
