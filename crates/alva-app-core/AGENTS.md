# alva-app-core
> Thin facade over extracted agent crates, plus skill system, MCP, and environment management

## Role
`alva-app-core` is the harness crate for the product-facing agent stack. It
re-exports shared vocabulary from extracted crates and owns `BaseAgent`,
component presets, session projection, and app-level plugins. It no longer
re-exports the legacy `alva-host-native::AgentRuntimeBuilder`; app callers
should use `BaseAgentBuilder`, while SDK callers should use
`alva-agent-core::AgentBuilder`.

## Architecture
- **Facade re-exports** (`lib.rs`) — re-exports kernel event/config vocabulary,
  `alva-kernel-abi` types, built-in tool presets, security/memory types, and
  native model resolution. Runtime construction is through `BaseAgentBuilder`,
  not `AgentRuntimeBuilder`.
- **Kept modules**:
  - `agent/` — ACP client (`agent_client/`), session management, persistence.
  - `skills/` — Skill system (loader, store, injector, agent templates).
  - `mcp/` — MCP protocol layer and tool adapter.
  - `environment/` — Embedded runtime management (Bun, Node, Python, Chromium).
  - `gateway/`, `base/`, `system/` — infrastructure placeholders.
  - `domain/`, `ports/`, `adapters/` — DDD layers.
  - `error.rs` — `EngineError`, `SkillError` (with `From<MemoryError>`).
  - `ports/tool.rs` — `SrowToolContext` implementing both `ToolContext` and
    `LocalToolContext`.

## Constraints
- Rust 2021 edition
- Async runtime: tokio (full features)
- Persistent storage: rusqlite + tokio-rusqlite (WAL mode)
- Acts as backward-compat facade; UI layer (`alva-app`) imports through here

## Module Map
| Name | Path | Role |
|------|------|------|
| lib.rs | `src/lib.rs` | Facade re-exports + module declarations |
| error.rs | `src/error.rs` | EngineError, SkillError |
| agent/ | `src/agent/` | ACP client, persistence, session |
| ports/tool.rs | `src/ports/tool.rs` | SrowToolContext (ToolContext + LocalToolContext) |
| skills/ | `src/skills/` | Skill system (loader, store, injector, templates) |
| mcp/ | `src/mcp/` | MCP protocol, tool adapter |
| environment/ | `src/environment/` | Embedded runtime management |
| domain/ | `src/domain/` | Domain models (DDD) |
| ports/ | `src/ports/` | Port interfaces (DDD) |
| adapters/ | `src/adapters/` | Adapter implementations (DDD) |
| tests/ | `tests/` | Integration tests |
