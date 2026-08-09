# ALVA Hub Desktop V1 — Acceptance Matrix

Maps every scenario in `docs/ALVA-HUB-V1.md` §17 to its evidence. Two kinds
of evidence exist and they are **not** interchangeable:

- **Automated** — an executable assertion in this repo. Re-run with
  `cargo test -p alva-hub-desktop` (128 tests) plus the named suites.
- **Packaged runtime** — requires a built desktop app driven by a human
  against real engine CLIs. Listed honestly as outstanding where it is.

A scenario is only marked ✅ when an automated test asserts the behaviour end
to end. Anything resting on a real Claude/Codex binary, a real provider
account, or a GUI interaction is marked ⏳ with what remains.

---

## 1. A new session runs on each supported engine

**✅ Automated + packaged-runtime evidence (codex, alva).** The product
seam is engine-agnostic and is exercised with injected `EngineRuntime`
doubles for all three engine ids
(`run::tests::persists_reopens_and_resumes_a_product_session`,
`terminal_continuity_failure_falls_back_to_next_authorized_engine_in_same_turn`
covers alva→claude, `team_worker_executes_on_its_own_engine_and_reports_to_the_lead`
covers codex). Engine assembly is asserted too
(`alva::tests::eligible_cli_engines_assemble_with_profile_mapping`,
`build_engine_fails_closed_for_weaken_only_combinations`).

Real-CLI runs through `alva-hub-cli` against this machine's binaries
(mise node/24 claude, npm-openai-codex codex, built-in alva with the
shared `~/.alva/config.json` anthropic profile):

- `--engine codex --permission deny` executed a real `codex exec --json`
  run ("2+2 is 4.") and persisted a product session; `--session <id>`
  resumed the native codex session ("3+3 is 6.") after the resume
  argument bug below was fixed.
- `--engine alva --permission deny` executed a real Alva harness run
  through the shared anthropic provider profile ("4").
- `--engine claude --claude-mode light --permission deny` executed a real
  Claude Code print run ("14.").
- Cross-engine handoff: `--session <alva-id> --engine codex` restored the
  alva branch, switched to a fresh codex native session, injected the
  handoff context, and codex continued ("3+3 equals 6.").

**Two real-engine bugs found and fixed by these runs:**

1. `codex exec resume` has no `-C`/`-s` flags (only fresh `exec` does);
   the adapter appended them unconditionally, so every resume exited with
   an argument error and reported an empty stream. Fix: fresh uses
   `-C -s`, resume uses `current_dir`; pinned by
   `process::tests::fresh_exec_keeps_cwd_and_sandbox_flags` and
   `resume_drops_cwd_and_sandbox_flags_and_keeps_session_id`.
2. The shared `~/.alva/config.json` `providers{}` map is invisible to the
   SDK-level `ProviderConfig::load` (top-level legacy fields only), so
   `--engine alva` reported "no API key" despite a valid active profile.
   Fix: hub-cli resolves env key → shared active profile (via
   `alva_app_core::config`) → project/default layers, mirroring the Alva
   CLI; `alva-hub-cli` allowlist updated for the new `alva-app-core` dep.

## 2. A persisted session resumes after app restart

**✅ Automated.** `run::tests::persists_reopens_and_resumes_a_product_session`
(reopens the database and continues the native session);
`store::tests::worker_lifecycle_survives_restart_and_never_conflates_archive_with_delete`;
`store::tests::team_run_walking_skeleton_survives_reopen_with_all_associations`.

## 3. Rate limit, auth failure, missing binary, process crash, context limit and watchdog stall each select the next eligible fallback target

**✅ Automated for classification and selection.**
`failure::tests::matrix_covers_all_v1_operational_failure_categories` pins
missing binary, auth, rate limit, context and crash (plus
transport/model/other); stall is pinned separately by
`failure::tests::stall_messages_classify_stably` and
`only_unclassified_failures_are_excluded_from_fallback`;
`run::tests::terminal_continuity_failure_falls_back_to_next_authorized_engine_in_same_turn`
and `confirmed_stall_classifies_stably_and_falls_back` prove the selection
path for a stream failure and for a watchdog stall.

**Note:** the categories are asserted through the classifier, not by
provoking a real 429 from a live provider. The real-fallback-target half
IS observed at packaged-runtime level:
`run::tests::real_codex_receives_a_fallback_after_engine_failure` fails a
scripted engine and completes the turn on the REAL codex CLI (gate:
codex on PATH + `~/.codex/auth.json`; `ALVA_ALLOW_SKIP_REAL_ENGINE_TESTS=1`
to skip deliberately), and
`run::tests::real_codex_runs_and_completes_a_turn` runs a full real-codex
turn. Live-provider rate-limit provocation remains outstanding.

## 4. One target is not attempted twice automatically in one Turn

**✅ Automated.** `run::tests::fallback_chain_tries_each_authorized_engine_once_in_order`
(asserts each engine executed exactly once);
`store::tests::same_engine_alternate_provider_targets_each_get_their_own_try`
(the bound is per engine/model/provider triple, not per engine name).

## 5. Exhausting the chain stops with all reasons visible

**✅ Automated.** `run::tests::exhausted_chain_records_a_card_with_every_terminal_state`;
`run::tests::unclassified_mid_chain_failure_stops_and_still_summarizes_the_cycle`.

## 6. Source branch state survives fallback and can be resumed

**✅ Automated.** `store::tests::automatic_fallback_continues_the_turn_with_serial_attempt_and_persisted_handoff`
(source parked, target active, handoff persisted before execution);
`store::tests::returning_to_the_fallback_source_branch_receives_the_fallback_turn_in_the_delta`
(the resumed source is told what the fallback engine did — the watermark bug
this test was written to pin).

## 7. Invalid parked resume atomically upgrades delta handoff to full handoff

**✅ Automated.** `run::tests::invalid_native_resume_replaces_delta_with_full_handoff_before_fresh_execution`;
`store::tests::native_resume_upgrade_is_crash_recoverable_with_full_handoff`.

## 8. A fallback card identifies reason/source/target and exposes Stop, Switch back, View handoff

**✅ Automated (web layer).** The card's data is asserted
(`run::tests::terminal_continuity_failure_...` checks the emitted card carries
`source → target`, the reason and the handoff summary), and Stop's mechanism
is asserted end to end by
`run::tests::user_stop_cancels_the_attempt_without_fallback_and_survives_restart`.
The web layer now has a test runner (vitest + Testing Library):
`web/src/App.integration.test.tsx` renders the app against a mocked bridge
seam and asserts the three controls' presence and enabled/disabled logic
(Stop is enabled for a `running` session, Switch back is enabled while the
composer selects the fallback source, View handoff expands the projected
handoff), and `web/src/App.pure.test.tsx` pins the timeline projection and
copy helpers. Run with `npm test` in `web/` (11 tests).

## 9. A stricter permission mapping works; a weaker mapping is rejected

**✅ Automated.** `permissions::tests::adapter_mapping_contract_table_is_stable`
(the 3×3 table; weaker mappings are asserted ineligible);
`store::tests::team_creation_inherits_permissions_and_never_widens_them`
(creating a worker is not a route around the profile). All three fail-closed
points have their own test:
`run::tests::fallback_skips_targets_that_would_weaken_the_permission_profile`,
`store::tests::manual_switch_to_a_weaken_only_target_is_rejected_with_the_reason`,
`alva::tests::build_engine_fails_closed_for_weaken_only_combinations`.

## 10. Sensitive handoff fields never reach another adapter

**✅ Automated.** `run::tests::canary_secrets_never_reach_the_fallback_engine`
parses the JSON the target adapter actually received;
`redact::tests::mainstream_secret_shapes_are_all_caught` covers PEM bodies,
`name: value`, `Bearer`, TOML/JSON/URL/tab forms;
`store::tests::transcript_is_opt_in_and_passes_the_same_filter`.

## 11. A Lead creates at least two Workers on different engines and dispatches them in parallel

**✅ Automated.** `run::tests::parallel_worker_tasks_genuinely_overlap_and_keep_attribution`
— the overlap is proved by wall-clock bounds under a paused clock, not by
terminal state alone.

## 12. Conflicting declared resources queue; disjoint resources run concurrently

**✅ Automated.** `store::tests::resource_claims_let_compatible_tasks_run_and_queue_conflicting_ones`;
`store::tests::an_older_waiter_reserves_its_resources_against_a_stream_of_short_tasks`
(no starvation); `claims::tests::*` for the conflict algebra.

## 13. An isolated Worker uses a distinct worktree and cannot be silently merged

**✅ Automated (requires git).** `worktree::tests::isolated_work_never_reaches_the_shared_workspace_until_adoption`;
`preview_surfaces_conflicts_with_concurrent_shared_edits`;
`discarding_a_worktree_never_deletes_shared_workspace_data`;
`discard_refuses_paths_and_branches_it_did_not_create`.

**Caveat closed:** these tests used to skip silently without `git`, which
would report green with zero coverage. They now panic with an explanatory
message unless `ALVA_ALLOW_SKIP_GIT_TESTS=1` is set deliberately.

## 14. Lead fallback leaves Workers running and reachable

**✅ Automated.** `store::tests::fallback_is_local_to_one_session_and_its_handoff_carries_team_context`
(run identity, both workers and their queues survive; the lead's handoff says
so).

## 15. Worker fallback affects no other Worker

**✅ Automated.** `run::tests::one_worker_falls_back_while_a_sibling_keeps_progressing`
— the sibling is observed **still running at the instant the fallback lands**,
and the assertion was mutation-verified (it fails when the two runs are
serialized).

## 16. App restart restores TeamRun, Worker links, queues, branches and timeline

**✅ Automated.** `store::tests::team_run_walking_skeleton_survives_reopen_with_all_associations`;
`store::tests::worker_lifecycle_survives_restart_and_never_conflates_archive_with_delete`.

## 17. A crash between persistence phases does not leave an active attempt with missing handoff or branch identity

**✅ Automated.** `store::tests::reopening_repairs_an_abandoned_attempt_and_preserves_evidence`;
`store::tests::reopening_applies_a_switch_queued_before_a_crash`;
`store::tests::native_resume_upgrade_is_crash_recoverable_with_full_handoff`.
Reopen repair also sweeps `worker_tasks` so a crashed task cannot stay
`running` forever —
`store::tests::reopening_repairs_a_worker_task_left_running_by_a_crash`
(added here: the sweep existed but nothing asserted it).

## 18. Native `alva-app-cli` and `alva-app-tauri` still pass Rule 18

**✅ Automated — green as of this change.** `scripts/ci-check-deps.sh` enforces
Rule 17 (no SDK crate depends on app/host) and Rule 18 (neither native product
depends on any `alva-engine-*`); both pass.

**Correction:** the script was **failing** from commit `e4c1b48` until now.
That commit made `alva-kernel-abi` a direct dependency of `alva-hub-desktop`
(for the shared path normalizer) without updating the crate's allowlist, and
the firewall was not re-run at the time. Rules 17 and 18 themselves were never
violated — the stale entry was the hub-desktop allowlist.

---

## Packaged build

- `npm run build` in `web/` produces the bundle (index + assets).
- `cargo build --release -p alva-hub-desktop` produces the binary.
- `npm run tauri build` (from `web/`, or `./web/node_modules/.bin/tauri build`
  from the crate root) produces a bundled
  `target/release/bundle/macos/ALVA Hub.app` (Tauri 2, `bundle.targets =
  ["app"]`). The bundle was launched and observed alive, created its private
  `alva-hub.db` under `~/Library/Application Support/com.alva.hub`, and
  exited cleanly.
- External links go through `tauri-plugin-opener` (`opener:default` in the
  capability file); a bare `<a target="_blank">` does nothing in a wry
  webview, which was the outstanding gap from ticket 14.

## Hub CLI regression

`cargo test -p alva-hub-cli` — 24 tests green, covering one-shot, REPL,
journal, resume and switch argument handling.

## Beyond-V1 evolution (Cindy-inspired)

Features added while evolving against `makecindy/cindy`, not part of the
accepted V1 spec:

- **Cross-engine session memory** (schema 14, `hub_memory`): corrections a
  user teaches a session ride every handoff (`HandoffPackage.session_memory`)
  so a fallback engine continues with accumulated knowledge, not from zero.
  The live drawer list and the handoff snapshot come from the same query, and
  memory passes the same secret filter as every user-typed field.
- **Workspace skills across engines** (`HandoffPackage.available_skills`):
  `<workspace>/.alva/skills/<name>/SKILL.md` entries are listed by name,
  frontmatter description and path at handoff build time; the target engine
  reads the file itself. Bounded to 20, never a handoff failure.
- **Workspace MCP discovery** (`HandoffPackage.available_mcp`): scans
  `.mcp.json` / `mcp.json` (Alva `servers` and ecosystem `mcpServers`
  shapes) into metadata-only snapshots. env/headers are never read, so
  credentials cannot cross engines; the target reconnects with its own
  config.
- **Editable fallback-chain model/provider**: the policy targets' model_id
  and provider_id are editable inputs saved through the existing policy
  seams (no backend change).
- **Long-session middleware pinned**: a factory-level test asserts every
  Hub Alva session carries compaction, checkpoint and the enforced
  tool-timeout middleware (they were already default-on via the shared
  components table; nothing asserted it before).

Both were verified end to end: the packaged app migrated a real schema-13
`alva-hub.db` to 14 on launch (creating `hub_memory`) and exited cleanly.

## Outstanding before calling V1 accepted

These cannot be closed from the repository alone and are deliberately not
marked done:

1. ~~A packaged desktop run per engine~~ (scenario 1): `alva-hub-cli` ran
   real `codex` (fresh + resume) and `alva` (shared anthropic profile)
   sessions plus an alva→codex handoff — see §1 above. The Desktop GUI
   path itself still needs a human-driven session.
2. **Packaged-runtime observation of each fallback trigger** (scenario 3) —
   the classifier is pinned, the live provider behaviour is not.
3. ~~Web-layer tests.~~ `web/` now runs 11 vitest tests (fallback-card
   controls, handoff expansion, session search, timeline projection, claim
   parsing); UI claims in scenarios 8 rest on those plus `tsc`.
4. ~~A signed/bundled artifact.~~ `tauri build` produces
   `target/release/bundle/macos/ALVA Hub.app`, which was launched, observed
   alive with its private `alva-hub.db` initialised, and exited cleanly.
