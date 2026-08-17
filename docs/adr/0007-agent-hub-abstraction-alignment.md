# ADR-0007: Agent Abstraction Depth and Hub Plugin Direction

- Status: Accepted
- Date: 2026-08-16
- Scope: alva-kernel-core loop shape, alva-agent-core plugin authority, alva-hub-core extensibility, and their alignment with DeepSeek Harness

## Context

A research pass compared Alva against DeepSeek Harness (DSH, `deepseek-ai/deepseek-harness`),
which is organized as "everything is a Cordis plugin". The comparison surfaced
two findings that this ADR records for decision, not as informal drift.

### Finding 1 — Alva's agent loop is a concrete function, not a swappable driver

DSH splits the agent loop cleanly:

```text
dsh-agent        interface Agent + AgentFactory   (zero loop dependency)
dsh-agent-loop   class ReactLoopAgent implements Agent   (the only concrete loop)
```

`dsh-agent` states the contract explicitly: "every plugin programs against the
`Agent` handle — it has zero loop dependency, so the loop is swappable." The
concrete `ReactLoopAgent` is a **resident driver**: messages are delivered by
`send/followup/steer/inject` into an inbox queue, a `status` state machine emits
`agent/status`, and every step/turn/LLM boundary is mediated by an event
dispatcher that external listeners can veto or modify.

Alva's equivalent is `alva-kernel-core::run_agent`, a `pub async fn`:

```rust
pub async fn run_agent(
    state: &mut AgentState,
    config: &AgentConfig,
    cancel: CancellationToken,
    input: Vec<AgentMessage>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), AgentError>
```

Its inputs are already interface-shaped (mutable `AgentState`, immutable
`AgentConfig`, `CancellationToken`, event channel). What it lacks is the
explicit **driver interface + factory** seam: there is no `Agent` trait whose
implementation can be swapped, and no factory registry. `run_agent` is a
one-shot function, not a resident queue-fed driver with a lifecycle status.

### Finding 2 — Hub has an engine seam, but not a plugin system

`alva-hub-core::HubRuntime` exposes `register_engine(Arc<dyn EngineRuntime>)`.
`EngineRuntime` is object-safe (`execute / cancel / respond_permission / capabilities`),
so any engine backend can already be registered. Today only the four official
adapters (alva / claude / codex / pi) use this seam. There is no third-party
surface above it: fallback policy, handoff, timeline projection, and team
orchestration are hard-wired product logic, not plugins.

### Finding 3 — Layer separation was previously conflated

Hub and the agent layer have different deployment envelopes:

- **Hub** (`alva-hub-core`, `alva-hub-desktop`, `alva-hub-server`) runs on a host
  that can install language runtimes (local desktop / container).
- **agent** (`alva-kernel-*`, `alva-agent-core`) is the portable layer intended
  to run on cloud / WASM / web / iOS.

AEP (`alva-app-extension-loader`, spawns `python3`/`node` subprocesses) is a
**host-layer** third-party-injection mechanism. It must not be treated as the
agent layer's cross-environment extension story, and `alva-worker-wasm`'s
QuickJS `run_script` is an agent *tool*, not a plugin mechanism — these two were
conflated during research.

### Finding 4 — Alva's capability inventory vs DSH

The agent layer already has: event stream (`SessionEvent`/`AgentEvent` +
`ListenableInMemorySession`), cancellation (`CancellationToken`), permission
approval (`PermissionManager`/`ApprovalPlugin`), session scoping (`ScopedSession`),
sub-agent dispatch (`SubAgentPlugin` + `agent-graph` `SendTo`), workflow
orchestration (`alva-agent-graph`, LangGraph-style), and compaction
(`CompactionMiddleware`/`auto_compact`).

The one capability DSH has that Alva lacks is the **swappable loop interface**.
This is the substance of the "does the underlying agent limit Hub?" question:
the capability inventory does not limit Hub; the absence of a loop interface
limits the freedom to replace the loop the way DSH does.

## Decision

### Order: agent loop first

The agent-loop interface is advanced before the hub plugin surface. Hub
pluginization ultimately drives agents through `EngineRuntime`; a clear loop
interface gives that work a stable lower layer, so it is the correct first step.

### Agent loop: introduce a swappable driver interface, keep `run_agent` as default

Borrow DSH's one structural idea — the `Agent` interface / `AgentFactory`
separation that makes the loop swappable — without adopting Cordis, the inbox
queue model, or the event-dispatch veto machinery outright. Concretely:

- An object-safe loop/driver interface lives in `alva-agent-core` (or
  `alva-kernel-core` if it must stay kernel-local; the exact home is a
  follow-up research item).
- The existing `run_agent` remains the default implementation behind that
  interface, so no current consumer breaks.
- The interface is shaped around the inputs `run_agent` already has
  (`AgentState`, `AgentConfig`, `CancellationToken`, event channel), not a new
  parallel model.

### Hub: absorb DSH's advantages selectively, do not clone it

Hub moves toward DSH's strengths — swappable loop, resident-driver follow-up,
and a plugin/assembly surface — but deliberately:

- Does **not** adopt Cordis or a second DI framework.
- Does **not** make UI/tools/skills/session all one plugin system on day one.
- Absorbs only what reduces real coupling: first the loop seam, then the
  highest-value plugin surface, judged case by case.

### Deliverable order: document, then research, then code

This round produces the accepted ADR. The next step is targeted research into
the agent-loop interface (where it lives, its exact surface, and migration
impact), not immediate implementation.

## Consequences

### Positive

- `run_agent` gains a swappable seam without breaking existing consumers, so a
  future resident-driver or non-standard loop can be experimented with.
- Agent layer and hub layer are recorded as separate deployment envelopes,
  preventing the AEP/worker-wasm conflation that occurred during research.
- Hub's plugin direction is bounded: it absorbs DSH's structural wins without a
  wholesale Cordis-style rewrite.

### Negative

- The agent-loop interface is a real kernel-adjacent change with CI boundary
  implications (Rule 17) that must be verified.
- "Absorb selectively" is a judgment line, not a crisp spec; each later hub
  step still needs its own scoping decision.

## Verification

- ADR accepted and committed to `docs/adr/`.
- Follow-up research (next step) settles the interface home and surface before
  any code lands.
- When code lands: `cargo build` clean, `scripts/ci-check-deps.sh` green, and
  no existing `run_agent` consumer changes behavior.

## Follow-up Research — Loop Interface Location and Surface

This section records the targeted research that followed the acceptance above;
it grounds the interface's home, surface, and migration impact before any code
lands.

### Home: `alva-kernel-core`

The dependency direction fixes the home without a preference call:

```text
alva-kernel-core → alva-kernel-abi
alva-agent-core  → alva-kernel-abi + alva-kernel-core
```

`run_agent`'s parameter types (`AgentState`, `AgentConfig`, `AgentEvent`) are
owned by `alva-kernel-core`, and `alva-agent-core` already depends on it. A
driver interface therefore belongs in `alva-kernel-core`, with `run_agent` kept
as its default implementation. This honors the Rule 17 stable-SDK posture
(no reverse dependency) without moving kernel types upward.

### Migration impact: three real callers

`run_agent` is called directly from exactly three non-test sites:

| Caller | File |
|---|---|
| SDK `Agent::run` | `alva-agent-core/src/agent.rs:90` |
| Harness `BaseAgent` | `alva-app-core/src/base_agent/agent.rs:90` |
| WASM facade | `alva-host-wasm/src/agent.rs:136` |

Introducing an interface whose default implementation is the current
`run_agent` changes behavior at zero of these sites; the only edit is routing
the call through the interface. The many `run_agent(` calls under
`alva-kernel-core/tests/` continue to test the default implementation directly.

### Surface: minimal, execution-only

The first interface exposes only the one action `run_agent` already has, and
deliberately does **not** import DSH's queue/stateness machinery now:

| DSH `Agent` member | Verdict |
|---|---|
| execute / run | ✅ borrow — this is `run_agent` |
| cancel + signal | ✅ already covered by `CancellationToken` |
| status (idle⇄running) | ⚠️ later — `AgentEvent::AgentStart/AgentEnd` already expresses lifecycle |
| send / followup / steer / inject (inbox queue) | ⚠️ later — `BaseAgent::pending_messages` simulates follow-up today; a queue is a separate, larger decision |
| whenIdle / runMaintenance | ❌ do not import — Cordis fiber-lifecycle coupled |
| inbox / dispatch event veto | ❌ do not import — `MiddlewareStack` provides the equivalent |

The first step is therefore a thin `execute(state, config, cancel, input,
event_tx)` seam, not a rewrite of the loop model. This keeps "interface" from
inflating into "re-architecture", consistent with the selective-absorption
decision above.

## Implementation Note

The interface landed (`7fd8ae0`) with two preparatory commits and one follow-up
proof, all wider than ticket 01 had scoped:

- `bebc7d6` made `Extensions` interior-mutable (`Arc<RwLock>`, `get` returns
  `Arc<T>`).
- `0f44bb8` changed `&mut AgentState → &AgentState` across the **entire**
  `Middleware`/`PhaseHandler` trait and every cross-crate implementation — 27
  files — not just the one `state.extensions.insert(cancel)` site ticket 01 had
  assumed. This is the fallback ticket 01's contingency clause called for, and
  it had not been written back until now.
- `9003f7f` proved swappability with a second `AgentLoop` implementation.

Why the broad `&mut → &` is still safe: it is a pure signature change. The only
real mutation `run_agent` performed on `state` was `extensions.insert(cancel)`;
`state.tools`/`state.model`/`state.session` are all read through `Arc`, and no
code wrote those fields. Rust's borrow checker guarantees no path mutates them
through the old `&mut` (it would not compile otherwise), and non-trivial files
were spot-checked for logic drift. `cargo build --workspace`, `scripts/ci-check-deps.sh`
(including the wasm32 link build), and `cargo test -p alva-kernel-core` all pass.

One wording correction carried forward from the implementation: the reason for
`&mut → &` was the onion composition of `wrap_llm_call`/`wrap_tool_call`
(an exclusive borrow cannot thread cleanly through the nested `&dyn LlmCallFn`/
`ToolCallFn` continuations) — **not** an object-safety requirement. `#[async_trait]
fn drive(&self, state: &mut AgentState, …)` is itself object-safe; Rust's
object-safety rules do not constrain the mutability of non-`self` parameters.
The corrected rationale lives in the `Middleware` trait doc comment.
