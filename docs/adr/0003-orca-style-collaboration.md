# ADR-0003: Cindy Orca-style Cross-engine Collaboration

- Status: Accepted
- Date: 2026-07-31
- Scope: Lead/Worker model, parallelism, workspace, permissions, and lifecycle

## Context

Cindy Orca demonstrates a collaboration model in which one durable Lead session
creates multiple durable Worker sessions. Workers have independent harnesses,
models, histories, queues, and lifecycle, and can execute in parallel.

ALVA already has useful lower-level building blocks:

- in-process sub-agent spawning;
- Blackboard communication;
- ToolLockRegistry shared through the Bus;
- worktree tools;
- team roster/message tools.

Those facilities do not yet form a durable, cross-engine Hub product
collaboration model. In particular, current Hub adapters do not produce
Hub-owned Worker sessions, queues, ownership, or restart recovery.

## Decision

### Product model

- V1 adopts Cindy Orca's Lead + durable Worker model.
- A TeamRun belongs to one HubSession.
- A TeamRun has exactly one LeadSession and multiple WorkerSessions.
- Lead and each Worker may select different engines, models, and providers.
- Workers are visible collaboration objects, not engine-private subagents.

### Parallelism

- The Lead may dispatch independent WorkerTasks in one parallel batch.
- Fallback attempts inside one Lead or Worker Turn remain serial.
- Lead fallback does not replace TeamRun or Workers.
- Worker fallback is isolated to that Worker.

### Workspace

- Shared workspace is the V1 default, matching Cindy's low-friction behavior.
- ALVA adds explicit ResourceClaims and product-level conflict coordination.
- Conflicting shared-workspace tasks queue.
- The Lead may choose an isolated managed worktree for high-conflict or
  exploratory tasks.
- Isolated results require explicit adoption; V1 does not silently merge.

### Permissions

- Workers inherit the HubSession PermissionProfile.
- Worker creation never implies bypass.
- The user may explicitly enable an autonomous TeamRun profile.
- Equivalent concurrent approvals may be grouped without hiding Worker or
  resource scope.

### Lifecycle

- Worker identity/history, UI visibility, and live runtime state are distinct.
- Idle Workers may hibernate and later resume.
- Ending a TeamRun archives Workers.
- V1 never automatically deletes durable Workers.
- SQLite is the source of truth for restart rehydration.

### Presentation

- Workers remain under the Lead collaboration surface rather than appearing as
  unrelated top-level HubSessions.
- The user can focus, inspect, message, interrupt, hibernate, resume, and archive
  Workers.
- The Lead receives real Worker terminal outcomes and validation evidence before
  summarizing to the user.

## Consequences

### Positive

- Different engines can specialize and work concurrently.
- Worker context remains inspectable and reusable.
- Lead/Worker continuity survives engine fallback and application restart.
- Existing Alva tool locks, Blackboard, and worktree capabilities have a clear
  product-level consumer.

### Negative

- Cross-engine orchestration requires new durable domain and storage; existing
  in-process team tools are insufficient.
- External engines cannot automatically share Alva's in-process ToolLockRegistry.
- Shared workspace concurrency still depends on accurate resource ownership.
- Worker queues, budgets, permissions, hibernation, and recovery significantly
  expand V1 implementation and test scope.

## Cindy Behaviors Adopted

- Lead and Workers are full sessions.
- Workers can use different harness/model combinations.
- Batch dispatch expresses parallel intent.
- Workers are managed under one collaboration surface.
- Worker runtime can be released without deleting context.
- Lead and Worker links rehydrate after restart.

## Cindy Behaviors Strengthened

- Shared workspace adds resource conflict coordination.
- Worker permissions inherit session policy instead of default bypass.
- Automatic engine fallback is first-class for Lead and Workers.
- Engine branches and fallback attempts are explicit product objects.

## Alternatives Rejected

### Only native/in-process subagents

Rejected because they cannot represent durable cross-engine Workers with
independent sessions and user intervention.

### Run fallback attempts in parallel and select a winner

Rejected because fallback is a continuity mechanism, not speculative
multi-engine evaluation. Parallel attempts can duplicate side effects.

### Require one worktree per Worker

Rejected as the default because it adds merge overhead to every collaboration.
It remains available for tasks that need isolation.

### Copy Cindy's shared worktree without product resource control

Rejected because concurrent external engines can edit overlapping paths and run
global commands without coordination.

## Verification

- At least two Workers on different engines execute disjoint tasks concurrently.
- Conflicting ResourceClaims queue.
- TeamRun and Worker links survive restart.
- Lead fallback preserves all Workers.
- Worker fallback affects no sibling.
- Worker permissions never become less restrictive than the HubSession.
- Hibernation preserves identity/history; archive and delete remain distinct.
