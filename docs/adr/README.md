# ALVA Hub Architecture Decision Records

These records capture accepted ALVA Hub product and architecture decisions.
They describe target constraints; the implementation status is tracked in the
linked product specification and current crate documentation.

| ADR | Status | Decision |
|---|---|---|
| [0001](./0001-alva-hub-desktop-first.md) | Accepted | Hub V1 is a new Desktop-first L7 product; existing Alva Desktop remains. |
| [0002](./0002-reversible-multi-engine-fallback.md) | Accepted | Explicit engine choice plus visible, reversible automatic fallback. |
| [0003](./0003-orca-style-collaboration.md) | Accepted | Cindy Orca-style durable Lead/Worker collaboration across engines. |
| [0004](./0004-hub-owned-agent-inspector.md) | Accepted | Retire the standalone Alva desktop direction and move recursive Alva inspection into Hub. |
| [0005](./0005-typed-durable-session-records.md) | Accepted | Keep one AgentSession while separating typed durable records, live state, and telemetry. |
| [0006](./0006-hub-tauri-ui-system.md) | Accepted | Build the Hub UI as a strict Tauri/React component system aligned to Cindy's structure and interaction. |
| [0007](./0007-agent-hub-abstraction-alignment.md) | Accepted | Introduce a swappable agent-loop interface; move Hub toward DSH's strengths selectively without cloning Cordis. |

The complete V1 scope and acceptance scenarios live in
[ALVA-HUB-V1.md](../ALVA-HUB-V1.md).
