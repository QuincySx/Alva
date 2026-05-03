---
name: autonomous-ui-test
description: >
  Use this skill when the user asks the agent to "explore", "smoke test",
  "regression test", "stress the UI", "find bugs in the app", or any phrasing
  that implies driving the alva-app-tauri web UI autonomously. Triggers on
  "测一下", "跑一遍 UI", "找找 bug", "verify the UI works",
  or when the user wants the agent to act as its own QA. Drives the existing
  browser_* tools (Chrome via CDP) against the Tauri app's web layer; records
  findings to a structured bug log; never modifies source code (a sibling
  skill `autonomous-ui-repair` handles that).
---

# Autonomous UI Test — alva-app-tauri

You are testing the **alva-app-tauri** web UI as if you were a QA engineer
seeing it for the first time. Your job is to find bugs, not to write code.

## Operating mode

This skill is **read-only**: drive the UI, observe, record. Do **not**
modify source files in this skill. Code changes belong to the
`autonomous-ui-repair` skill, which runs after you produce the bug log.

## Tools you'll use

You already have the `browser_*` tool family from `alva-app-extension-browser`:

- `browser_start` — boot a Chromium instance via CDP
- `browser_navigate` — go to a URL
- `browser_snapshot` — return the **accessibility tree** of the current page
  (semantic, much smaller than full HTML — prefer this over screenshots for
  reasoning about what's on screen)
- `browser_screenshot` — visual proof for "looks broken" judgements
- `browser_action` — click / type / scroll / press key on an element
- `browser_status` — check if browser is alive
- `browser_stop` — clean up at the end

Plus standard `execute_shell` to start the dev server and `read_file` /
`write_file` to manage the bug log.

## The test loop

```
[Setup]
   ↓
[Map] ─→ list every interactive element on current screen
   ↓
[Pick an unexplored target]
   ↓
[Predict] → "If I click X, I expect Y to happen"
   ↓
[Act] → browser_action click/type
   ↓
[Observe] → browser_snapshot + browser_screenshot + check console errors
   ↓
[Compare] → Did Y happen? Console clean? Layout still sane?
   ↓
[Record] ─→ if mismatch: append to bug log
   ↓
[Loop until stop condition]
   ↓
[Report]
```

## Setup (first session only)

1. **Start the Vite dev server** for the Tauri web UI:
   ```sh
   cd crates/alva-app-tauri/web && pnpm install --silent && pnpm dev
   ```
   Run with `execute_shell` and `runInBackground: true`. Wait ~5 seconds,
   then read its stdout — Vite prints the URL (typically
   `http://localhost:5173`).

2. **Start the browser**: `browser_start` to spawn Chrome.

3. **Navigate**: `browser_navigate` to the printed URL.

4. **Initial snapshot**: `browser_snapshot` and `browser_screenshot` to
   confirm the app loads. If you see a blank page or console errors at this
   stage, that's bug #1 — record it before continuing.

If the dev server fails to start (port in use, missing dep, etc.), report
this as a setup failure and stop. Don't try to fix infra in this skill.

## UI surface map

Source: `crates/alva-app-tauri/web/src/`. Use this as a checklist of
surfaces to exercise.

### Top-level layout (`App.tsx`)
- **NavSidebar** (left panel) — session list, "New Chat", session switching
- **Main pane** — current route content
- **Inspector** (right panel, toggleable) — debug/event view

### Routes (`routes/`)
- `Home.tsx` — main chat view (input field + message list + tool calls)
- `Skills.tsx` — skill management
- `Mcp.tsx` — MCP server management
- `Placeholder.tsx` — fallback / unimplemented routes

### Modals & pickers (`components/`)
- `Modal.tsx` — generic modal shell
- `SettingsModal.tsx` — provider config / API key / model picker
- `ModelPicker.tsx` — model selector
- `SkillPicker.tsx` — pick a skill to inject
- `ToolPicker.tsx` — pick which tools are enabled
- `Inspector.tsx` — event timeline viewer
- `ResizableSplit.tsx` — pane divider (drag to resize)

## Test strategies (run in this order)

### 1. Smoke test (every run)

Verify each top-level surface mounts without errors:

- Navigate Home, Skills, Mcp routes one by one. After each: snapshot, check
  console errors, screenshot.
- Open and close every modal (Settings, Model picker, Skill picker, Tool picker).
  Each open → snapshot. Each close → snapshot. State should return to
  previous.
- Toggle Inspector panel on/off. Drag ResizableSplit. Layout should
  re-flow without overflow.

### 2. Functional walks

Pick one user flow per session, end-to-end:

- **Send a message**: open Home, focus chat input, type a short prompt,
  send. Observe: message appears in list; tool calls if any render in
  Inspector; loading state shows then clears.
- **Switch sessions**: New Chat → send msg → click another session in
  sidebar → verify prior msg persists when you click back.
- **Configure a provider**: Settings → API key field → save → close →
  re-open → field should still hold value (if intentional) or be cleared
  (if security-sensitive). Either is valid; **inconsistency is the bug**.
- **Pick a skill / tool / model**: open the picker, select a non-default,
  verify it shows in the active state somewhere visible.

### 3. Stress / random walk

- Click through 20 random buttons rapidly. Watch for:
  - Console errors (especially React warnings: keys, hooks-rule violations,
    state updates on unmounted components)
  - Permanently stuck loading spinners
  - Visual glitches (overlapping elements, off-screen content)
  - Layout that breaks at narrow widths (browser_action can resize)

### 4. Negative cases

Deliberately do "wrong" things:

- Send empty message
- Save Settings with empty API key
- Open Settings while another modal is open
- Click rapidly on async actions (does it fire twice?)
- Type into disabled fields
- Press Esc in various contexts

## What counts as a bug (severity guide)

| Severity | Examples |
|----------|----------|
| **error** | Crash / blank screen / unrecoverable stuck state / infinite spinner / data loss / promise rejection in console |
| **warning** | Layout overflow / wrong element shown briefly / state inconsistency that resolves / `console.warn` from React |
| **info** | Cosmetic (alignment, slightly wrong color, copy typo) / non-blocking inconsistency |

Don't record opinions — record **observations**. "Button is ugly" is not a
bug. "Button has no hover affordance and looks identical to a div" is.

## Bug log format

Append to `.alva/test-runs/<YYYY-MM-DD-HHmm>/bugs.jsonl`. One line per bug:

```json
{
  "id": "uuid-or-counter",
  "discovered_at": "ISO-timestamp",
  "title": "short one-line summary",
  "surface": "Home | Settings | NavSidebar | ...",
  "severity": "error | warning | info",
  "steps_to_reproduce": [
    "navigate to /",
    "click 'New Chat'",
    "type 'hi' in input",
    "click send"
  ],
  "expected": "message appears in list",
  "observed": "input clears but no message renders; console: Cannot read property 'id' of undefined at MessageList.tsx:42",
  "screenshot": "path/to/screenshot.png",
  "console_errors": ["..."],
  "url_at_observation": "...",
  "status": "open"
}
```

If you can't tell whether something's a bug (fuzzy / ambiguous), log as
`severity: info` with a `note` field. Don't pretend confidence you don't
have.

## Stop conditions (apply OR-logic)

Stop the loop when ANY of:

- You've covered every entry in the UI surface map at least once
- You've recorded ≥ 10 distinct bugs (don't pile up findings — pause and
  let the user/repair-skill triage)
- You've hit the same bug surface 3 times (clearly broken there; no need
  for a fourth confirmation)
- 30 minutes wall-clock have elapsed in a single session
- Three consecutive `browser_action` calls return errors that aren't
  trivially "wrong selector" — likely the app crashed; report and stop

## Final report

Once stopped:

1. Print a count of bugs by severity
2. Print top 3 by severity (error > warning > info)
3. State which surfaces you covered and which you skipped (for the next
   session to pick up)
4. `browser_stop` to clean up; the dev server stays running unless the
   user said otherwise

Hand off to `autonomous-ui-repair` (or back to the user) and stop.

## Anti-patterns

- **Don't synthesize tests from imagination** — drive the actual UI. If
  you can't reach a feature, that itself is a finding (discoverability
  problem).
- **Don't fix code** — this skill is read-only. Recording suspicion that
  "this looks wrong" goes in the bug log; actual edits go to the repair
  skill.
- **Don't test what you can't see** — IPC paths into Tauri Rust backend
  aren't accessible to web-only browser tools. If a feature requires the
  real Tauri runtime (file dialogs, native menus), record "skipped:
  requires Tauri runtime" and move on.
- **Don't get stuck in modals** — if you open a modal and can't close it
  (missing X / Esc not bound), that's a bug; reload the page (`browser_navigate`
  to the same URL) and continue.
- **Don't burn tokens on screenshots when you don't need them** — only
  screenshot when you record a bug (visual proof) or when the snapshot
  text is ambiguous.
