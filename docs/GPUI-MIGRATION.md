# ALVA Hub GPUI Migration

> Status: accepted product and migration boundary as of 2026-08-11.

## Goal

`alva-hub-gpui` becomes the only desktop product. The migration is not a
page-for-page rewrite of either retired Alva App or the old Hub dashboard. It
creates one macOS-native, chat-first Hub while preserving the runtime abilities
that still matter.

The first replacement milestone is deliberately narrow: a usable Hub chat plus
the Alva Inspector capability that made the standalone Alva App valuable.

## Product Boundary

- Keep `alva-hub-gpui` as the only desktop product target.
- Retire `alva-app-gpui` and `alva-app-tauri` after the first replacement gate.
- Retire `alva-hub-desktop` after the separate Hub capability gate.
- Keep `alva-app-cli`, `alva-app-core`, the Alva engine, and Inspector data
  production.
- Do not maintain two GPUI applications or a second single-engine desktop UI.
- V1 release scope is native macOS. Linux, Windows, and GPUI/WASM Web are future
  projects and do not block this migration.

## Target Window

The native window follows the interaction shape of the current Agent products:

```text
┌──────────────────┬─────────────────────────────┬──────────────────────┐
│ Projects         │ Active HubSession chat      │ Tool Workspace       │
│ Sessions         │ messages + live execution   │ Inspector            │
│                  │ composer                    │ Agent / Git / Terminal│
└──────────────────┴─────────────────────────────┴──────────────────────┘
```

- The left side owns project and session navigation.
- The center is the continuous chat and composer.
- The right side is a resizable, collapsible `DockArea` workspace.
- Inspector, Agent, Git, and Terminal are separate tool tabs.
- Temporary details use `Popover`, `Sheet`, or `Dialog` instead of becoming
  permanent dashboard columns.
- The first version does not require detachable macOS child windows.

Chat message typography, bubble treatment, code blocks, tool-call presentation,
composer proportions, and motion remain a later product-design pass. They are
not implied by adopting GPUI or gpui-component.

## Current Reality

The latest source was rebuilt and inspected in a real GPUI window on
2026-08-11.

Already present:

- native GPUI window and `Root` initialization;
- gpui-component `Sidebar`, `TabBar`, `Tag`, `Sheet`, inputs, buttons, loading,
  and status controls;
- Hub SQLite store, session recovery, engine execution, cancellation,
  permission handling, fallback, readiness, TeamRun, and worktree behavior;
- a release `.app` assembly path.

Not yet the target product:

- the shell is still a fixed sidebar plus dashboard-like main column;
- there is no `DockArea`/`PanelView` tool workspace;
- persisted and live events are not projected as a continuous chat transcript;
- the composer is still a thin command form rather than the primary interaction
  surface;
- Inspector is not available in Hub;
- Agent and Inspector are not separate right-side tools;
- current documentation still contains obsolete claims about a second
  `alva-app-gpui` migration and Tauri 1:1 visual parity.

The old Tauri directories mostly contain tracked generated build artifacts in
the current checkout. Their meaningful source baselines remain available from
Git history, so generated files are not a product capability or parity source.

## Migration Stages

### G0 — Freeze the Product Boundary

- Make this document and ADR-0004/0005 authoritative.
- Mark the old dual-GPUI and page-parity task set as superseded.
- Keep old desktop directories read-only until their corresponding retirement
  gate passes.

### G1 — Build the Native Hub Shell

- Replace the fixed two-column shell with gpui-component `DockArea` and
  `PanelView` foundations.
- Add left project/session navigation, central chat panel, and collapsible right
  Tool Workspace.
- Preserve `Root` overlay layers, native keyboard focus, minimum-size behavior,
  and theme-only colors.
- Centralize GPUI and gpui-component dependency declarations and retain exact
  lockfile revisions during each migration batch.

### G2 — Make Chat the Runtime Surface

- Project HubSession, Turn, EngineAttempt, runtime events, approvals, handoffs,
  and final results into one ordered chat model.
- Support streaming, resume, stop, approval, retry, and engine selection from
  the chat surface.
- Keep fallback, readiness, and TeamRun reachable through their existing
  gpui-component sheets while their final chat presentation is deferred.

### G3 — Add the Hub Inspector Contract

- Implement ADR-0005 typed durable `SessionRecord` storage.
- Add the separate optional `EngineInspector` capability.
- Give Alva full historical snapshots, live updates, raw records, and recursive
  child-session traversal.
- Give Claude and Codex only the truthful summary or unsupported state their
  adapters can provide.
- Add the global experimental visibility switch, off by default.

### G4 — Build the Inspector Tool

- Add a dedicated Inspector tab in the right Tool Workspace.
- Preserve HubSession, Turn, and EngineAttempt identity.
- Support live selection, historical reload, nested child navigation, large
  payload previews, redaction indicators, and JSONL Bundle export.
- Keep Agent as a separate tab for product child-agent relationships.

### G5 — Retire the Standalone Alva App

Delete `alva-app-gpui` and `alva-app-tauri` only after Gate A passes. Remove
their packaging scripts, workspace entries, obsolete CI rules, and generated
artifacts in the same cleanup.

### G6 — Preserve the Remaining Hub Capabilities

- Keep current fallback, readiness, provider, TeamRun, worker, handoff,
  permission, recovery, and engine-switch behavior working in the GPUI product.
- Refine those experiences incrementally after Chat + Inspector are usable.
- Consolidate duplicate Hub GPUI packaging scripts and smoke-test the packaged
  app.

### G7 — Retire the Tauri Hub

Delete `alva-hub-desktop` only after Gate B passes. This does not require
page-for-page visual parity, code signing, notarization, Linux, Windows, or Web.

## Retirement Gates

### Gate A — Alva App Replacement

- left project/session navigation works;
- central Hub chat can run, stream, resume, stop, and answer approvals;
- right Tool Workspace can open, resize, switch tabs, and collapse;
- Alva Inspector supports real-time and historical records;
- recursively linked child runs are navigable;
- Inspector visibility is controlled by the global experiment switch;
- packaged macOS development app passes a cold-start smoke test;
- the user confirms deletion.

After Gate A, delete `alva-app-gpui` and `alva-app-tauri`. Keep
`alva-app-cli` and `alva-app-core`.

### Gate B — Hub Desktop Replacement

- Alva, Claude, and Codex can run through the GPUI Hub at their truthful
  capability levels;
- Hub session persistence and crash recovery pass smoke coverage;
- cancellation, permissions, switching, fallback, and existing TeamRun access
  remain usable;
- no Cargo, CI, documentation, or packaging path requires
  `alva-hub-desktop`;
- the user confirms deletion.

After Gate B, delete `alva-hub-desktop` and its tracked generated artifacts.

## Acceptance Philosophy

- Preserve capabilities, not the old dashboard's pixels or page hierarchy.
- Use Cindy as a product-ownership and chat-first interaction reference, not as
  a source copy.
- Use gpui-component for standard controls and panel infrastructure.
- Keep chat rows, execution records, and Inspector visualization as Alva product
  components where no standard component expresses the domain.
- A feature may remain in its current Sheet during migration; it must not block
  the Chat + Inspector milestone merely because its final visual treatment is
  undecided.

## Non-Goals

- maintaining `alva-app-gpui` as a second desktop product;
- recreating the old Tauri screen one-to-one;
- requiring every Hub feature redesign before Inspector ships;
- promising equivalent Inspector depth for Claude and Codex;
- making signing or notarization a development migration gate;
- committing to GPUI/WASM Web as part of V1.
