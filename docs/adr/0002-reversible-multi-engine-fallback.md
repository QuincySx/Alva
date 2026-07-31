# ADR-0002: Reversible Multi-engine Fallback

- Status: Accepted
- Date: 2026-07-31
- Scope: Session, attempt, policy, handoff, and permission semantics

## Context

The current Hub maps one product session to one current engine-native session
and records a previous engine during handoff. V1 needs automatic continuity
when an engine cannot proceed, while preserving user control and avoiding
infinite retry loops.

The user experience goal is closer to a system connection handoff than a modal
failure wizard: continue automatically, show what changed, and let the user
switch back.

## Decision

### Selection

- The user explicitly selects the primary engine.
- Each HubSession stores an ordered, user-authorized FallbackPolicy snapshot.
- Hub automatically selects the next eligible target for V1 continuity
  failures.
- V1 does not automatically route successful work based on predicted quality.

### Identity

- HubSession remains the product identity.
- One execution session retains multiple EngineBranches.
- Fallback changes the active branch but does not destroy the source branch.
- Returning to an engine resumes its parked native session when possible.

### Attempts

- One Turn may contain multiple serial EngineAttempts.
- An EngineTarget is tried at most once automatically per attempt cycle.
- The fallback chain never loops automatically.
- Exhaustion stops and shows every failure.

### Handoff

- HandoffPackage is a structured product contract.
- It contains goals, decisions, workspace state, changed files, validation,
  pending work, fallback reason, and side effects.
- Full handoff serves a fresh native session.
- Delta handoff serves a resumed parked branch.
- Invalid resume atomically upgrades delta handoff to full handoff.
- Workspace state wins over transcript and native engine memory.

### Interaction

- Fallback continues without a blocking confirmation.
- A durable timeline card shows source, target, reason, and controls.
- The user can Stop, Switch back, or View handoff.
- Manual switching during an active attempt is deferred unless the user chooses
  Stop and switch.

### Permissions and billing

- PermissionProfile belongs to HubSession.
- A target may be stricter but never less restrictive.
- Adding an EngineTarget to FallbackPolicy authorizes its displayed account and
  billing route for automatic use.
- Hub never selects an installed but unauthorized engine.

## Consequences

### Positive

- Operational engine failures do not force the user to restate work.
- Users retain control without modal interruptions.
- Native engine context remains reusable.
- Attempt and branch history makes fallback diagnosable and testable.

### Negative

- Multi-branch persistence is more complex than the current single-route map.
- Handoff filtering and side-effect evidence become security-critical.
- Usage and cost may span providers during one visible Turn.
- Some engines will be skipped because they cannot honor the permission
  contract.

## Alternatives Rejected

### Manual fallback only

Rejected because continuity, not conservative stopping, is the primary product
promise.

### Fully automatic “best engine” routing

Rejected for V1. It adds quality evaluation, cost optimization, and opaque
decision-making before operational continuity is proven.

### Replace the source native session

Rejected because switching back would lose engine-native context and make
handoff irreversible.

### Retry engines in a cycle until one succeeds

Rejected because it can create retry storms, duplicate side effects, and
unbounded cost.

### Transfer the complete raw transcript by default

Rejected because it wastes context and may expose hidden reasoning, credentials,
sensitive tool output, or adapter-private data.

## Verification

The acceptance matrix in
[ALVA-HUB-V1.md](../ALVA-HUB-V1.md#17-acceptance-scenarios) is normative.
At minimum, each trigger, chain bound, branch return, resume failure, permission
mapping, sensitive-field filter, and crash boundary needs direct test evidence.
