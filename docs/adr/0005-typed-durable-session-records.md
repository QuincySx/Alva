# ADR-0005: Typed Durable Session Records

- Status: Accepted
- Date: 2026-08-11
- Scope: AgentSession persistence, live observation, telemetry, and extension records

## Context

The current `SessionEvent` is the durable source for conversation messages,
execution skeleton events, approval state, compaction reconstruction, extension
records, and nested-run projection. Its `event_type: String` and untyped JSON
payload make those different meanings difficult to validate and evolve.

At the same time, Alva already has separate process-local `AgentEvent` streams
and analytics events. Replacing `AgentSession` with four independent stores or
traits would lose the single ordered history that message projection,
causality, approval recovery, and Inspector rely on.

The project is still in development, so the durable contract may be corrected
before it becomes a permanent compatibility burden.

## Decision

### One durable authority

- `AgentSession` remains the single durable session boundary.
- Conversation and execution records share one monotonically ordered durable
  sequence.
- Storage backends implement one logical append, query, restore, subscribe,
  flush, and close contract.
- The redesign must not introduce four independently synchronized session
  stores.
- Durable append returns an explicit committed result.
- Atomic batch append is part of the storage contract for state transitions
  that must become visible together.
- Before an external LLM or tool effect, the corresponding intent and stable
  settlement identifiers are durably committed.

### Four architectural channels

- Conversation records are durable and form model-visible history.
- Execution records are durable and explain or recover runs, turns, attempts,
  tools, approvals, compaction, and nested execution.
- Live state and live events are process-local UI observation. Streaming drafts
  and progress updates are not replayed as durable records.
- Telemetry remains a separate, passive diagnostics channel and is not a
  recovery source.

These are lifecycle boundaries, not four variants that must all be persisted
inside one event type.

### Typed extensible envelope

- Replace the stringly typed `SessionEvent` persistence schema with a typed,
  versioned `SessionRecord` envelope.
- The envelope owns stable ordering, identity, causal parent, timestamp,
  emitter, category, type name, schema version, and payload.
- Built-in record payloads use Rust types and typed constructors.
- Third-party plugins use namespaced type names, explicit schema versions, and
  JSON-serializable extension payloads without adding a Kernel enum variant.
- Unknown extension records can be preserved or ignored by readers instead of
  making the whole session unreadable.

### Projection

- `RunRecord` remains an Inspector projection, not a persistence format.
- Message history, pending approvals, Inspector views, statistics, and other
  read models are projections of the durable record stream.
- A UI attaches through an atomic current-state snapshot followed by live
  events; reconnect creates a new snapshot.

### Conversation structure

- Conversation entries form an append-only parent-linked tree.
- A current head selects the branch used for future model context.
- Rollback and branch switching move the head or append from an earlier entry;
  they do not physically delete durable history.
- Compaction is represented by durable entries and projection rules instead of
  destructive event-log rewriting.

### Child runs

- A child agent owns an independent AgentSession and durable record stream.
- Child linkage records `parent_session_id`, `parent_run_id`, and the spawning
  `tool_call_id`.
- Parent logs no longer duplicate every child record inline.
- Inspector follows links recursively and preserves the existing unlimited
  drill-down experience.

### Hub and engine storage ownership

- Follow Cindy's hybrid ownership boundary rather than copying every native
  engine record into the Hub product database.
- Hub SQLite owns product truth and bounded read models: HubSession, Branch,
  Turn, Attempt, product-visible messages, Agent-tab child relationships,
  Inspector catalog entries, and blob reference bookkeeping.
- The Alva adapter owns the full typed AgentSession record repository in a
  dedicated engine-native SQLite store under Hub application data.
- Hub stores the qualified Alva native session ID and capability metadata, then
  retrieves snapshots and live updates through the engine inspection contract.
- Claude and Codex may retain their native history in their own storage. Hub
  stores only truthful product projections and opaque native identifiers unless
  an adapter exposes stronger inspection support.
- Large payload bytes live outside SQLite in content-addressed compressed blob
  storage. Durable records and Hub reference ledgers retain previews, hashes,
  media types, sizes, and blob references.
- The existing Alva adapter message-only `HistoryStore` is removed when the
  AgentSession repository becomes authoritative; Hub must not retain two
  competing Alva message histories.
- Deleting a HubSession cascades Hub-owned rows and schedules engine-native
  record/blob cleanup. A durable cleanup outbox or tombstone completes
  cross-store deletion safely because it cannot be one SQL transaction.

This follows Cindy's separation between product-owned sessions, messages, and
bounded sub-agent projections and engine-native session/transcript ownership,
while adapting the boundary to Alva's typed AgentSession contract. Cindy also
keeps content-addressed media bytes outside its product database. See its
[local database schema](https://github.com/makecindy/cindy/blob/main/apps/desktop/src/main/localDb/schema.ts),
[sub-agent projection](https://github.com/makecindy/cindy/blob/main/apps/desktop/src/main/localDb/subagentRuns.ts),
[session storage adapter](https://github.com/makecindy/cindy/blob/main/apps/desktop/src/main/maker-host/session-storage.ts),
and [message persistence broadcaster](https://github.com/makecindy/cindy/blob/main/apps/desktop/src/main/messagePersistBroadcaster.ts).

### Inspection capability

- `alva-engine-runtime` gains a separate optional `EngineInspector` capability;
  inspection is not added to or overloaded onto the execution-focused
  `EngineRuntime` trait.
- Capability discovery reports `Full`, `Summary`, or `Unsupported` inspection
  support and the truthful features available from that adapter.
- Alva implements full historical snapshots, process-local live updates, raw
  record access, and recursive child-session traversal.
- Claude and Codex implement summary or unsupported modes according to data
  actually exposed by their adapters.

### Sensitive and large payloads

- Local Inspector capture retains complete prompts, responses, tool arguments,
  and tool results when they are available.
- Recognized API keys, authorization headers, authentication tokens, and
  environment secrets are redacted before durable append or blob storage.
- Large fields are not destructively truncated. They move to compressed blob
  storage and leave a bounded preview plus reference in the SessionRecord.
- Telemetry remains secret-free by default and never becomes an alternate copy
  of full Inspector payloads.

### Retention and export

- Inspector history follows HubSession lifetime. Archiving retains it; deleting
  the HubSession initiates cascading product, native-record, and blob cleanup.
- Hub exports a redacted JSONL Bundle containing product identities, ordered
  records, recursive child-session links, and referenced payload manifests.
- Export does not silently include unredacted secrets or unrelated engine-native
  files.

### Context cleanup

- Remove the duplicate Context `SessionAccess` and Context-local `SessionEvent`
  vocabulary.
- Context features that need persistence consume the authoritative AgentSession
  contract through explicit application/plugin wiring.
- No compatibility adapter is added for the unused duplicate vocabulary.

### Compatibility policy

- The new record schema is a clean break during the current development/debug
  stage.
- Existing Alva App SQLite sessions are not migrated.
- Existing Alva CLI JSON sessions are not migrated or opened by the new
  implementation.
- The project does not add dual-write, conversion, or legacy-read paths for
  these retired development formats.

### Scope boundary

- Adopt the durable boundaries and snapshot model described by PI Harness V2.
- Do not adopt PI's entire lane, operation, recovery, and harness API wholesale.
- Alva's Plugin, Bus, Context, Security, and Engine adapter architecture remain
  authoritative.

## Consequences

### Positive

- Inspector gains a stable, versioned source without becoming the storage
  owner.
- Persistent recovery facts cannot be confused with transient UI deltas.
- Built-in schema mistakes become compile-time failures.
- Plugins can add durable records without modifying stable Kernel enums.
- Memory, JSONL, SQLite, and future remote backends can share one conformance
  contract.

### Negative

- The change crosses `alva-kernel-abi`, `alva-kernel-core`, context/security
  consumers, CLI persistence, Alva adapter integration, and Inspector
  projection.
- Existing backends and tests require coordinated migration.
- Precise crash boundaries may require append acknowledgements and atomic
  multi-record writes instead of the current deferred-error contract.
- Existing development session files become intentionally unreadable after the
  cutover.

## Verification

- Conversation and execution records retain one total durable order.
- Streaming token deltas and transient progress do not inflate durable logs.
- A crash can distinguish committed intent, unknown external effect, and
  committed outcome at every adopted durable boundary.
- JSONL and SQLite pass the same record-storage conformance suite.
- Unknown plugin records do not prevent the rest of a session from loading.
- Inspector rebuilds historical state from durable records and overlays live
  state without maintaining a second source of truth.
- Retired App and CLI session formats receive no compatibility code.
- Child-session records are stored once and recursively reachable from their
  parent linkage.
- Hub product tables do not duplicate the complete Alva SessionRecord stream.
- Removing the Alva adapter `HistoryStore` leaves AgentSession as the sole
  authoritative Alva message and execution history.
- Inspector capability discovery never promises unsupported engine detail.
- Durable storage and JSONL Bundle export redact recognized secrets.
- Large payloads remain recoverable through content-addressed blob references.
- HubSession deletion eventually removes linked native records and unreferenced
  blobs even if the first cross-store cleanup attempt is interrupted.
- Context persistence has no second SessionEvent contract.
