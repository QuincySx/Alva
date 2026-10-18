# Alva

Layered AI Agent SDK and products, with Go as the target implementation.

The target stack is moving to Go with a self-drawn MyGo desktop, without React
or a Rust compatibility bridge. The first runnable foundation lives in
[go/README.md](./go/README.md); [go/MIGRATION.md](./go/MIGRATION.md) separates
verified work from pending product parity. The Rust architecture below remains
the behavior reference until that acceptance is complete; existing data and
dependencies are not removed automatically.

The Go implementation is open source under [AGPL-3.0-only](./go/LICENSE).
Its pinned RelayKit conversion dependency and attribution are documented in
[go/NOTICE](./go/NOTICE); this does not relicense the historical Rust tree.

The core architectural contract is: stable SDK kernel, optional capability by
plugin. `alva-kernel-*` and `alva-agent-core` provide the reusable SDK surface;
product-specific harness decisions live in `alva-app-*` / `alva-host-*`.

## Go: start with the layer map

The current layout preserves each abstraction, with no old-path aliases:

| Directory | Role |
|---|---|
| `go/sdk/` | Portable L0–L3: values/bus/ABI → kernel → Agent assembly/capabilities |
| `go/extension/` | L4 optional tools, plugins and middleware |
| `go/harness/` | L5 opinionated BaseAgent recipe, not the SDK |
| `go/apps/` | L6 direct/Gateway and L7 Hub products; thin executables in apps/cmd |
| `go/integration/` | External model/protocol/engine adapters; only the engine root contract is L4.5 |
| `go/platform/` | Host execution ownership, low-level system primitives, durable storage and WASM implementations |
| `go/architecture/` | Tagged dependency, public-surface and documentation gates |

Read [the L0→L7 map and reading order](./go/README.md#先看地图l0--l7), then the
relevant directory's AGENTS.md. Integration/platform are responsibility groups,
not a forced linear layer: host may consume the harness Runner, while system
provides lower-level OS primitives. Alva engine bridging consumes the complete
harness; the process adapter shares Claude/Codex/Pi implementation by Vendor.
Neither adapter is part of the portable SDK.

This reclassification is not a claim of completed feature parity. Rust crate
paths and the behavior-reference architecture below remain unchanged.

## Architecture Flow (Rust behavior reference)

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
| L4.5 | `alva-engine-runtime`, `alva-engine-adapter-{claude,codex}` | SDK-clean multi-engine contract and external engine adapters |
| L5 | `alva-app-core`, `alva-host-*` | opinionated harness and platform assembly |
| L6 | `alva-app-cli`, `alva-app-tauri` | end-user products |
| L7 | `alva-hub-core`, `alva-engine-adapter-alva`, `alva-hub-{cli,desktop}` | multi-engine Hub product aggregation; Desktop tickets 01–03 provide the Alva timeline, private SQLite session/turn/attempt persistence, resume, and crash repair |

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
- [docs/ALVA-HUB-V1.md](./docs/ALVA-HUB-V1.md): accepted, incrementally
  implemented Hub Desktop V1 product and acceptance contract.
- [docs/ALVA-HUB-GLOSSARY.md](./docs/ALVA-HUB-GLOSSARY.md): canonical Hub
  domain vocabulary.
- [docs/adr/README.md](./docs/adr/README.md): accepted Hub architecture
  decisions.
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
