# Alva

Rust implementation of a layered AI Agent framework.

The core architectural contract is: stable SDK kernel, optional capability by
plugin. `alva-kernel-*` and `alva-agent-core` provide the reusable SDK surface;
product-specific harness decisions live in `alva-app-*` / `alva-host-*`.

## Architecture Flow

```text
CLI / Tauri / third-party harness
        |
        v
BaseAgentBuilder or AgentBuilder
        |
        v
Plugin register phase
  - tools
  - middleware
  - phase contributions / phase handlers
  - bus capabilities
  - system prompt fragments
  - slash command metadata
        |
        v
AgentBuilder assembly
  - Bus / BusWriter
  - PluginHost
  - ToolRegistry
  - MiddlewareStack
  - AgentState / AgentConfig
  - AgentAssemblySnapshot
        |
        v
Agent / BaseAgent
        |
        v
alva-kernel-core::run_agent
  - InputCommitted hook fires after input is persisted
  - ContextRuntime prepares LLM request context
  - ToolBatchCoordinator commits declared tool results in model order
        |
        v
LLM call <-> middleware onion <-> tool batch execution
        |
        v
AgentEvent / SessionEvent / RunRecord / Inspector
```

## Layers

| Layer | Crates | Role |
|---|---|---|
| L0 | `alva-kernel-bus` | typed capabilities, event bus, state cell |
| L1 | `alva-kernel-abi` | stable contracts: Tool, LanguageModel, Session, Message |
| L2 | `alva-kernel-core` | `run_agent`, AgentState/AgentConfig, MiddlewareStack, ContextRuntime, ToolBatchCoordinator |
| L2.5 | `alva-agent-core` | Plugin/Registrar, AgentBuilder, Agent, assembly snapshot |
| L3 | `alva-agent-{context,memory,security,graph}` | reusable capability libraries |
| L4 | `alva-agent-extension-builtin`, `alva-app-extension-*` | built-in and heavy optional plugin implementations |
| L5 | `alva-app-core`, `alva-host-*` | opinionated harness and platform assembly |
| L6 | `alva-app-cli`, `alva-app-tauri` | end-user products |

## Plugin Model

Use the smallest fitting abstraction:

- `Tool`: an LLM-callable verb.
- `Plugin`: a capability bundle that registers tools, middleware, bus
  capabilities, phase contributions, prompt fragments, and command metadata.
- `Middleware`: an onion layer around the agent loop.

`BaseAgentBuilder` installs default `memory`, `security`, and
`system_context` plugins unless the caller registers a plugin with the same
`name()`. There are no ad-hoc setters such as `with_memory()`; replacement is
by same-name plugin.

## Important Docs

- [AGENTS.md](./AGENTS.md): agent-facing project map and repository rules.
- [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md): current architecture detail.
- [FRACTAL-DOCS.md](./FRACTAL-DOCS.md): required documentation protocol.
- [docs/BUS-RULES.md](./docs/BUS-RULES.md): bus anti-God-object rules.

## Boundary Checks

Run:

```bash
scripts/ci-check-deps.sh
```

The script enforces SDK -> app/host dependency boundaries, bus surface limits,
and wasm32 checks for crates that are intended to be wasm-clean. Native host
assembly lives in `alva-host-native`; wasm host assembly lives in
`alva-host-wasm`.
