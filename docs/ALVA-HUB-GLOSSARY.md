# ALVA Hub Domain Glossary

> Status: Accepted vocabulary for the ALVA Hub Desktop V1 target.
>
> When product copy, persistence, APIs, and code use different words for the
> same concept, this document is the naming authority.

## Identity and Runtime

### HubSession

The durable product conversation owned by the user. It binds a workspace,
permission profile, fallback policy snapshot, timeline, mainline execution, and
an optional TeamRun. It is also the aggregate entry point for developer trace
inspection; inspection does not create a parallel session identity.

A HubSession ID is never an engine-native session ID.

Avoid: “thread” or “chat” in domain APIs when the value is a HubSession.

### Engine

An agent harness that can execute work through `EngineRuntime`. V1 engines are
Alva, Claude, and Codex.

An Engine is not a model and not a provider.

### Model

The inference model selected inside an Engine, such as an engine-supported
Claude or GPT model.

### Provider

The account, endpoint, or billing source through which a model is invoked.
Multiple providers may expose the same model ID.

### EngineTarget

One fallback destination: Engine ID plus model policy, provider/account source,
automatic-use authorization, and required capabilities.

### EngineSessionId

The opaque native session identity returned by an Engine. It belongs to one
EngineBranch and must always be qualified by engine identity.

Never call an EngineSessionId a ProductSessionId.

### EngineBranch

The durable binding between one product execution session and one Engine's
native session. It records engine/model/provider identity, native session ID,
watermark, status, and handoff history.

One execution session may retain several EngineBranches so the user can return
to an earlier engine without starting over.

### Active branch

The EngineBranch selected for the next execution attempt.

### Parked branch

An inactive EngineBranch whose native session and watermark are retained for a
possible return.

Parked does not mean archived, deleted, failed, or currently running.

## Turns and Attempts

### Turn

One user instruction and all automatic EngineAttempts used to complete it.

Automatic fallback does not create a new Turn.

### EngineAttempt

One bounded execution of a Turn on one EngineTarget. It has start/terminal
timestamps, events, usage, failure reason, side-effect evidence, and outcome.

EngineAttempts in one fallback chain are serial. Parallel work belongs to
WorkerSessions, not to attempts of the same Turn.

### Inspector visibility switch

The global experimental-feature switch that reveals Agent Inspector surfaces.
It is off by default so normal Hub operation remains chat-first.

The switch controls visibility only. It is not a separate product mode and does
not control trace capture or retention.

### Agent Inspector

The HubSession-scoped surface for examining execution details. It preserves
Turn and EngineAttempt identity and may expose engine-specific detail.

Alva preserves its current full recursive run inspection. Claude and Codex show
whatever truthful detail their adapters expose and are not assumed to provide
equivalent internals.

Agent Inspector is shown inside the shared right-side Tool Workspace. It is not
a separate window or top-level product mode.

### Tool Workspace

The resizable auxiliary workspace attached to the right side of the active Hub
chat. It hosts separate switchable tools such as Terminal, Git history, Agent,
and Inspector while keeping the HubSession conversation visible.

### Agent tab

The Tool Workspace surface for child agents associated with the current
thread. It presents product relationships and navigation between those agents.

Agent is not Inspector. Agent answers which child agents belong to the work;
Inspector explains how one recorded execution behaved.

### Alva run trace

The full execution record for one Alva run, including model rounds, messages,
tool calls, middleware observations, usage, errors, and recursively linked child
agent runs. Child runs remain navigable without a fixed nesting depth.

### SessionRecord

The versioned durable envelope written through Alva `AgentSession`. Conversation
and execution payloads share one monotonically ordered sequence. Built-in
payloads are typed; plugin payloads use a namespaced type and schema version.

A SessionRecord is not a streaming UI event and is not a telemetry span.

### Durable commit

A successful AgentSession append acknowledgement. After it returns, the record
and any provisioned settlement identifiers survive process restart. External
LLM and tool effects that require recoverability start only after their intent
record has durably committed.

### Conversation tree

The append-only parent-linked set of conversation entries inside one Alva
AgentSession. A current head selects the active branch. Returning to an older
entry moves the head or creates a new branch; it does not delete history.

### Child run link

The durable relationship from one spawning tool call to a child AgentSession,
identified by parent session, parent run, and tool-call identities. Inspector
uses the link for recursive navigation without copying child records into the
parent log.

### Live Inspector state

The process-local snapshot and event stream used to render an active run,
including assistant drafts, running tools, and progress. It overlays durable
SessionRecords and may disappear after a process crash.

### EngineInspector

The optional engine capability used by Hub to query historical Inspector
snapshots, subscribe to live inspection state, traverse child runs, and clean
up engine-native trace storage. It is separate from `EngineRuntime` so an
engine may execute sessions without claiming inspection support.

Inspection support is reported truthfully as Full, Summary, or Unsupported.

### Inspector catalog entry

The Hub-owned link from an EngineAttempt to a qualified engine-native session
and its advertised inspection capability. It is a bounded product index, not a
copy of the engine's full trace.

### Inspector blob

A content-addressed, compressed payload stored outside SQLite when a prompt,
response, tool argument, or tool result is too large for an inline record. A
SessionRecord retains a bounded preview and the blob's hash, media type, size,
and reference.

### Inspector Bundle

A redacted JSONL export of one HubSession's product identities, ordered
inspection records, recursive child-session links, and blob manifest. It does
not export recognized secrets or unrelated native engine files.

### Attempt cycle

The finite sequence of automatic attempts started by one user action. Each
EngineTarget may appear at most once. A user-requested retry starts another
cycle while remaining part of the same visible Turn unless the user submits a
new instruction.

### Terminal

An attempt state from which that attempt will emit no more execution events:
completed, failed, cancelled, or interrupted.

### Continuity failure

An operational condition that prevents the current Engine from keeping the work
moving, such as missing runtime, auth failure, model unavailability, limit,
crash, terminal transport error, context overflow, or confirmed stall.

Continuity failures may trigger automatic fallback.

### Quality concern

A concern about whether a technically successful result is good enough.
Quality concerns do not automatically trigger fallback in V1.

## Switching and Handoff

### Fallback

An automatic transition from a failed EngineAttempt to the next authorized
EngineTarget under the same Turn.

Fallback is not:

- an in-engine retry;
- a user-selected manual switch;
- spawning a Worker;
- choosing a better model after a successful answer.

### Retry

Another attempt on the same EngineTarget after a recoverable engine/transport
condition or explicit user action. Adapter-local retries remain inside the
attempt until the adapter emits a product-visible terminal result.

### Manual switch

A user-requested change of active EngineBranch. During a running attempt it
becomes a pending switch unless the user explicitly chooses Stop and switch.

### Switch back

A manual switch to a parked EngineBranch. Hub first tries native resume and
uses a delta handoff.

### HandoffPackage

Structured product context transferred between EngineBranches. It contains the
goal, instructions, decisions, completed/pending work, workspace evidence,
validation, failure reason, side effects, and optional Team snapshot.

Adapters render the package into their target protocol. The structured package,
not rendered prompt text, is the product contract.

### Full handoff

A HandoffPackage sufficient for a fresh Engine native session to continue the
whole product conversation.

### Delta handoff

A HandoffPackage containing only progress since a parked target branch's
watermark, plus durable global workspace state.

If native resume is invalid, delta handoff must be replaced atomically by full
handoff before fresh execution.

### Watermark

The durable position in product history at which an EngineBranch was parked.
It determines which later events belong in a delta handoff.

### TimelineEvent

An append-only product fact rendered in the Hub timeline. Examples include
fallback, manual switch, handoff, approval, Worker creation, Worker terminal
outcome, and engine health failure.

Timeline events are not raw adapter stream events.

## Fallback Policy and Permissions

### FallbackPolicy

An ordered, session-scoped list of EngineTargets and eligibility requirements.
It is copied from the global default when a HubSession is created.

### Eligible target

An authorized EngineTarget that is healthy and satisfies required capabilities,
permission semantics, and model/provider availability.

### PermissionProfile

The HubSession-owned policy controlling tool approval and sandbox behavior. It
is inherited by Lead and Workers.

### Permission downgrade

Any adapter mapping that is less restrictive than the PermissionProfile.
Permission downgrade is forbidden.

### Autonomous TeamRun

An explicitly selected TeamRun permission profile that permits wider automatic
execution. It is never implied by Worker creation.

## Cindy Orca-style Collaboration

### TeamRun

The durable collaboration aggregate under one HubSession. It contains exactly
one LeadSession and zero or more WorkerSessions.

TeamRun identity survives Lead engine fallback and app restart.

### LeadSession

The execution session responsible for workspace inspection, decomposition,
Worker creation/reuse, dispatch, monitoring, verification, and user-facing
summary.

Lead is a role in a TeamRun, not an Engine. Any eligible Engine may drive it.

### WorkerSession

A durable side session owned by one TeamRun. It has its own EngineBranches,
Turns, queue, history, lifecycle, and fallback policy.

A WorkerSession is not an in-engine native subagent. Native subagents remain
engine-private execution details unless explicitly promoted into a product
WorkerSession.

### Worker

The user-facing shorthand for WorkerSession.

### WorkerTask

One task dispatched by the Lead to a Worker. It records requested scope,
resource claims, queue/acceptance state, terminal outcome, and result evidence.

### Worker queue

Ordered pending WorkerTasks/messages that have not begun execution. Pending
items must be inspectable, editable, and cancellable before consumption.

### Worker outcome

The terminal WorkerTask report sent back through the Worker channel. It includes
result, changed files, validation, remaining risk, and error/fallback evidence.

### Focused Worker

The Worker currently shown in the collaboration details surface. Focus affects
UI only; it does not change execution priority or ownership.

### Hibernated Worker

A WorkerSession whose durable identity/history remain but whose live runtime is
released. New work resumes it.

### Archived Worker

A WorkerSession hidden from the active TeamRun UI while its record/history
remain. Archive is not delete.

### Deleted Worker

A WorkerSession whose product identity is intentionally removed under an
explicit destructive workflow. V1 does not automatically delete durable
Workers.

## Workspace and Resource Control

### Workspace

The local project directory associated with a HubSession. It is the final truth
for file state.

### Shared workspace Worker

A Worker that operates in the HubSession workspace. This is the V1 default.

### Isolated Worker

A Worker operating in a managed worktree. Its results require an explicit
adoption/merge action.

### ResourceClaim

A product-visible declaration that a WorkerTask intends to read or mutate a
path, workspace-wide facility, or globally exclusive tool. Claims determine
whether tasks may run concurrently.

### Tool lock

The runtime guard that enforces compatible ResourceClaims. The existing Alva
`ToolLockRegistry` covers in-product Alva tools; Hub must add product-level
coordination for external engines that cannot share that in-process registry.

### Resource conflict

Two active tasks with incompatible ResourceClaims. Conflicting tasks queue or
use isolated worktrees; they do not run concurrently in a shared workspace.

## Terms That Must Stay Distinct

| Do not conflate | Reason |
|---|---|
| HubSession / EngineSessionId | Product identity must survive engine replacement. |
| Engine / Model / Provider | Harness, inference model, and account route change independently. |
| Fallback / Retry | Fallback changes EngineTarget; retry does not. |
| Fallback / Worker spawn | Fallback is serial continuity; Workers are parallel collaboration. |
| Parked / Hibernated | Parked describes an engine branch; hibernated describes a Worker runtime. |
| Archived / Deleted | Archive preserves identity and history. |
| Worker completed / User work accepted | A terminal Worker outcome still requires Lead synthesis or follow-up. |
| TimelineEvent / RuntimeEvent | Product audit facts and adapter stream events serve different contracts. |
