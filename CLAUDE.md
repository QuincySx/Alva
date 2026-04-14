# Project Instructions

> **All project-specific instructions live in [AGENTS.md](./AGENTS.md). Read it before doing anything in this repository.**

`AGENTS.md` is the single source of truth for:

- The architectural philosophy (stable kernel + agent-core, everything else as `Extension` / `Tool` / `Middleware`)
- The full crate inventory across all 29 workspace crates, organized by layer
- The default-replacement contract (how to swap built-in extensions like `MemoryExtension` / `SecurityExtension` by registering same-named extensions)
- The `agent-graph` exception (it's a state-machine library, not an extension)
- The CI Rule 17 SDK→app/host boundary
- Git commit conventions
- GPUI guidance, bus rules, and compact instructions

When in doubt, read `AGENTS.md`. Do not duplicate or fork its content here.
