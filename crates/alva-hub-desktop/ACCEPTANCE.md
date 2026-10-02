# ALVA Hub Desktop V1 — Acceptance Matrix

## Recursive ALVA context policy and authority — 2026-10-03

App-core `agent` children now bind the product reference-first policy to their
actual repository-created session ID. Each child/grandchild gets an own-history
reader, short policy prompt, first assembly hook and before-provider middleware;
parent-bound readers are replaced. Missing sources latch a per-child error and
stop the provider even if subsequent hooks remove entries. Communication hooks
are trusted code, not sandboxed; no new child compactor or SDK policy was added.
Empty host-tool grants still allow bounded own-history reads, not parent records.

Recursive schemas, template inherit-all and execution grants now use the direct
parent's actual tools, preventing grandchildren from regaining excluded root
tools. Default model inheritance uses the actual parent model too. Existing
security approvals, resource locks, cancellation/depth/budgets and typed causal
session links remain. Real BaseAgent parent/child/grandchild tests prove large
text/media references before communication/provider calls, explicit own reads,
cross-session denial, durable original identities and post-reopen retrieval.

This batch passes **344 app-core tests** (328 lib + 16 security/lock/workspace/
swap/wiring integration), **43 ALVA adapter, 246 Desktop lib / 4 exact account
tests filtered, 4 Desktop bin, 14 HTTP**: 651 tests, including six new regressions.
Desktop/server all-target check, public snapshots, dependency/bus/wasm32/host-link
gates pass; wasip2 is explicitly unavailable/skipped. UI, native AX/visual and
performance evidence were not renewed. No real accounts, billing, external MCP,
new dependencies, SDK/kernel edits or publication. Cross-branch/engine product
history, media transformations/selection/delivery receipts and schema JSON-path
retrieval remain outstanding; the prior runtime evidence below is historical.

## Reference-first ALVA runtime — 2026-10-03

Desktop/server ALVA factories now share their native SessionRepository with a
ContextRetrievalPlugin. Tool text >4 KiB, any ToolContent image, and every managed
MCP invocation result become verified durable references in the model-request
copy, before existing automatic compaction. Original redacted records, UI/Inspector
events, error status and identities survive. Bounded retrieval is exempt from
re-referencing; compaction references retain their original source.

Managed MCP registers one gateway and ≤4 KiB/32-server prompt index, not all
remote schemas. Search is paged, describe selects one contract, call validates an
exact connected catalog identity; the existing HITL applies to all gateway actions.
Native stdio retains 1 MiB frames while supporting 8192 tools/128 pages with a
32 MiB cumulative catalog cap. Real authored fixtures prove 5000-tool discovery,
single registry entry, selected schema, host budget rejection, normal approval,
600 kB original result → model reference → history search. Generic >1 MiB outputs,
reused call IDs, altered-source rejection, UTF-8 limits, missing/corrupt blobs,
typed MCP media/data-URI exclusion and compaction/restart are covered.

History search/read is fixed to the current native ALVA session and its execution
context, not cross-engine Hub history. It excludes internal provider/hook events,
hidden reasoning and typed media bytes, with high-water pagination and explicit
128-record/64-MiB scan, 20-hit, 2-KiB read and 16-KiB response budgets. Hub-owned
ALVA Workers use this factory. The initial batch did not include recursive
children; the subsequent coverage is recorded above.
External engine loops and actual media transformation/delivery receipts remain pending.

Initial runtime verification: **40 ALVA adapter, 325 app-core lib, 86 MCP protocol,
246 Desktop lib / 4 exact account tests filtered, 4 Desktop bin, 14 HTTP** tests
pass. Desktop/server all-target check, new-file rustfmt, diff check, public SDK
snapshots and dependency/bus/wasm32/host-link gates pass; wasip2 target remains
unavailable/skipped. The initial parallel Desktop run had one existing reopen-test
lock failure (245 passed); the full serial run passed 246. No claim that this
intermittent lock was diagnosed or fixed. UI/Chrome/native performance were not
rerun in this backend-only batch; prior UI/upload evidence below is historical.
No real accounts, paid models, external MCP services, downloads or publication.

## Conversation foundations and reference-first input — 2026-10-03

Verified: real recent-session home/empty-workspace draft actions; literal user
messages, safe assistant Markdown/tables/quotes/native external links, visible
failure reasons, copy feedback, IME/Shift Enter and busy Stop; search/navigation
across unmounted loaded entries without re-enabling follow. Search/copy includes
loaded messages and tool previews, not paged full tool bodies or AI history search.
Trusted compiled renderer registry consumes bounded alva.display v1 table/chart/
diagram data from assistant text and complete tool bodies; raw source survives
unknown/invalid/failing renderers. No remote plugin installer or executable HTML.

Schema 23 stores original attachment bytes and processing metadata in private
SQLite plus unique no-follow/create-new files under the saved session cwd.
Limits: **20 MiB/file, 40 MiB/8 files per Turn**, and a run-only 64 MiB HTTP body
cap. The upload prompt contains only references/manifests, never file bodies.
Images/videos can be uploaded and supported formats locally previewed with
revocable Blob URLs. Source hash/size/type are durable; no compression/transcode/
frame extraction/AI media analysis was performed. delivery_scope=upload_prompt
does not claim later tools never read it or that a model request completed.
Other sessions cannot read the private preview API; shared cwd is not OS isolation.
Failure may leave only new copies/directories; no implicit project cleanup.

Latest checks: **233 UI tests / 24 files; 246 Desktop lib tests / 4 exact real-account
tests filtered; 4 Desktop bin tests; 14 HTTP tests; 6 real Chrome windowing tests**.
HTTP includes a successful exact-20-MiB binary upload, durable bytes/provenance
and cross-session denial. Other regressions cover original-byte Unicode/binary
fidelity, reboot, schema-22 migration, symlink rejection, correlated-start failure
draft restoration, module validation/error fallback and native opener failures.
TypeScript/Vite build, Desktop/server all-target check, diff/rustfmt, public
surface snapshots and dependency/bus/wasm32-unknown-unknown/host-link gates pass.
wasm32-wasip2 remains unavailable and explicitly skipped. Existing warnings and
the >500-kB bundle warning are recorded, not treated as measured performance.

Outstanding after the deliveries above: **cross-engine product history,
actual media-derived operations and delivery receipts**.
Generic results, progressive managed MCP and native ALVA history are implemented
within the stated scope. See ADR-0011 and docs/ALVA-HUB-CONTEXT-RETRIEVAL.md for accepted
direction and concrete acceptance. Packaged WKWebView/AX/visual/performance
acceptance remains deferred; Chrome tests do not renew native claims. No real
accounts, paid models, user MCP services, downloads or external publication.

## Current workbench delivery — 2026-10-02

This section is the current evidence ledger. Older V1 scenarios below are
retained as historical evidence, not renewed claims about today's packaged
GUI, real accounts, or performance. Automated, native-PTY, HTTP, and
WKWebView evidence are deliberately separate.

Environment: macOS 27.0 arm64, rustc 1.98.1, Node v24.19.0. All new commits
are local and unsigned (command-scoped signing override after the existing
1Password failure); nothing was pushed. Existing user edits were preserved.

| Area | Implemented and verified | Still outstanding |
|---|---|---|
| H09/H20 conversation | Attempt-scoped UTF-16 positions; duplicate/late fragments; schema-20 durable visible assistant journal; session-scoped cursor recovery; crash terminal repair; ALVA foreground startup Stop disposes pending assembly, and incomplete Hub EOF fails without fallback | Production WKWebView latency/scrolling matrix; old unstored fragments cannot be reconstructed; other engines still require native start for UI Stop |
| H09 tool body | Schema-19 atomic redaction + immutable UTF-8 chunks; Tauri/HTTP paging; page-only UI retention, retry and stale-response isolation | Full native paging flow; legacy previews have no recoverable full body; per-body cap is 64 MiB, not a total-database quota |
| H10 Files/Text | Real persisted cwd, no-follow capability-relative opens, existing SensitivePathFilter, bounded text/binary/permission states | Native multilingual and narrow-container visual pass |
| H11 Git | Real worktree branch/status/diff, staged/unstaged/untracked, literal/NUL paths, independent linked worktrees, read-only limits | Native UI refresh after a real task; no new Git write actions |
| H12 user browser | Explicit session-owned incognito native window; navigation/reload/history/close; HTTP(S)-only URL policy and no Hub IPC/popups/downloads | Native navigation/error/close lifecycle interaction pass; HTTP is explicitly unsupported |
| H13 terminal | Real Unix PTY tests for cwd/input/resize/Ctrl-C/exit/reopen/group cleanup; bounded output and session ownership | Native UI lifecycle pass; line-oriented, not a fullscreen emulator or security sandbox; HTTP is unsupported |
| H14 Inspector | Real SQLite flush/reopen preserves typed records and recursive child/grandchild ownership; product/attempt detail, unsupported and missing states | Native recursive page/focus/keyboard pass; no non-Alva internal trace is invented |
| H17 secondary pages | Real Inspector navigation and scoped shortcuts; persisted theme/density/reduced motion; session-scoped Skills create/edit/archive/restore; schema-21 paused-first local interval automations; schema-22 ALVA capability switches and trusted local stdio MCP CRUD/discovery/calls, shared by Desktop/HTTP; included-plugin tab replaces sample installations | Native theme/Skills/automation/MCP/keyboard/narrow/multilingual pass; cron/daemon, remote MCP/auth, third-party plugin install registry, Worker tuning and SSH/IM remain unavailable |
| K01 boundaries | Dependency firewall, bus surface, public symbol snapshots, wasm32 clean set and host full link pass; kernel/agent-core implementation unchanged | wasm32-wasip2 target is not installed, explicitly skipped |

### Reproducible automated checks

From the repository root:

```sh
cargo test -p alva-hub-desktop --lib -- --test-threads=1 \
  --skip real_claude_runs_and_completes_a_turn \
  --skip real_codex_runs_and_completes_a_turn \
  --skip real_codex_receives_a_fallback_after_engine_failure \
  --skip real_codex_worker_task_executes_and_closes_with_result
cargo test -p alva-hub-desktop --bin alva-hub-desktop -- --test-threads=1
cargo test -p alva-hub-server -- --test-threads=1
cargo test -p alva-protocol-mcp -- --test-threads=1
cargo test -p alva-app-core --lib -- --test-threads=1
cargo test -p alva-hub-core -p alva-engine-adapter-alva -- --test-threads=1
cargo check -p alva-hub-desktop -p alva-hub-server --all-targets
bash scripts/ci-check-deps.sh
bash scripts/public-surface.sh
```

Latest startup-Stop pass: **341 Rust tests** comprising **239 Desktop lib
tests / 4 explicitly filtered real-account tests**, **4 bin tests**, **12 HTTP
tests**, **51 Hub core tests** and **35 ALVA adapter tests**; all-target check
passes. The preceding MCP corrective pass additionally verified **85 MCP
protocol tests** and **323 app-core lib tests**; these two packages were not
rerun in the startup-Stop pass. The dependency
firewall initially exposed the missing declaration of Desktop's direct
SensitivePathFilter dependency; its single intended use was documented and
the gate rerun successfully. No Rule 17/18/19 was removed. The final script
also passed bus lint, public symbol snapshots, wasm32-unknown-unknown checks and host linking;
wasm32-wasip2 was skipped because the target is absent.

Target precision: this checkout's Rule 18 loop checks `alva-app-cli` and
`alva-app-gpui`; `alva-app-tauri` is not a Cargo workspace member here.
The script's success is not claimed as a check of that absent target, and
this work did not rename or migrate the separate native product entry.

In `crates/alva-hub-desktop/web`: `npm test -- --reporter=dot` reports
**210 tests / 20 files**, and `npm run build` passes TypeScript/Vite, both
rerun in the startup-Stop pass. The preceding MCP corrective pass changed
no UI code; neither pass renewed native GUI acceptance.
Existing React act/controlled-state, Rust unused-item and large-bundle
warnings remain. This iteration made no real provider or paid SDK calls,
installed no new runtime, and did not rerun a blocked one-off Playwright
package command. Unit tests are not claimed as visual or performance proof.

Appearance additionally has 13 dedicated regressions: reload persistence,
live system theme, OS accessibility precedence, invalid/oversized preferences,
visible storage failure/retry, cross-window updates and StrictMode cleanup.
Light foreground tokens meet AA contrast against the tested normal, hover,
active and inset backgrounds. Root density tokens affect Controls/Surfaces
without reducing text size. Reduced motion removes displacement but preserves
dialog centering, switch state and spinner progress; this is a source/behavior
contract, not native visual acceptance.

Workspace Skills uses session-persisted cwd, canonical protocol parsing,
capability-relative no-follow file access, 128-KiB manifests and a 200-entry
scan limit. Create never replaces an existing directory; only managed project
files can be edited, with an optimistic source-revision check. macOS/Linux
archive/restore uses atomic no-replace directory moves and preserves resources;
unsupported hosts do not expose those actions. Existing project enable-lists
keep unknown/global names and extra fields. Ecosystem files are read-only;
global directories are intentionally not scanned and this is not a complete
runtime registry. No remote installation or script execution is introduced.
Six backend regressions prove real FsSkillRepository discovery, edits, state
preservation, archive/restore, limits and traversal/symlink rejection. Seven
UI regressions and one App integration cover actions, focus, errors, drafts and
late session replies; real HTTP verifies create/edit/archive/restore without
engine execution. HTTP fixtures now use an explicit private catalog instead
of the production shared/legacy configuration constructor.

Local automations bind an existing ordinary HubSession and a stored branch/model/
provider/account/billing snapshot. Create/edit is paused; enabling requires an
explicit billing review in the UI and a matching revision/route in the store.
Schedules use 5–10080 minute intervals, not cron/local-time calendars. The product
pump polls every 10 seconds with four execution slots; schedule claim and Turn
creation commit together. Each run uses the same durable execution, timeline,
approval and Stop paths but never automatic fallback or TeamRun auto-approval.
Busy ticks and ticks missed by at least a full interval are skipped; success
waits one full interval, failure/Stop/route change pauses. Restart delays enabled
idle schedules and pauses interrupted ones. No daemon runs after the application
or headless service exits, and no missed work is replayed. Pause does not cancel
a current Turn; open its session to Stop. Account credentials/CLI logins remain
the currently configured account, not an immutable credential snapshot.
Ten backend regressions include real engine doubles, no-fallback, duplicate
claims, concurrency, limits, migration and restart. Eight UI regressions plus
an App navigation test and real HTTP CRUD prove actual configuration writes;
isolation rejects enable and starts no pump. A schema-18 test fixture was updated
to actually remove post-18 tables; production migrations were not weakened.

ALVA capability preferences use ten real component-catalog groups, not a
parallel tool registry. Every new Desktop/HTTP run captures one persisted
snapshot, including fallback assembly; it does not mutate a running agent or
control Claude/Codex/Pi tools. Security, approvals, tool timeouts and loop guards
cannot be disabled. These are availability controls, not a sandbox; child
agents may have their own tools. Hub-managed mode excludes ambient MCP/AEP
discovery, even when its explicit server list is empty.

Managed MCP currently supports installed native stdio programs only: absolute
executable plus literal argv, no shell parsing/install/download, inherited
credentials, environment-secret fields or remote HTTP/SSE. Configuration is
private schema-22 data with optimistic revisions, 32-server limits and bounded
fields. Create/edit always disables; enable and one-shot connection tests need
explicit host-access confirmation. Tests bind a saved session cwd, have at most
two concurrent slots, never call a model or enable a server, and close the
owned test process. Deleting removes only configuration, not programs or an
already running agent's connection. Isolated UI QA permits configuration but
rejects enable/test execution.

The native transport performs real initialize/initialized, paginated tools/list
and tools/call with 1-MiB frames, 256-tool/16-page bounds, request timeouts,
cancellation invalidation and Unix owned-process-group cleanup. Discovery has
per-server timeouts and a 60-second aggregate startup budget. Closed transports
do not remain callable; reconnect failures clear obsolete tools. Direct MCP
tools and mcp_runtime both use the existing SecurityGuard/HITL path; server
read-only annotations grant no authority. Stable provider-safe aliases retain
server/tool identity in descriptions and resolve exact legacy template names.
Native fixture subprocesses prove actual handshake/discovery/call/cwd/literal
argv, no ambient HOME, cancellation and static failure messages, without
external packages or accounts. Store/migration, real HTTP, actual ALVA catalog
assembly/approval and seven UI regressions verify the functional pipeline.
The included-plugin page is not a third-party AEP installer. Visual polish was
explicitly deferred by the user; these tests are not native visual acceptance.

The corrective pass adds eight regressions, each observed failing before its
fix: identical MCP registration preserves live connections; changed configs
replace them; discovery shares the handshake deadline and settles failures;
cancelled discovery leaves no stale Connecting state. Protocol catalog/state
snapshots exclude closed transports, calls reject them, and explicit reconnect
restores service; busy RPC snapshots remain nonblocking. An existing harness
regression additionally pins closed-transport call rejection. Templates with a
declared but unavailable MCP set keep an explicit empty whitelist instead of
silently inheriting all parent tools. No SDK kernel or agent-core code changed.

The startup-Stop pass adds five red-to-green regressions: cancelling ALVA
during factory assembly drops the pending build before model calls; dropping
the startup stream releases assembly/session ownership; dropping a running
stream interrupts a permanently pending model and permits a fresh-token
resume. The product seam uses real private SQLite and an authorized fallback
double to prove pre-native-start Stop is failed, not successful, with no
fallback assembly and a durable cancellation fact after reopen. The App
regression enables Stop only after the correlated foreground ALVA user
message, ignores foreign worker tokens, and retains control until the real
terminal rather than the cancel acknowledgement. Other engines still wait
for running; restored pre-start sessions are not newly enabled. No runtime
event/schema/public SDK changes, real account calls or native GUI claims
were introduced.

### Native evidence and deferred acceptance

The debug fixture uses real private stores and two local Git worktrees,
but only a deterministic EngineRuntime double. A real Tauri/WKWebView
window displayed the completed conversation, Markdown/code and first
16-KiB tool page with a five-page pager. The preserved
[partial native screenshot](../../.scratch/alva-hub-workbench-reset/native-before-worktree-fix.png)
is from **before** the worktree IPC correction, not a final acceptance image.
That native pass found the nested-Result worktree response defect, fixed in
`f179468f` and pinned by a serde IPC contract test.

After rebuilding, native automation could no longer read the window due
to macOS AX permission errors. The user explicitly deferred permission
repair on 2026-10-02 (“暂时不方便，做你能做的”). No bypass was attempted.
The full project → task → approval/stop → diff → resume → restart flow,
native browser/terminal/Inspector interaction, multilingual/narrow-window
review and production WKWebView performance budgets **remain unaccepted**.
Earlier Chromium traces are not WKWebView measurements; per-fragment SQLite
write latency introduced by the durable journal has not been measured there.
Real Claude SDK/Codex/Pi/provider-account tests require separate authority.

To resume **account-free** native QA later, use three terminals from the repo:

```sh
# Terminal 1: fresh explicit fixture directory, then the isolated desktop
hub_qa_dir=$(mktemp -d /tmp/alva-hub-ui.XXXXXX)
cargo run -p alva-hub-desktop --example native_ui_fixture -- "$hub_qa_dir"
cargo run -p alva-hub-desktop --bin alva-hub-desktop -- \
  --isolated-ui-data "$hub_qa_dir/data"

# Terminal 2: application UI at the configured development origin
cd crates/alva-hub-desktop/web
npm run dev -- --host 127.0.0.1 --port 1421

# Terminal 3, optional: browser fixture pages (use Terminal 1's printed path)
python3 -m http.server 1437 --bind 127.0.0.1 --directory <fixture-root>/pages
```

Start the UI server before launching the desktop. The isolate flag is
debug-only: it disables ambient provider/account probes, worker/automation pumps,
engine execution, automation/MCP enable, MCP tests and account mutations. Paused schedule and MCP CRUD are allowed. Browser and
PTY controls remain explicit user actions. The completed QA app and local
servers were stopped; fixture data was retained, not deleted. H18/H21 are
not marked complete by this handoff.

---

## Historical V1 evidence

Maps every scenario in `docs/ALVA-HUB-V1.md` §17 to its evidence. Two kinds
of evidence exist and they are **not** interchangeable:

- **Automated** — an executable assertion in this repo. Re-run with
  `cargo test -p alva-hub-desktop` (180 tests) plus the named suites.
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
3. ~~Web-layer tests.~~ `web/` now runs 63 Vitest tests (fallback-card
   controls, handoff expansion, session search, timeline projection, claim
   parsing, provider settings, workbench and shared UI primitives); UI claims
   in scenarios 8 rest on those plus `tsc` and 11 Playwright visual scenarios.
4. ~~A signed/bundled artifact.~~ `tauri build` produces
   `target/release/bundle/macos/ALVA Hub.app`, which was launched, observed
   alive with its private `alva-hub.db` initialised, and exited cleanly.

## UI wireframe review workflow

Screenshot review is not always possible (headless agents cannot read
images); the wireframe tool exists so any UI change can be verified from
text. It walks the live DOM of ANY web frontend — no framework hooks.

```bash
# 1. dev server running (or `npm run dev` in web/)
cd web && npm run dev &

# 2. headless Chrome with CDP
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" \
  --headless=new --remote-debugging-port=9223 --user-data-dir=/tmp/chrome-wire about:blank &

# 3. print the wireframe (real Tauri window size by default)
node scripts/ui-wireframe.mjs http://127.0.0.1:1421 --port=9223
```

Or paste `scripts/wireframe.js` into any page console and call
`wireframe()` (returns layout-tree + ASCII blueprint + node count).

This tool already caught real layout regressions: a composer-model line
stretched the topbar to 153px, and five sidebar panels overflowed a
469px viewport (now collapsed-by-default + all first-screen visible).
Any UI commit should include a wireframe pass showing the affected
region.
