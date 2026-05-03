---
name: autonomous-ui-repair
description: >
  Use this skill when bugs from `autonomous-ui-test` need fixing, when the
  user says "修一下" / "fix the bugs you found" / "go through the bug log",
  or when you have an open `.alva/test-runs/<...>/bugs.jsonl` and the user
  wants progress on it. Reads the bug log, brainstorms 2-3 fix approaches
  per bug, picks the best with reasoning, applies it, then verifies via the
  autonomous-ui-test loop. Sibling to `autonomous-ui-test` — never modify
  source there; modify here.
---

# Autonomous UI Repair — alva-app-tauri

You take the bug log produced by `autonomous-ui-test` and **fix it**.
This is the write side of the loop.

## Operating mode

Two-phase per bug:

1. **Diagnose** — read bug log entry → grep for code → read relevant files
   → understand root cause. **No code changes yet.**
2. **Repair** — brainstorm 2-3 approaches → pick one with reasoning → apply
   → verify the build still compiles → verify the bug is fixed by re-running
   `autonomous-ui-test` on JUST the affected surface.

If you can't diagnose with confidence (root cause unclear), record a
"needs-investigation" note in the bug entry and move on. **Do not guess.**
Half-fixes that don't address root cause introduce new bugs.

## Inputs

- The latest `.alva/test-runs/<YYYY-MM-DD-HHmm>/bugs.jsonl`. List with
  `ls -t .alva/test-runs/` to find it; pick the most recent.
- Bugs with `status: "open"` only. `fixed` and `wontfix` are skipped.
- Process highest severity first: error → warning → info.

## The repair loop

```
[Pick next open bug, highest severity first]
   ↓
[Read entry → reproduce mentally → grep code]
   ↓
[Identify likely files / functions]
   ↓
[Read those files; understand current behavior]
   ↓
[Brainstorm 2-3 fix approaches]
   ↓
[Pick one — explain reasoning IN the bug entry]
   ↓
[Apply patch via Edit tool]
   ↓
[Verify build: cargo check (Rust) or pnpm typecheck (TS)]
   ↓
[Verify behavior: run autonomous-ui-test on JUST this surface]
   ↓
[Mark bug status: fixed / regressed / needs-investigation]
   ↓
[Loop until no open bugs OR three consecutive failures]
```

## Brainstorming format

For every bug, before applying a fix, write to the bug entry's `repair_plan`
field this exact structure:

```json
"repair_plan": {
  "approaches": [
    {"title": "narrow guard at call site", "pro": "minimal blast radius", "con": "doesn't fix root cause; same bug elsewhere"},
    {"title": "validate input at component boundary", "pro": "stops the class of bug", "con": "more code to write/test"},
    {"title": "rework data flow so undefined is impossible", "pro": "eliminates the bug at type level", "con": "larger refactor; touches 3 files"}
  ],
  "chosen": "approach #2 — validate at boundary",
  "rationale": "Approach #1 is whack-a-mole; #3 too disruptive for a P2 bug. #2 is proportional and the boundary already has an obvious place for a guard.",
  "files_to_change": ["src/components/MessageList.tsx"]
}
```

The first time you do this, **always brainstorm at least 2 approaches**.
Single-approach commits skip the "is this really the best fix" check
that's the whole point of this loop.

## Per-language guidance

### TypeScript / React (`crates/alva-app-tauri/web/src/`)

- Most UI bugs live in `components/` or `routes/`. Grep for the bug's
  `surface` field first.
- `agent-bridge.ts` is where IPC to the Rust backend happens; bugs that
  show up as "data missing / undefined" often originate there, not in
  the React component that displayed nothing.
- After each edit, run `pnpm typecheck` from `crates/alva-app-tauri/web/`.
  If it fails, fix the type error before moving on (don't accumulate red
  state).

### Rust backend (`crates/alva-app-tauri/src/`)

- Tauri command handlers live in `agent.rs`, `provider_api.rs`,
  `mcp.rs`, `session_projection.rs`, `sqlite_session/`.
- After each edit: `cargo check -p alva-app-tauri`. If clean, the
  full project should pick it up next dev rebuild.
- Bugs whose root cause is in `alva-app-core` or below are out of scope
  for THIS skill's edits — record as `out_of_scope` and let the user
  decide.

### Cross-cutting

- `console.log`/`tracing::debug!` you add as part of repair are
  **temporary**. Strip them before marking bug `fixed`. If you genuinely
  needed permanent observability, that's a separate decision — record a
  follow-up note instead of leaving stray logs.

## Verification

After applying a fix:

1. Compile check (per language above)
2. Re-launch dev server if not already running (skill `autonomous-ui-test`
   knows how)
3. Drive **specifically** the steps_to_reproduce from the bug entry.
   Don't re-run the full test loop yet — just confirm the one bug is
   actually fixed.
4. Update `status: "fixed"` and append `fix_commit: <sha>` if you've made
   a commit.

If the fix made the bug worse or introduced a regression visible during
this targeted verification: revert (`git restore <files>`) and mark
`status: "regressed"` with notes. Don't try a second approach on the same
bug in the same session — leave it for the user to look at.

## Stop conditions

Stop when ANY of:

- All `open` bugs in the log are now `fixed` or `out_of_scope` or
  `needs-investigation`
- Three consecutive bugs hit `regressed` (something systemic is wrong;
  user should look)
- 60 minutes wall-clock elapsed
- A compile error you can't resolve in 3 attempts (back out and stop)

## Final report

When stopped:

1. Counts: fixed / regressed / out-of-scope / needs-investigation
2. List of fixed bugs by id with one-line summaries
3. List of skipped bugs with reasons
4. `git log --oneline` showing the commits made (if you committed each
   fix; otherwise the unstaged diff `git diff --stat`)
5. Suggest next: re-run `autonomous-ui-test` to look for regressions
   introduced by the fixes, and/or pick up needs-investigation bugs
   manually

## Commit discipline

For each accepted fix, make a small focused commit:

```
fix(<surface>): <one-line>

Closes test-run/<id>/bug-<id>: <title>
```

Don't bundle multiple bug fixes into one commit unless they share a root
cause. Trace-back ergonomics matter more here than convenience.

## Anti-patterns

- **Don't fix the symptom** — if a bug's `observed` says "X undefined",
  trace where X comes from. Defending against undefined at the read site
  is acceptable as a stopgap, but the bug entry should note it's a stopgap
  and create a `followup` note for the real fix.
- **Don't refactor opportunistically** — the user opted into a fix loop,
  not a cleanup loop. If you see other code smells nearby, leave them.
- **Don't add tests blindly** — if the bug's reproduction is "navigate
  through 4 modals to provoke", a unit test won't catch it. The actual
  test is the autonomous-ui-test loop. Only add a unit test if it's
  genuinely the right granularity.
- **Don't mark `fixed` based on compilation alone** — verify behavior in
  the running app. A fix that compiles but doesn't fix the bug is worse
  than no fix because it claims confidence you don't have.
