# ALVA Hub Desktop V1 Product Specification

> Status: Accepted target; implementation started with ticket 01.
>
> Decision date: 2026-07-31
>
> Related ADRs:
> [ADR-0001](./adr/0001-alva-hub-desktop-first.md),
> [ADR-0002](./adr/0002-reversible-multi-engine-fallback.md), and
> [ADR-0003](./adr/0003-orca-style-collaboration.md).

## 1. Current State and Target State

This document describes the accepted V1 target. It must not be read as a claim
that the target already exists.

The current repository has:

- `alva-hub-core`: an in-memory multi-engine registry and product/native
  session router with resume, cancel, permission routing, engine switching, and
  stream diagnostics.
- `alva-hub-cli`: one-shot and REPL flows, a private JSON session journal,
  explicit Alva/Claude/Codex selection, manual switching, and text handoff.
- `alva-hub-desktop`: a ticket-01 Tauri/React walking skeleton that selects a
  workspace, runs one Alva EngineAttempt through the real `EngineRuntime` seam,
  and projects running/completed/failed product timeline states. It does not
  yet persist HubSession state or implement multi-engine branches, fallback,
  or TeamRun.
- `alva-app-tauri`: the existing Alva-only Tauri desktop product. It directly
  consumes `BaseAgent` and remains subject to Rule 18.

The remaining accepted target expands this separate L7 `alva-hub-desktop`
product. It does not turn `alva-app-tauri` into a multi-engine product and does
not remove it.

## 2. Product Promise

In the next 8–12 weeks, ALVA Hub succeeds when a user can keep one local project
session and:

1. explicitly choose Alva, Claude, or Codex;
2. continue automatically on another authorized engine when the selected engine
   cannot keep the work moving;
3. switch back without losing the parked engine session;
4. understand every switch, permission decision, and handoff;
5. run a Cindy Orca-style Lead with durable cross-engine Workers in parallel.

The product optimizes for continuity of real work. A conservative stop is not
automatically better than a visible, reversible fallback.

## 3. First Users

The first users are individual developers and technical leads who work in local
repositories and already use at least two coding agents.

V1 does not target non-technical consumers, organization-wide administration,
or cloud collaboration. The UI should still hide incidental engine plumbing;
being a developer must not mean manually debugging PATH, native session IDs, or
adapter event formats.

## 4. Product Principles

### 4.1 Explicit primary choice, automatic continuity

The user explicitly chooses the primary engine. Hub may automatically choose an
authorized fallback target when the active engine encounters a V1 continuity
failure.

Hub does not automatically choose the allegedly “best” engine for ordinary
successful work in V1.

### 4.2 Visible and reversible

Fallback is never a silent mutation. It creates a durable timeline event with
the source engine, target engine, reason, handoff, and available controls.

The user can stop, inspect the handoff, or switch back to the parked source
branch.

### 4.3 Product sessions outlive engine sessions

A `HubSession` is the user-owned conversation and workspace context.
Engine-native session IDs are replaceable implementation details held by
`EngineBranch` records.

### 4.4 Workspace is the final truth

Transcript, handoff, and engine-native memory are evidence. The actual
workspace is authoritative whenever they disagree.

### 4.5 Local-first

V1 requires no ALVA account. Sessions, policies, branches, attempts, handoffs,
and audit records stay on the local machine.

### 4.6 Permissions may tighten, never loosen

An engine adapter may enforce a stricter mode than the session asks for. It may
not silently weaken the session permission policy.

### 4.7 Connect engines; do not reimplement them

Hub owns product continuity, orchestration, health, and presentation. Each
adapter preserves its engine's native capabilities and translates the common
contract without inventing another Agent Loop.

## 5. Product Surfaces

### 5.1 ALVA Hub Desktop

`alva-hub-desktop` is the primary V1 product. It uses Tauri 2 with a
React/Vite/TypeScript frontend.

It is a new L7 entrypoint above `alva-hub-core`. It may gradually extract and
reuse presentation-only components, themes, or interaction patterns from
`alva-app-tauri`, but it must not depend on that product's Rust backend or
session database.

### 5.2 ALVA Hub CLI

`alva-hub-cli` remains an auxiliary surface for contract validation,
diagnostics, automation, and headless use. It is not the primary V1 UX.

### 5.3 Existing Alva products

`alva-app-cli` remains the native Alva reference product and high-density
regression harness. `alva-app-tauri` remains an independently maintained
Alva Desktop. Neither consumes `alva-engine-*`.

## 6. Domain Model

```text
HubSession
├── Workspace
├── PermissionProfile
├── FallbackPolicy snapshot
├── TimelineEvent[]
├── MainlineSession
│   ├── EngineBranch[]
│   └── Turn[]
│       └── EngineAttempt[]          # serial fallback attempts
└── TeamRun?                         # Cindy Orca-style collaboration
    ├── LeadSession                  # may fallback without replacing TeamRun
    └── WorkerSession[]              # durable and parallel
        ├── WorkerTask / queue
        ├── EngineBranch[]
        └── Turn[]
            └── EngineAttempt[]      # serial inside one Worker
```

The canonical vocabulary is defined in
[ALVA-HUB-GLOSSARY.md](./ALVA-HUB-GLOSSARY.md).

## 7. Fallback Contract

### 7.1 V1 automatic triggers

V1 automatically falls back for continuity failures that Hub can observe:

- engine missing or incompatible;
- engine process start failure;
- authentication failure;
- selected model unavailable;
- rate, quota, plan, or billing limit;
- context limit;
- process crash;
- terminal transport or stream failure;
- watchdog-confirmed stall or lack of progress;
- another adapter terminal error classified as non-recoverable.

### 7.2 V1 non-triggers

V1 does not automatically fall back merely because:

- an answer appears mediocre;
- Hub guesses another model might be better;
- the model says it is not good at the task;
- the user dislikes an otherwise successful result.

The UI may offer a non-blocking “continue with another engine” action for those
cases.

### 7.3 Attempt bounds

Within one Turn:

- fallback targets are tried in policy order;
- an engine target is attempted at most once automatically;
- the chain never loops back automatically;
- exhausting the chain stops the Turn and presents all failure reasons;
- a user retry creates a new explicit attempt cycle.

### 7.4 Fallback transition

1. Persist the current `EngineAttempt` terminal reason.
2. Capture the current workspace and structured handoff evidence.
3. Select the first authorized, healthy, capability-compatible target.
4. Create or resume the target `EngineBranch`.
5. Start the next attempt under the same Turn.
6. Persist the branch, attempt, active pointer, and timeline event in one
   product transaction.
7. Show a non-blocking system card with Stop, Switch back, and View handoff.

If target startup fails synchronously, the source branch and pending handoff
remain recoverable, and selection proceeds to the next eligible target.

### 7.5 Reversible branches

Fallback does not destroy the source engine-native session. Each engine has a
parked branch. Returning to a branch resumes its native session when possible
and sends only the delta accumulated while it was inactive.

If native resume is invalid, Hub atomically replaces delta handoff with full
handoff before creating a fresh native session.

## 8. Fallback Policy

A `FallbackPolicy` is an ordered list of `EngineTarget` values.

Each target contains:

- engine ID;
- model policy: pinned model or engine-recommended model;
- provider/account source;
- whether automatic use is authorized;
- capability and permission requirements.

The user sets a global default. Creating a HubSession copies it into a
session-level snapshot. Later global edits affect new sessions only; an existing
session can be edited explicitly.

Adding an engine target to the policy is consent to automatic use under the
displayed account and billing source. Hub never auto-selects an installed engine
that is not in the session policy.

## 9. HandoffPackage

The product-level handoff is structured data. Adapters render it into the text
or protocol their target engine accepts.

Required fields:

- user goal and latest instruction;
- accepted decisions and constraints;
- completed work with evidence;
- pending work;
- changed files;
- recent commands and verification results;
- current workspace summary;
- source and target branch identities;
- fallback or manual-switch reason;
- known side effects that must not be repeated;
- TeamRun and Worker snapshot when handing off a Lead or Worker.

The default package excludes:

- hidden reasoning;
- credentials and environment variables;
- approval tokens;
- raw sensitive tool output;
- adapter-private metadata not required by the target.

Full transcript transfer is optional, user-controlled, and still filtered.

## 10. Manual Switching

When the current attempt is running, selecting another engine records a pending
switch. The switch applies at the next safe terminal boundary.

The UI also offers “Stop and switch now.” That action explicitly interrupts the
attempt, records the interruption, captures workspace state, and performs a
handoff. Merely browsing another engine in the picker does not kill work.

## 11. Engine Runtime Management

Hub owns the engine readiness experience:

- automatic discovery;
- supported-version checks;
- capability probing;
- health and stream-contract diagnostics;
- upgrade guidance;
- one-click installation when distribution permits it;
- shortest official installation flow otherwise;
- authentication status and repair entrypoint.

An unhealthy engine is skipped with a visible reason. The engine manager does
not silently change a pinned model, account, permission profile, or binary.

## 12. Permission Contract

`PermissionProfile` belongs to HubSession and is inherited by its TeamRun and
Workers.

- A target that supports the same policy uses it.
- A target may use a stricter policy.
- A target that can only weaken the policy is ineligible.
- TeamRun does not imply `bypass`.
- The user may explicitly enable an autonomous TeamRun profile.
- Equivalent simultaneous Worker requests may be grouped into one approval
  card, but the card must show every Worker and affected scope.

## 13. Cindy Orca-style Collaboration

### 13.1 Lead and Workers

A TeamRun has one LeadSession and multiple durable WorkerSessions.

The Lead:

- reads the workspace;
- decomposes work;
- creates or reuses named Workers;
- dispatches independent tasks in one parallel batch;
- monitors queues and status;
- verifies Worker terminal outcomes through the same execution channel;
- summarizes or requests follow-up.

A Worker:

- is a complete visible session, not a one-shot subagent;
- has its own engine, model, history, queue, and fallback branches;
- can receive follow-ups;
- can send results to the Lead;
- can be focused, interrupted, hibernated, resumed, or archived.

### 13.2 Cross-engine workers

Lead and Worker engine choices are independent. A Codex Lead may create Claude
and Alva Workers; each Worker may have its own ordered fallback policy.

A Lead fallback switches only the LeadSession branch. The TeamRun and Workers
continue. A Worker fallback switches only that WorkerSession branch.

### 13.3 Workspace strategy

The default is a shared HubSession workspace, matching Cindy's low-friction
experience.

ALVA adds two safety mechanisms:

- all in-product Alva Workers share the existing `ToolLockRegistry`;
- mutating tasks declare resource/path scope so conflicting operations queue.

The Lead can request an isolated managed worktree for exploratory, overlapping,
or high-risk work. Isolated worktrees require an explicit result adoption path;
V1 does not silently merge them.

Adapters that execute outside the Alva tool scheduler cannot be assumed to obey
its in-process lock. Hub must represent their declared workspace ownership at
the product layer before starting conflicting work.

### 13.4 Lifecycle

V1 follows Cindy Orca's durable Worker semantics:

- Worker identity/history can exist independently of UI visibility and runtime
  liveness;
- idle Workers may hibernate without losing their session;
- new work resumes a hibernated Worker;
- Team end archives Workers;
- V1 does not automatically delete durable Workers;
- Lead and Worker links rehydrate from SQLite after restart.

### 13.5 Worker completion

Worker terminal status is not allowed to disappear into unstructured chat.
The Lead receives the real terminal outcome, changed files, validation evidence,
and final result through the Worker channel, then reports the result to the
user or sends follow-up work.

## 14. Desktop Information Architecture

V1 uses a restrained two-column layout:

- left: HubSession list, create, and search;
- main: unified timeline and composer;
- composer controls: engine, model, permission profile, and fallback chain;
- timeline cards: fallback, handoff, permission, Worker status, and terminal
  errors;
- optional details drawer: branches, attempts, handoff, engine diagnostics, and
  collaboration Workers.

Workers do not appear as unrelated top-level sessions in the main sidebar. They
live under their Lead's collaboration surface. V1 does not implement Cindy's
general recursive panel tree.

## 15. Persistence

Hub Desktop owns an independent private SQLite database, `alva-hub.db`.
It does not reuse `alva-app-tauri` storage.

Minimum tables:

- `hub_sessions`;
- `engine_branches`;
- `turns`;
- `engine_attempts`;
- `timeline_events`;
- `fallback_policies`;
- `team_runs`;
- `worker_sessions`;
- `worker_tasks`;
- `resource_claims`.

Required invariants:

- product ID and engine-native ID are never conflated;
- one mainline/Worker has at most one active attempt;
- a TeamRun has one Lead;
- branch switch and attempt transition are transactional;
- handoff is durable before target execution;
- timeline events are append-only audit facts;
- restart can rehydrate every active, parked, pending, and hibernated object;
- directory/file privacy, symlink rejection, locking, and atomicity match or
  exceed the current hardened Hub CLI journal.

The CLI JSON journal remains a CLI implementation detail until a separate
migration decision is accepted.

## 16. V1 Non-goals

V1 does not include:

- ALVA accounts or required sign-in;
- cloud sync;
- mobile clients;
- team/organization administration;
- IM and scheduled remote work;
- automatic quality-based engine selection;
- side-chat and general workflow-run trees;
- automatic merge of isolated Worker worktrees;
- replacement or removal of `alva-app-tauri`;
- moving Hub behavior into kernel or `agent-core`.

## 17. Acceptance Scenarios

V1 is not complete until automated tests and desktop runtime evidence cover:

1. A new session runs on each supported engine.
2. A persisted session resumes after app restart.
3. Rate limit, auth failure, missing binary, process crash, context limit, and
   watchdog stall each select the next eligible fallback target.
4. One target is not attempted twice automatically in one Turn.
5. Exhausting the chain stops with all reasons visible.
6. Source branch state survives fallback and can be resumed.
7. Invalid parked resume atomically upgrades delta handoff to full handoff.
8. A fallback card identifies reason/source/target and exposes Stop, Switch
   back, and View handoff.
9. A stricter permission mapping works; a weaker mapping is rejected.
10. Sensitive handoff fields never reach another adapter.
11. A Lead creates at least two Workers on different engines and dispatches
    them in parallel.
12. Conflicting declared resources queue; disjoint resources run concurrently.
13. An isolated Worker uses a distinct worktree and cannot be silently merged.
14. Lead fallback leaves Workers running and reachable.
15. Worker fallback affects no other Worker.
16. App restart restores TeamRun, Worker links, queues, branches, and timeline.
17. A crash between persistence phases does not produce an active attempt with
    missing handoff or branch identity.
18. Native `alva-app-cli` and `alva-app-tauri` still pass Rule 18.

## 18. Delivery Order

1. Domain contracts and SQLite persistence.
2. Multi-branch session projection and deterministic HandoffPackage.
3. Automatic fallback policy and engine health manager.
4. Hub Desktop session/timeline/composer shell.
5. Reversible switch and branch inspection UX.
6. TeamRun, Lead/Worker persistence, queues, and cross-engine dispatch.
7. Shared-workspace resource claims and isolated worktree option.
8. Engine installation/repair UX and acceptance hardening.

Each phase must preserve the current Hub CLI behavior and the SDK/app dependency
rules.
