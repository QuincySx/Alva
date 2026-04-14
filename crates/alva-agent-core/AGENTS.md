# alva-agent-core
> Agent-layer core: Extension trait, HostAPI, event dispatch, MockToolFs.

## Role
`alva-agent-core` holds the pure agent-internal extension machinery that used
to live inside `alva-app-core/src/extension/`, plus `MockToolFs` which used
to live in `alva-agent-tools`. It is the foundation on which built-in tools,
app-level extensions, and future agent runtimes are layered.

## Public Surface
Re-exported from `src/lib.rs`:
- `Extension` — the async extension trait (event hooks, command registration).
- `ExtensionHost` — registers extensions and dispatches `ExtensionEvent`s.
- `HostAPI` — capability handle passed to extensions (tool registration, bus, workspace).
- `ExtensionContext`, `FinalizeContext` — per-event execution contexts.
- `ExtensionEvent`, `EventResult` — the event payload and handler outcome.
- `ExtensionBridgeMiddleware` — kernel middleware that bridges host events into the kernel middleware stack.
- `RegisteredCommand` — extension-registered tool/command descriptor.
- `MockToolFs` — in-memory `ToolFs` implementation for tests.

## Dependency Policy
- Only `alva-kernel-abi` and `alva-kernel-core`.
- NO protocol crates, NO LLM providers, NO `tokio` process/fs, NO persistence.
- Compiles cleanly for `wasm32-unknown-unknown` (part of the CI wasm invariant).

## Module Map
| Name | Path | Role |
|------|------|------|
| `extension/mod.rs` | `src/extension/mod.rs` | `Extension` trait + public re-exports |
| `extension/host.rs` | `src/extension/host.rs` | `ExtensionHost` + `HostAPI` implementation |
| `extension/bridge.rs` | `src/extension/bridge.rs` | `ExtensionBridgeMiddleware` wiring host events into kernel middleware |
| `extension/context.rs` | `src/extension/context.rs` | `ExtensionContext` + `FinalizeContext` |
| `extension/events.rs` | `src/extension/events.rs` | `ExtensionEvent` + `EventResult` |
| `mock_fs.rs` | `src/mock_fs.rs` | `MockToolFs` in-memory test fixture |

## Where Things Do NOT Live
- Built-in tool implementations → `alva-agent-extension-builtin`.
- `LocalToolFs` native adapter → `alva-agent-extension-builtin`.
- `BaseAgent` and session wiring → still in `alva-app-core` (historical; planned for a later refactor).
