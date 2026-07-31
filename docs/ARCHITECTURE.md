# Alva Architecture

This file is the architecture source of truth for the current tree.

Related living docs (not historical plans):

- `docs/specs/` — accepted product/runtime specs still in force
- `docs/ALVA-HUB-V1.md` / `docs/ALVA-HUB-GLOSSARY.md` — accepted Hub
  target and canonical domain vocabulary; target behavior is not current
  implementation
- `docs/adr/` — accepted Hub architecture decisions
- `docs/BUS-RULES.md` / `docs/BUS-TEMPLATES.md` — bus governance
- `docs/reviews/` — dated reviews; useful as backlog snapshots, not design truth
- `AGENTS.md` — crate inventory and extension philosophy

## Core Contract

Alva is a layered agent SDK plus product harnesses.

The stable SDK layers do not take product features directly. Optional behavior
is injected through:

- `Tool`: an LLM-callable action.
- `Plugin`: an assembly-time capability bundle.
- `Middleware`: an onion layer around the agent loop.
- `Phase` / `PhaseEffect`: kernel-owned runtime timeline vocabulary in
  `alva-kernel-abi`.
- `PhaseContribution`: the agent-core assembly contribution emitted by
  semantic helpers such as observers, policies, context, and remote plugins.

`Plugin` replaced the older Extension/HostAPI/EventBridge design. There is no
runtime plugin event bridge in the SDK. Plugins register tools, middleware,
phase contributions, bus capabilities, prompt fragments, and command metadata
through `Registrar`.

## Runtime Flow

```text
User surface
  CLI / Tauri / third-party harness
        |
        v
Harness assembly
  BaseAgentBuilder (opinionated) or AgentBuilder (SDK)
        |
        v
Plugin register()
  Registrar::tool/tools
  Registrar::phase / phase_handler
  Registrar::middleware
  Registrar::provide
  Registrar::system_prompt
  Registrar::command
        |
        v
AgentBuilder
  creates Bus / PluginHost
  builds ToolRegistry
  builds sorted MiddlewareStack
  calls Plugin::finalize()
  builds AgentState / AgentConfig
  records AgentAssemblySnapshot
        |
        v
Agent::run / BaseAgent::prompt_text
        |
        v
alva-kernel-core::run_agent
  input_committed hook
  ContextRuntime prepares request context
  before hooks
  LLM wrap chain
  ToolBatchCoordinator executes declared tool calls
  tool execution wrap chain
  after hooks
        |
        v
SessionEvent + AgentEvent
        |
        v
RunRecord / Inspector / CLI diagnostics
```

Alva Hub is a separate product path above the native harness. Native CLI and
Tauri do not consume the multi-engine contract:

```text
current: alva-hub-cli + alva-hub-desktop (ticket-01 walking skeleton)
target:  complete Desktop V1 (fallback + branches + Orca collaboration)
        |
        v
alva-hub-core
  product session != engine session
  dispatch / resume / cancel / permission routing
        |
        v
alva-engine-runtime (neutral contract)
        |
        +--> alva-engine-adapter-alva
        |          |
        |          v
        |    alva-app-core::BaseAgent
        |          |
        |          v
        |    the native SDK/kernel flow above
        |
        +--> alva-engine-adapter-claude
        |          |
        |          v
        |    Full: Node.js Agent SDK bridge
        |    Light: claude -p --output-format stream-json
        |
        +--> alva-engine-adapter-codex
                   |
                   v
             codex exec --json
```

## Layer Map

| Layer | Crates | Responsibility |
|---|---|---|
| L0 | `alva-kernel-bus` | typed capabilities, `BusWriter`/`BusHandle`, event bus |
| L1 | `alva-kernel-abi` | stable traits and value types: Tool, LanguageModel, Session, Message |
| L2 | `alva-kernel-core` | session-centric agent loop, middleware stack, context runtime, tool batch coordinator, runtime execution context |
| L2.5 | `alva-agent-core` | Plugin/Registrar/LateContext, AgentBuilder, Agent, assembly observability |
| L3 | `alva-agent-context` | context hooks, prompt layers, compaction, multi-agent scope/blackboard |
| L3 | `alva-agent-memory` | memory backend trait, in-memory backend, retrieval service |
| L3 | `alva-agent-security` | SecurityGuard, permission manager, path/url policy, security middleware |
| L3 | `alva-agent-graph` | Pregel-style state graph runtime; parallel to `run_agent`, not a plugin system |
| L4 | `alva-agent-extension-builtin` | built-in tools and lightweight plugin wrappers |
| L4 | `alva-app-extension-browser` | browser automation plugin; native/heavy |
| L4 | `alva-app-extension-memory` | SQLite memory backend; native/heavy |
| L4 | `alva-app-extension-loader` | AEP subprocess plugin loader |
| L4.5 | `alva-engine-runtime` | lightweight multi-engine request/event/control contract |
| L4.5 | `alva-engine-adapter-claude` | SDK-clean Claude dual-mode engine contract adapter: Full Node.js Agent SDK bridge; Light `claude -p --output-format stream-json`, default `--safe-mode` + fail-closed `dontAsk` (bypass explicit opt-in) |
| L4.5 | `alva-engine-adapter-codex` | SDK-clean Codex CLI (`exec --json`) implementation of the engine contract |
| L5 | `alva-app-core` | BaseAgent harness, component catalog, app-layer plugins, session projection |
| L5 | `alva-host-native` | native host assembly and native middleware |
| L5 | `alva-host-wasm` | wasm host facade |
| L6 | `alva-app-cli` | terminal product |
| L6 | `alva-app-tauri` | Tauri desktop product and Inspector |
| L7 | `alva-hub-core` | product-level engine registry and product/native session supervision |
| L7 | `alva-engine-adapter-alva` | canonical app-layer Alva adapter; builds one complete BaseAgent per engine session |
| L7 | `alva-hub-cli` | auxiliary Hub CLI with one-shot/REPL flows, private session journal, resume, manual switching, handoff, and diagnostics |
| L7 | `alva-hub-desktop` | Desktop-first Hub product; ticket 01 currently provides an Alva workspace/composer/timeline walking skeleton through the real EngineRuntime seam |

## Accepted Hub Target (Implementation Started)

The current tree proves multi-engine execution through `alva-hub-core`,
adapters, and `alva-hub-cli`. It now also contains a separate
`alva-hub-desktop` walking skeleton above those layers. The remaining accepted
V1 target expands that product:

- Desktop is the primary V1 surface; Hub CLI remains auxiliary.
- `alva-app-tauri` remains an independently maintained Alva-only product.
- one HubSession retains reversible engine branches and serial fallback
  attempts;
- operational continuity failures can automatically use a visible,
  user-authorized fallback chain;
- Cindy Orca-style TeamRuns provide one durable Lead and parallel durable
  cross-engine Workers;
- shared workspace is the default, with product-level resource coordination
  and optional isolated worktrees;
- permissions may tighten across adapters but never loosen.

The normative target, vocabulary, and decisions are
[`ALVA-HUB-V1.md`](./ALVA-HUB-V1.md),
[`ALVA-HUB-GLOSSARY.md`](./ALVA-HUB-GLOSSARY.md), and
[`docs/adr/`](./adr/README.md). Current crate-level behavior remains documented
by each crate's `AGENTS.md`.

## Plugin Assembly

`alva-agent-core::Plugin` has two phases:

```rust
async fn register(&self, r: &Registrar);
async fn finalize(&self, cx: &LateContext) -> Vec<Arc<dyn Tool>>;
```

Use `register()` to provide capabilities. Use `finalize()` only for work that
needs the full tool list, model, or capabilities provided by other plugins.

`AgentBuilder` records an `AgentAssemblySnapshot` containing:

- final plugin names
- final middleware names
- per-plugin registered tools
- per-plugin finalized tools
- per-plugin middleware
- per-plugin phase contributions
- per-plugin commands
- per-plugin prompt fragment count

CLI and Tauri write this into `eval_config_snapshot` as `plugin_names`,
`plugin_assembly`, and `middleware_names`.

## BaseAgent Defaults

`BaseAgentBuilder` delegates to `alva-agent-core::AgentBuilder` and adds harness
defaults:

- `system_context`
- `security`
- `memory`

To replace a default, register a plugin with the same `name()`. The builder
skips its default when a same-name plugin is already present.

Single-purpose middleware can be registered directly with `.middleware(...)`.
Capabilities that combine tools, middleware, bus services, prompt fragments, or
commands should be plugins.

## Component Catalog

`alva-app-core::components` is the shared switchboard for CLI, Tauri, and tests.
It contains:

- `COMPONENTS`: display/default metadata.
- `ComponentToggles`: user overrides.
- `ComponentContext`: harness-provided construction inputs.
- `apply_components`: the single attachment path.

This avoids each product hand-copying its own plugin/middleware stack.

## Platform Boundary

SDK crates must not depend on `alva-app-*` or `alva-host-*`. This is enforced by
`scripts/ci-check-deps.sh` (Rule 17). `alva-engine-runtime` remains part of that
SDK boundary; `alva-engine-adapter-alva` is deliberately outside it because the
canonical adapter now wraps the complete app-level `BaseAgent`.

Native product entrypoints must not depend on `alva-engine-*` (Rule 18):
`alva-app-cli` and `alva-app-tauri` stay on `BaseAgent`. Only Hub (and other
multi-engine products) consume `EngineRuntime`, so engine-contract changes do
not force a three-client blast radius.

wasm-clean crates are checked explicitly. Native-only crates such as
`alva-host-native` are not required to compile for wasm; wasm assembly belongs
in `alva-host-wasm`.

## Known Pressure Points

- `alva-app-core` still combines facade, harness, component catalog, app-layer
  plugins, and session projection. It is the main future split candidate.
- The module path `extension/` remains for historical compatibility even though
  the public abstraction is `Plugin`.
- Component display metadata and plugin contribution metadata are now both
  available, but dependency/conflict declarations are still future work.
