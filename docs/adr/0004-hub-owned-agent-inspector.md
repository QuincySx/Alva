# ADR-0004: Hub-owned Agent Inspector

- Status: Accepted
- Date: 2026-08-11
- Scope: Alva desktop retirement, Inspector ownership, visibility, and trace depth

## Context

The standalone Alva desktop product mainly retains one capability that is not
already better placed in Hub: the Agent Dev/Inspector view for understanding
each model round, tool call, middleware step, and nested agent run.

Hub is now the primary desktop product and owns durable product conversations
across Alva, Claude, and Codex. Keeping a second desktop product solely for
Alva-only diagnostics fragments the user experience and duplicates desktop UI
work.

The available trace depth is not equal across engines. Alva owns its execution
loop and can expose full run records, including recursively nested sub-agent
runs. Claude and Codex adapters currently expose normalized runtime events, not
equivalent prompt, middleware, and nested-run internals.

### Current Alva implementation

- Alva persists the complete `SessionEvent` stream in SQLite rather than
  storing a second serialized `RunRecord`.
- Inspector projects `RunRecord` from the event stream on demand.
- Child-agent events are delimited inside the parent stream and recursively
  projected into `ToolCallRecord.sub_run`.
- Deleting a session cascades to its persisted events and snapshot.
- The current persistence path has no Inspector-specific secret redaction,
  payload-size cap, or retention limit.
- The current desktop Inspector loads a point-in-time projection when selected;
  Hub must add live incremental refresh to satisfy the new real-time decision.
- ADR-0005 replaces the stringly typed persistence schema while preserving one
  ordered `AgentSession` source of truth.

## Decision

### Product ownership

- The standalone Alva desktop product is formally retired as a product
  direction. This retirement covers `alva-app-gpui` and `alva-app-tauri`.
- `alva-app-cli`, `alva-app-core`, the Alva engine, and Alva's inspection data
  capability remain supported.
- Agent Dev/Inspector moves into Alva Hub.
- HubSession is the Inspector entry point and aggregate context.
- Existing Turn and EngineAttempt identities remain visible inside that
  HubSession instead of being flattened into one opaque trace.
- Inside an Alva attempt, the current Alva Inspector hierarchy and recursive
  run-record semantics remain unchanged.

### Visibility

- Inspector is a developer-facing surface hidden by default.
- One global experimental-feature switch controls whether Inspector surfaces
  are shown. Hub does not introduce a separate Developer Mode.
- The switch controls visibility only, not trace capture.
- Normal chat operation remains uncluttered when the switch is off.

### Chat-first presentation

- Inspector lives in Hub's shared resizable right-side tool workspace rather
  than a dedicated window or top-level product page.
- The same workspace hosts Terminal, Git history, Agent, Inspector, and other
  developer tools as switchable tabs.
- Opening Inspector keeps the active HubSession chat visible.
- Alva trace updates are visible while an attempt is running and remain
  inspectable after the attempt finishes.
- Agent and Inspector are separate tabs. Agent presents child agents associated
  with the current thread; Inspector is a debugging visualizer for the
  serialized execution record and does not own child-agent product navigation.

### Engine support

- Alva receives the complete Inspector experience.
- Alva traces preserve recursive child-agent relationships so users can drill
  from a HubSession through nested runs without a fixed depth limit.
- Claude and Codex are not required to expose the same internal detail.
- Claude and Codex expose whatever truthful diagnostic detail their adapters
  can observe, including nested relationships if an adapter can provide them.
- Missing external-engine detail is acceptable; Hub must not fabricate parity
  with Alva internals.
- Hub discovers inspection through the separate optional `EngineInspector`
  capability defined by ADR-0005. `EngineRuntime` remains focused on execution,
  cancellation, permission response, and execution capabilities.

### Retired Alva App pages

- The old Alva App navigation is not copied into Hub.
- Skills, MCP, and Tool Picker no longer require dedicated Alva App pages.
- Any management capabilities that remain useful follow Hub's settings and
  chat-first interaction model.
- Provider, model, and gateway flows use Hub's product design as the source of
  truth.

## Consequences

### Positive

- Hub becomes the single desktop surface for both normal work and optional
  execution diagnostics.
- Alva's unique observability remains available without maintaining a separate
  desktop product.
- Recursive child-agent inspection fits the HubSession context rather than
  appearing as disconnected top-level logs.
- External engines can improve incrementally without blocking the Alva
  Inspector.
- Terminal, Git, and Agent diagnostics share one predictable auxiliary
  workspace without displacing the chat-first primary surface.

### Negative

- Inspector storage must handle sensitive prompts, tool arguments, tool
  results, and nested runs safely.
- External-engine traces will be less detailed than Alva traces unless their
  adapters gain stronger observability contracts.
- Retiring the standalone desktop requires an explicit migration and deletion
  gate; this ADR does not authorize immediate crate deletion.

### Retirement gate

- `alva-app-gpui` and `alva-app-tauri` may be deleted after Hub ships the
  real-time and historical Alva Inspector, recursive nested-run inspection,
  the experimental visibility switch, and packaged acceptance coverage.
- This Alva App retirement gate does not wait for unrelated TeamRun work.
- Deleting `alva-hub-desktop` remains governed by the separate full Hub GPUI
  parity gate.
- No existing Alva App SQLite sessions are migrated into Hub.

## Verification

- With the Inspector experiment disabled, Hub remains a chat-first product without
  Inspector navigation.
- With the Inspector experiment enabled, a HubSession exposes its execution history.
- The Inspector updates during an active Alva attempt and remains available
  afterward.
- Terminal, Git, and Agent inspection switch inside the shared right-side tool
  workspace without opening another application window.
- An Alva attempt exposes full run details and recursive child-agent runs.
- Claude and Codex never claim unavailable Alva-only trace detail.
- Neither `alva-app-gpui` nor `alva-app-tauri` is deleted until the follow-up deletion gate is
  accepted and verified.
- Existing Alva App SQLite data is not imported into Hub.
