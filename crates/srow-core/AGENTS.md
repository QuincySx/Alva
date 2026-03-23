# srow-core
> Thin facade over extracted agent crates, plus skill system, MCP, and environment management

## Role
`srow-core` is the central crate that re-exports public APIs from extracted
crates (`alva-types`, `alva-core`, `alva-tools`, `alva-security`,
`alva-memory`, `alva-runtime`) and keeps modules that have not yet been
extracted: ACP client, skills, MCP, environment runtime, domain models, and
DDD ports/adapters.

## Architecture
- **Facade re-exports** (`lib.rs`) — re-exports `Agent`, `AgentHooks`,
  `AgentEvent`, `AgentMessage` from `alva-core`; type vocabulary from
  `alva-types`; tool registrations from `alva-tools`; security from
  `alva-security`; memory from `alva-memory`; runtime builder from
  `alva-runtime`.
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
- Acts as backward-compat facade; UI layer (`srow-app`) imports through here

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
