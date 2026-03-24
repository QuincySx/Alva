# alva-agent-core
> Hook-driven agent engine — double-loop LLM execution with middleware support

## Role
`alva-agent-core` is the framework-agnostic agent execution engine. It owns the
prompt/tool-call loop, hook-based configuration (`AgentHooks`), event
emission (`AgentEvent`), and an async middleware subsystem (onion model).

## Architecture
- **Agent** (`agent.rs`) — public handle; holds model, hooks, state, cancellation.
  Callers call `prompt()` and receive an `AgentEvent` stream.
- **Agent Loop** (`agent_loop.rs`) — double loop: outer handles follow-ups,
  inner drives LLM calls, tool execution, and steering injection.
  Middleware hooks fire at each lifecycle point (agent start/end, before/after
  LLM call, before/after tool call).
- **Tool Executor** (`tool_executor.rs`) — parallel or sequential tool
  execution with before/after hooks and middleware integration.
- **Types** (`types.rs`) — `AgentHooks` (renamed from AgentConfig), `AgentMessage`,
  `AgentState`, `AgentContext`, hook type aliases, `ConvertToLlmFn`.
- **Event** (`event.rs`) — `AgentEvent` enum for observability.
- **Middleware** (`middleware/`) — `Middleware` trait, `MiddlewareStack`,
  `Extensions` (type-safe key-value store), `MiddlewareError`.
  - `compression.rs` — `CompressionMiddleware` truncates old messages when
    estimated token count exceeds a threshold.

## Constraints
- Rust 2021 edition
- Async runtime: tokio
- No UI framework dependency — framework-agnostic
- All hooks are synchronous (`Fn` closures); middleware is async (`async_trait`)

## Module Map
| File | Public API | Role |
|------|-----------|------|
| `src/lib.rs` | re-exports | Crate root |
| `src/types.rs` | `AgentHooks`, `AgentMessage`, `AgentState`, `AgentContext`, `ConvertToLlmFn` | Core types and hook configuration |
| `src/agent.rs` | `Agent` | Public agent handle |
| `src/agent_loop.rs` | (crate-internal) | Double-loop execution |
| `src/tool_executor.rs` | (crate-internal) | Tool batch execution |
| `src/event.rs` | `AgentEvent` | Observable event enum |
| `src/middleware/mod.rs` | `Middleware`, `MiddlewareStack`, `MiddlewareContext`, `Extensions`, `MiddlewareError` | Async middleware subsystem |
| `src/middleware/compression.rs` | `CompressionMiddleware`, `CompressionConfig` | Context compression middleware |
