# alva-agent-core
> Agent-layer core: Plugin/Registrar assembly, AgentBuilder, Agent facade, MockToolFs.

## Role
`alva-agent-core` holds the pure SDK-level agent assembly machinery: plugins
register tools, phase contributions, middleware, bus capabilities, commands,
and system prompt fragments through a `Registrar`; `AgentBuilder` runs the
plugin lifecycle and produces an `Agent` backed by
`alva-kernel-core::run_agent`. It also contains
`MockToolFs`, a lightweight in-memory `ToolFs` test fixture.

## Public Surface
Re-exported from `src/lib.rs`:
- `Agent` — SDK-level runnable agent facade.
- `AgentAssemblySnapshot` / `PluginAssemblySnapshot` — build-time plugin,
  phase, middleware, tool, command, and prompt contribution snapshots used by
  harnesses and CLIs for observability.
- `AgentBuilder` — SDK-level builder that assembles model, session, plugins,
  tools, middleware, bus, and context config.
- `extension::Plugin` — self-contained capability bundle with
  `register()` and optional late `finalize()`.
- `extension::Registrar` — setup handle passed to plugins for registering
  tools, phase contributions, middleware, bus capabilities, system-prompt
  fragments, and commands.
- `alva_kernel_abi::{Phase, PhaseEffect}` — kernel-owned stable runtime
  timeline vocabulary and effect categories.
- `extension::{PhaseContribution, PhaseOrder}` — agent-core assembly
  descriptors targeting the kernel phase vocabulary. `Registrar::phase(...)`
  records metadata-only contributions.
- `extension::PhaseHandler` — executable phase contribution registered through
  `Registrar::phase_handler(...)`. Agent-core compiles it into the current
  middleware stack while kernel-native phase execution is pending.
- `extension::LateContext` — read context for late tool discovery and
  cross-plugin wiring after all `register()` calls finish.
- `PluginHost` — runtime container for plugin-registered middleware,
  command metadata, prompt fragments, and cancellation binding.
- `RegisteredCommand` — plugin-registered slash-command descriptor.
- `MockToolFs` — in-memory `ToolFs` implementation for tests.

## Dependency Policy
- Only `alva-kernel-abi` and `alva-kernel-core`.
- NO protocol crates, NO LLM providers, NO `tokio` process/fs, NO persistence.
- Compiles cleanly for `wasm32-unknown-unknown` (part of the CI wasm invariant).

## Bus Assembly Rule
- `AgentBuilder::with_bus_writer(...)` is the normal external-bus path when
  plugins are registered. Plugins receive a `Registrar` and may publish typed
  capabilities through `Registrar::provide(...)`, which requires the writer for
  that same bus.
- `AgentBuilder::with_bus(...)` is handle-only and therefore read-only. It is
  only valid for plugin-less assembly; `build()` rejects handle-only bus usage
  when plugins are present so capability registration cannot disappear into a
  throwaway bus.

## Module Map
| Name | Path | Role |
|------|------|------|
| `agent.rs` | `src/agent.rs` | Runnable `Agent` facade over kernel state/config plus assembly snapshot |
| `agent_builder.rs` | `src/agent_builder.rs` | Plugin lifecycle, SDK-level assembly, and build metadata capture |
| `extension/mod.rs` | `src/extension/mod.rs` | Plugin system public re-exports |
| `extension/plugin.rs` | `src/extension/plugin.rs` | `Plugin` trait (`register` + `finalize`) |
| `extension/phase.rs` | `src/extension/phase.rs` | Stable phase contribution descriptors |
| `extension/registrar.rs` | `src/extension/registrar.rs` | `Registrar` and `LateContext` |
| `extension/host.rs` | `src/extension/host.rs` | `PluginHost`: runtime container for middleware, commands, prompt fragments, cancellation |
| `mock_fs.rs` | `src/mock_fs.rs` | `MockToolFs` in-memory test fixture |

## Where Things Do NOT Live
- Built-in tool implementations → `alva-agent-extension-builtin`.
- `LocalToolFs` native adapter → `alva-agent-extension-builtin`.
- `BaseAgent` and session wiring → still in `alva-app-core` (historical; planned for a later refactor).
