# Round 6 UI spec review

Scope: `5c64af16..5290e033`, R40–R45/R47/R53–R55/R59. Read FRACTAL-DOCS, web/src/AGENTS, UI glossary/wireframes/motion and each original round-5 card. No production edits. Root owns broad tests; this review ran only three independent component probes.

## Confirmed findings

1. **P2 / R42 — Locate record does not expose its record.** `web/src/components/workbench-tools.tsx:208–231` sets only the innermost tool ID and immediately queries DOM; the product-attempt, turn and attempt Disclosures at lines 287–296 remain closed. Radix does not mount their children. Opening Inspector in its normal collapsed state and clicking an enabled Locate record leaves the target absent; native Inspector only receives session ID, not a tool-record target. Probe fails `expected null not to be null`. Expand the complete owning path, target by attempt + tool identity, then scroll/focus after mount; test initial collapsed state, failure and finished tools, and keyboard activation. Original R42 requires opening and locating the correct record. Its “execution overview” at lines 194–197/254–285 is also only an activity list: Input/Agent/shared-clock timeline overview remains absent, so “完整迁入” is not complete.

2. **P2 / R54 — Read-only executions still claim changes.** `workbench-tools.tsx:79,83,86` retains `changeCount`, `failedChange`, `sessionChange`. A successful `file_read` produces “1 项变更…会话变更”; failures still become “变更失败.” Probe confirms. Replace the remaining metadata/status vocabulary with tool execution/record vocabulary; preserve null→pending. Original R54 explicitly forbids claiming read-only tools modified files.

3. **P2 / R55 — Completely missing usage escapes partial-cost detection.** `workbench-tools.tsx:403,408–415` discards `attempt.usage === null` before counting unknown costs. One $0.12 attempt plus another completed attempt with no usage displays `$0.12` without partial; probe confirms. Count coverage against applicable attempts, while retaining the existing measured token/duration sums. Original R55 requires mixed known/unknown to be visibly partial. The generic description says only reported usage is summarized, but cannot distinguish the new complete/partial cost states consistently.

## Per-card disposition

| Card | Outcome | Evidence / limits |
|---|---|---|
| R40 | Implementation accepted; live acceptance pending | Role-scoped merge correctly updates tool/fallback bodies while retaining assistant prose; three new reducer assertions cover captions. No real ToolStart/End integration in this review. |
| R41 | Character fix accepted; reading acceptance partial | Raw React children prevent double escaping; actual ChatTranscript tests cover injection/copy. Scroll test sets impossible normal-layout `scrollTop=-500` with zero geometry (`chat-transcript.test.tsx:95–126`); proves branch/state behavior, not browser scroll/long-code geometry. R45 must retain browser reading acceptance. |
| R42 | Rework | Confirmed finding 1; do not close based on visible overview heading or screenshots. |
| R43 | Copy scoped appropriately; matrix pending | Both languages now scope the filter to ALVA; no full engine×profile eligibility evidence. No new contradictory guarantee identified. |
| R44 | Accepted disable route | Sample name/email and fake save/cancel removed; editing disabled with truthful explanation. |
| R45 | Open, correctly acknowledged | Card explicitly says partial/open. Neither behavior.spec nor visual.spec changed in this range. Existing demo suite lacks real permission events, actual scroll restoration, two-density matrix, and full Tab navigation (programmatic `.focus()` remains at behavior.spec:98,104,134). Execution note saying real-layout scroll/two-density coverage was inherited is overstated. Carry old overlay inert/focus/Escape and handoff aria gaps forward. |
| R47 | Mostly accepted; minor residue | Chat spacings/radii tokenized, but `.chat-inline-code` padding still contains `1px` at app.css:1215. Do not create broad styling rewrite; finish alongside another UI cleanup. |
| R53 | Accepted disable route | Text preview clearly unwired; browser/terminal input and execution disabled; fake queued/ready paths removed. |
| R54 | Rework | Confirmed finding 2; null pending and no-record Git-clean inference fixed successfully. |
| R55 | Rework | Confirmed finding 3; all cost:null, actual zero and mixed within present usage are implemented correctly. |
| R59 | Accepted limited index; keyboard acceptance pending | Sessions + three real navigation actions indexed, explicit no-results state; settings query now matches a real action. Existing StatusListItem button semantics support keyboard. No new end-to-end keyboard assertion. |

## Independent validation

Archived `evidence/ui-round6.test.tsx`, copied temporarily to `web/src/components/review-round6.test.tsx`, ran `./node_modules/.bin/vitest run src/components/review-round6.test.tsx`: **3 tests, 3 expected regression failures**, exact output `evidence/ui-probes.txt`. Temporary production-directory test removed. Inspector's rejected fixture loader emits asynchronous act warnings; the target absence follows synchronous disclosure ancestry and is independent of network/store loading. No browser/real-store claim is made by these probes.

## R57 follow-up: permission effect summary lost in Chinese

**P2, independent static confirmation.** `crates/alva-engine-adapter-alva/src/adapter.rs:327` now maps `req.effect_summary` into PermissionRequest.description as intended. But `web/src/components/hub-timeline.tsx:182–188` renders that description through `localizeSystemText`, whose `i18n.tsx:1480` regex replaces unrecognized multiword English with “暂无中文诊断摘要，请在检查器中查看技术详情。” The exact custom tool self-description used by the new Rust test (`security.rs:726`), “publishes the draft post to the production blog”, therefore disappears from the default Chinese approval card while Allow/Deny remain available. This is substantive permission-decision information, not a dispensable diagnostic caption. Preserve tool-authored text as safe text in every locale (optionally add translated framing); do not guess effects again from tool names. R57's original acceptance requires the summary to reach the user-facing approval prompt, so the DTO fix alone cannot close the card.

The independent probe `evidence/ui-r57.test.tsx` uses that exact description in a permission DTO, renders HubTimeline in Chinese and English, and asserts the description alongside two decision buttons. Dynamic execution status will be appended after completion.

R57 dynamic result: `./node_modules/.bin/vitest run src/components/review-r57.test.tsx` completed: **Chinese FAIL / English PASS (2 tests)**. Chinese rendered exactly `操作授权请求暂无中文诊断摘要，请在检查器中查看技术详情。允许拒绝`, omitting the effect. Full output: `evidence/ui-r57-probes.txt`. Temporary src test removed. This confirms the defect at the actual permission-card renderer, with English as the control; it is not a full live-store/Tauri execution test.
