---
name: autonomous-ui-test
description: Drive the alva-app-tauri Tauri/web UI autonomously via chrome-devtools MCP — explore, find bugs, brainstorm 2-3 fix approaches per bug, apply the chosen fix, recompile, verify, loop. Use when the user asks to "测一下 / explore the UI / 找 bug / 跑一遍自动化测试" against this project. Specific to crates/alva-app-tauri/web (Vite-served React UI).
---

# Autonomous UI Test — alva-app-tauri

End-to-end loop where you (Claude Code) drive the Tauri web UI of THIS
project, find bugs, brainstorm fixes, apply the best one, verify, and repeat.

## What this skill does NOT do

- It does **not** change product behavior beyond fixing bugs you found.
- It does **not** run when there's no chrome-devtools MCP server (check
  with `list_pages`; if the tool errors, abort and tell the user to
  install/enable chrome-devtools MCP).
- It does **not** touch tests / CI / unrelated files. Stay scoped to bugs
  you actually observed.

## Token discipline — OCR first, image second

Screenshots cost ~1500-3000 vision tokens each; OCR-extracted text costs
~50-200. **Always run OCR before deciding whether the image itself needs
to go to the LLM.** A bundled Swift script (`ocr.swift` next to this
SKILL.md) wraps macOS `VNRecognizeTextRequest` — local, no network, no
API key, sub-second:

```sh
SKILL_DIR="$(dirname "$0")"   # or hardcode to .claude/skills/autonomous-ui-test/
swift "$SKILL_DIR/ocr.swift" path/to/screenshot.png         # plain text, top→bottom
swift "$SKILL_DIR/ocr.swift" path/to/screenshot.png --json  # bbox+confidence per region
```

The text alone preserves layout breaks (text wrapping mid-character at
narrow viewports literally shows up as one char per line in OCR output —
that **is** the bug signal). Only inspect the image directly when:

- Need to verify a **color / icon-only / non-text affordance**
  (e.g. "is the active tab visually highlighted?")
- OCR returns dense noise (low-contrast / small font / handwriting-style)
- A bug entry already opened with OCR text alone is ambiguous about
  spatial relationships

When you do read the image, attach it as `screenshot:` in the bug entry
for the user; **don't** read it back into your own context unless you
have a specific visual question. If you've ALREADY received the image
this session, prefer re-running OCR and reading the text instead of
re-loading the image.

Confidence threshold in `ocr.swift` is 0.3 by default — that catches
small-font sidebar items at the cost of occasional single-character
noise (icons mis-classified as Chinese chars). Tune via the source
constant if your project's UI runs into edge cases.

## Preflight

Before driving anything, confirm:

1. **Pick a browser-driver path**:
   - **Preferred**: `mcp__chrome-devtools__list_pages` — if it returns
     without error, you have full chrome-devtools MCP. Use it.
   - **Fallback**: if MCP isn't wired, drop to **playwright via Node
     scripts** (see "Playwright fallback" below). Tested working on this
     project; needs `~/.cache/ms-playwright/` chromium installed (one-time
     `npx playwright@1.59.1 install chromium`).
   - If neither: stop and tell the user to install one. Don't try to
     fake browser interaction with curl alone — too coarse.
2. **Workspace is clean enough**: `git status --short`. If there's a
   massive uncommitted diff, surface it — your fixes will end up in the
   same diff, hard to untangle.
3. **Pick a results dir**: `mkdir -p .alva/test-runs/$(date +%Y-%m-%d-%H%M)`
   — bug log + screenshots go here.
4. **Check if dev server is already running**: `curl -sI http://localhost:1420 | head -1`.
   If it returns `HTTP/1.1 200 OK`, **do not start a second one** — strictPort
   makes that fail anyway. Reuse the running instance.

## Phase 0 — start the dev server

`crates/alva-app-tauri/` is a **Tauri 2 desktop app**. The web UI lives in
`crates/alva-app-tauri/web/` (Vite + React); the native shell is the Rust
binary `alva-agent-tauri`. Two startup modes — **pick by what you're testing**:

### Mode A: Web-only (default — fast iteration, no Tauri IPC)

```sh
cd crates/alva-app-tauri/web && npm install && npm run dev
```

(Use `npm`, not pnpm/yarn — the project's `tauri.conf.json` calls
`npm --prefix web run dev`, so we match that.)

Then connect Chrome to **`http://localhost:1420`** (NOT 5173 — the Vite
config pins port 1420 with `strictPort: true`; if 1420 is in use, Vite
will refuse to start, you'll need to free it first).

In this mode the **real Tauri runtime is absent**. Calls to
`@tauri-apps/api`'s `invoke()` will throw `Tauri API not available` —
record those as `severity: info, reason: requires-tauri-runtime` and move
on. Everything web-layer (components, state, rendering, routing) is
still fully testable.

### Mode B: Full Tauri (slower, exercises IPC + Rust backend)

```sh
cargo tauri dev   # from repo root, or use --manifest-path
```

This compiles the Rust binary (~30-60s first time) AND runs the Vite
server as a side-effect (`beforeDevCommand`). The Tauri webview opens
automatically; you can attach Chrome DevTools by connecting Chrome to
**`http://localhost:1420`** in parallel — both webviews talk to the same
Vite server, so DOM state stays in sync (with caveats: each loads its
own React tree).

Use Mode B only when:
- The bug specifically requires `invoke()` to work (provider config save,
  session persistence, MCP server lifecycle, native dialogs)
- You're testing native windowing (the standalone Inspector window opens
  via `inspector.html` — separate Tauri window, not just a panel)
- You need to verify Rust-side state (sqlite_session, agent state)

### Two HTML entry points

The Vite config has both `index.html` (main app) and `inspector.html`
(standalone Inspector window). When testing in Mode A, navigate to:
- `http://localhost:1420/` — main UI
- `http://localhost:1420/inspector.html` — Inspector window (separate
  page, often where event-bus / runtime-event bugs surface)

### Sanity-check after start

Run `curl -sI http://localhost:1420 | head -1` — if it doesn't return
`HTTP/1.1 200 OK` within ~10s, the dev server isn't ready. Common causes:
- port 1420 already taken by an old dev session (`lsof -ti:1420 | xargs kill`)
- `npm install` had errors (re-run, capture stderr)
- TypeScript build error (the dev server won't serve until tsc passes)

## Phase 1 — connect + smoke

### With chrome-devtools MCP
```
mcp__chrome-devtools__new_page          → "http://localhost:1420/"
mcp__chrome-devtools__take_snapshot     → see top-level layout
mcp__chrome-devtools__list_console_messages → check for boot errors
mcp__chrome-devtools__take_screenshot   → save to .alva/test-runs/<ts>/00-boot.png
```

### With playwright fallback
Write to `.alva/test-runs/<ts>/probe.mjs` (one-shot — write fresh per
phase):

```js
import { chromium } from 'playwright';
const browser = await chromium.launch({ headless: true });
const ctx = await browser.newContext();
const page = await ctx.newPage();
const errors = [];
page.on('console', m => {
  if (m.type() === 'error' || m.type() === 'warning')
    errors.push({ t: m.type(), msg: m.text(), loc: m.location() });
});
page.on('pageerror', e => errors.push({ t: 'pageerror', msg: e.message, stack: e.stack }));
await page.goto('http://localhost:1420/', { waitUntil: 'networkidle', timeout: 8000 });

// Smoke checks
const title = await page.title();
const buttonCount = await page.locator('button').count();
const html = await page.content();
await page.screenshot({ path: '.alva/test-runs/<ts>/00-boot.png', fullPage: true });

console.log(JSON.stringify({ title, buttonCount, htmlLen: html.length, errors }, null, 2));
await browser.close();
```

Run it from a **scratch dir with playwright installed**:
```sh
mkdir -p /tmp/alva-probe && cd /tmp/alva-probe
[ ! -d node_modules ] && (npm init -y >/dev/null && npm i playwright@1.59.1 --silent)
node /Users/.../alva/test-runs/<ts>/probe.mjs
```

If snapshot/title is `"Alva Agent"` and at least the NavSidebar buttons are
present (>5 buttons), boot is OK. Anything else → record bug #1 first.

### Expected console noise in Mode A (NOT bugs)

The Vite-only mode triggers these **every boot** because `@tauri-apps/api`
can't reach `window.__TAURI__` (no native runtime). Filter these out
before classifying:

- `Cannot read properties of undefined (reading 'invoke')` from
  `agent-bridge.ts`
- `Cannot read properties of undefined (reading 'transformCallback')`
  pageerrors
- `NavSidebar listSessions failed` and similar consumer-side errors
  caused by the above

These are environmental, not regressions. **However**, if the app shows
no graceful fallback UI when these fire (just blank panels), that IS a
bug — `severity: warning` — log it as a finding. The fix is in
`agent-bridge.ts`: detect `__TAURI__` absence and either mock-respond or
render a "requires desktop app" message.

For Mode B (full Tauri), these errors should be absent — if you see them
there, that IS the bug.

## UI surface map (this project)

Source: `crates/alva-app-tauri/web/src/`. Use as your mental checklist.

### Top-level (`App.tsx`)
- `NavSidebar` (left) — session list, "New Chat", session switching
- main pane — current route
- `Inspector` (right, toggleable) — runtime event timeline

### Routes (`routes/`)
- `Home.tsx` — chat: input field + message list + tool-call rendering
- `Skills.tsx` — skill management
- `Mcp.tsx` — MCP server management
- `Placeholder.tsx` — fallback / TBD routes

### Modals & pickers (`components/`)
- `Modal.tsx` — generic shell
- `SettingsModal.tsx` — provider config / API key / model
- `ModelPicker.tsx` — model selector
- `SkillPicker.tsx` — pick skill to inject
- `ToolPicker.tsx` — pick which tools are enabled
- `Inspector.tsx` — event timeline
- `ResizableSplit.tsx` — drag divider

State store: `src/store/appStore.ts` (zustand).
IPC bridge: `src/agent-bridge.ts` — wraps `@tauri-apps/api` invokes; in
Vite-only mode these will throw `Tauri API not available`.

## Phase 2 — systematic exploration

Run in this order. Snapshot + console-check after every action.

### 2a. Smoke (every run, ~5 min)
- Navigate each route (Home / Skills / Mcp). Boot console clean? Layout intact?
- Open + close every modal. State returns to previous?
- Toggle Inspector panel. Drag ResizableSplit. Layout reflows?

### 2b. Functional walks (~15 min)
Pick 2-3 user flows end-to-end:
- **Send a message**: focus input → type → submit. Message renders?
  Loading clears? Tool-call panel updates if tools fired?
- **Switch sessions**: New Chat → type → click another session → verify
  prior msg preserved on switch back.
- **Configure provider**: Settings → put API key → save → reopen → state
  persists or clears intentionally? Inconsistency = bug.
- **Pick model / skill / tool**: open picker → select non-default →
  verify active selection is reflected somewhere visible.

### 2c. Stress / random (~10 min)
- Click ~20 buttons rapidly across the app. Watch:
  - React warnings (key/hook/unmounted-update)
  - Stuck spinners
  - Visual glitches (overlapping, off-screen)
  - Layout at narrow widths (`mcp__chrome-devtools__resize_page` →
    width=600, then 320)

### 2d. Negative cases (~5 min)
- Send empty message
- Save Settings with empty API key
- Open Settings while another modal is open
- Double-click async actions (does it fire twice?)
- Type into disabled fields
- Press Esc in various contexts
- Reload page mid-action; state recovery sane?

## Bug log format

Append one JSON line per bug to
`.alva/test-runs/<timestamp>/bugs.jsonl`. Each entry:

```json
{
  "id": 1,
  "discovered_at": "ISO-timestamp",
  "title": "short one-line",
  "surface": "Home | NavSidebar | Settings | ...",
  "severity": "error | warning | info",
  "steps_to_reproduce": ["navigate /", "click 'New Chat'", "type 'hi'", "click send"],
  "expected": "message appears in list",
  "observed": "input clears but no message renders; console: 'Cannot read property id of undefined' at MessageList.tsx:42",
  "screenshot": ".alva/test-runs/<ts>/bug-001.png",
  "console_errors": ["Uncaught TypeError: ..."],
  "url_at_observation": "http://localhost:5173/",
  "status": "open"
}
```

Severity guide:
- **error**: crash / blank / unrecoverable / data loss / promise rejection
- **warning**: layout overflow / wrong element shown briefly /
  state-inconsistency that resolves / React warning
- **info**: cosmetic / non-blocking / `requires-tauri-runtime` / etc.

Don't record opinions. Record observations. "Button is ugly" = no.
"Button has no hover affordance and looks identical to a div" = yes.

## Phase 3 — repair (after exploration)

Switch from observe-mode to fix-mode. For each open bug, **highest
severity first**:

### 3a. Diagnose (no edits yet)
1. `Read` the bug entry.
2. Grep / read the suspected file (use `surface` field to narrow).
3. State the root cause in plain prose. If unsure, mark
   `status: "needs-investigation"` and skip — don't guess.

### 3b. Brainstorm — at least 2 approaches
Append to the bug entry's `repair_plan` field:

```json
"repair_plan": {
  "approaches": [
    {"title": "guard at call site", "pro": "minimal diff", "con": "doesn't fix root; same bug elsewhere"},
    {"title": "validate at component boundary", "pro": "stops the class", "con": "more code"},
    {"title": "rework data flow so undefined impossible", "pro": "fixes at type level", "con": "3-file refactor"}
  ],
  "chosen": "approach #2",
  "rationale": "#1 is whack-a-mole, #3 disproportionate for a P2. #2 is right granularity.",
  "files": ["crates/alva-app-tauri/web/src/components/MessageList.tsx"]
}
```

Single-approach commits skip the "is this really the best fix" check —
that's the whole point of this loop.

### 3c. Apply + verify
1. `Edit` the file(s). Keep the diff minimal — just the chosen fix.
2. **TypeScript bug**: `cd crates/alva-app-tauri/web && pnpm typecheck`
   (or `tsc -b`). Compile clean before continuing.
3. **Rust bug** (if root cause is in `crates/alva-app-tauri/src/`):
   `cargo check -p alva-app-tauri`. Tauri dev rebuild picks up next
   reload.
4. Vite HMR most likely already applied the change — go back to the
   browser, replay the `steps_to_reproduce`, snapshot, screenshot.
5. If fixed: update the entry — `status: "fixed"`, `fix_diff`: `git diff
   --stat <files>`. Commit with `fix(<surface>): <one-line>` message,
   referencing the bug id.
6. If made worse: `git restore <files>`, mark `status: "regressed"`,
   move on. **Don't try a second approach in the same session** — leave
   for human review.

## Stop conditions (OR-logic)

Stop and report when ANY of:
- All `open` bugs are now `fixed | regressed | needs-investigation | out-of-scope`
- Three consecutive bugs hit `regressed` (something systemic; user should
  intervene)
- 60 wall-clock minutes elapsed
- A compile error you can't resolve in 3 attempts
- chrome-devtools MCP returns errors on 3 consecutive calls (browser likely
  crashed; report and stop)

## Final report

When stopped, print:
1. Bug counts by severity + by status
2. Top 3 `error` (or `warning` if none) bugs with one-liners
3. Surfaces covered + surfaces skipped
4. Commits made: `git log --oneline <since>` (or unstaged `git diff --stat`
   if you didn't commit)
5. Suggest next: "re-run this skill to look for regressions introduced by
   the fixes" / "human review needed for: <list of regressed/needs-investigation>"

Then `mcp__chrome-devtools__close_page` to clean up. Leave Vite running
unless user said to stop it.

## Anti-patterns

- **Don't synthesize tests** — drive the actual UI. If a feature is
  unreachable, that's discoverability, log it.
- **Don't refactor opportunistically** — user opted into a fix loop, not
  a cleanup. Adjacent code smells stay.
- **Don't fix symptoms only** — when fixing X, trace where bad data came
  from. A guard at the read site is a stopgap; note follow-up.
- **Don't claim fixed without behavioral verification** — compile-clean
  ≠ behavior-correct. Always re-drive the reproduction steps.
- **Don't get stuck in modals** — if a modal can't be closed, that's a
  bug; reload page and continue.
- **Don't burn tokens on screenshots gratuitously** — only screenshot
  when recording a bug or when snapshot text is genuinely ambiguous.
- **Don't add `console.log` debug statements** — if you need observability
  the app doesn't provide, that's a separate "improve logging" task; log
  it as a finding instead of inserting stray logs.

## When to ask the user instead of proceeding

- Setup fails (missing dep, port conflict you can't resolve)
- 3 consecutive `regressed` fixes
- A bug requires deciding between two valid behaviors (e.g. "should
  Settings clear on close?") — that's product judgement, not yours
- You'd need to modify CI / build config / tooling — out of scope
