# Cindy UI Coverage and QA

## Result summary

| Set | Manifest | JPEG | PNG | Linework / lineart | Dimension-paired | Unique PNG hashes | Unique linework hashes |
|---|---:|---:|---:|---:|---:|---:|---:|
| 0.1.45 live delta | 43 | 43 | 43 | 43 / 43 | 43 | 43 | 43 |
| 0.1.44 existing baseline | n/a | n/a | 67 | 67 cropped + 67 linework | not reliable for 15/31/32 | 66 at most because 31=32 | affected by mismatch |

Every new linework frame was produced deterministically with the existing CoreImage CIEdges pipeline:

- CIEdges intensity 3.2
- CIColorInvert
- grayscale, brightness -0.02, contrast 1.9
- crop to source extent
- PNG encoding

No generative redraw was used. manifest.jsonl stores original PNG SHA-256, raw JPEG SHA-256, linework SHA-256, pixel dimensions and linework_validated for every record.

## Evidence classes

- LIVE: observed in Cindy 0.1.45 and saved in the new manifest.
- BASELINE: present in the 0.1.44 reference collection.
- SOURCE: found in the 0.1.44→0.1.45 diff report or packaged app strings/assets, but not safely reproduced live.
- GATED: needs login state, credentials, an active runtime event, data mutation, crash/failure, external transmission, or a destructive operation.
- ABSENT: checked in the current live UI and not present.

The packaged-app string scan is only corroborating evidence, not proof that a branch is currently reachable. Representative exact-string counts in app.asar include AskUserQuestion 13, Orca Workers 1, iOS Simulator 170, login 702, skip login 4, migration 40, resource usage 18, slash 1099, mention 3905, Mermaid 304, contact 27, plan review 6, permission confirmation 6, queue 77, database 32, fatal 330, and crash 574.

## 0.1.45 delta checklist

| Requested delta | Status | Evidence / conclusion |
|---|---|---|
| Plan popover running breathing dot and failure reconciliation | PARTIAL LIVE + SOURCE | LIVE 024 shows the 2/6 plan popover and ordered steps. Current task was idle, so breathing dot and failure reconciliation were not manufactured. Diff report confirms the implementation change. |
| Subagent restored in chat flow | PARTIAL LIVE + SOURCE | LIVE 031/034 show restored Subagent tabs and persistent empty-state semantics. No active Subagent existed, so the populated inline runtime was unavailable. |
| Streaming output animation defaults on | SOURCE | Confirmed by the 0.1.44→0.1.45 diff report. Static screenshots cannot prove an animation default without starting a new run. |
| Drag task out to open window | SOURCE + equivalent LIVE | LIVE 028 shows “Open in new window”; packaged Resources contains tools/session-drag-release. Actual drag was not performed to avoid changing task/window layout. |
| Share selection and long-chat share image | LIVE | 029 shows selection mode; 030 shows Select all with 23 messages. Download/copy image was deliberately not executed. |
| Task menu removed split-open | LIVE | 028 definitively lists all header task actions and has no split-open item. |
| Resource usage window preheated | LIVE | 032 shows the populated resource tab; 033 shows the detached #/sidebar-window and populated table after a brief load. |
| New conversation no phone device | LIVE / ABSENT | 036/037 show all visible target choices. Only Conversation and local Projects appear; no phone/mobile device entry. |
| Switching tasks does not replay stream animation | SOURCE | Diff report evidence; proving it live would require switching during/after a streamed run and comparing motion over time. |
| Default question navigation bar | LIVE | Main task state and post-share task state expose “jump to previous question”; AX evidence is stored in 002/024/029/031 and manifest summaries. |

## Requested family reachability matrix

| Family / state | Status | Evidence / reason |
|---|---|---|
| Login | SOURCE + GATED | Current session authenticated. Logging out was prohibited. app.asar contains login UI strings. |
| Skip login | SOURCE + GATED | Requires signed-out first-run state; 4 exact strings in app.asar. |
| Migration gate | SOURCE + GATED | Requires legacy data/version state; 40 migration strings in app.asar. |
| Update prompt / update strip | LIVE | 001 captures 0.1.45 release notes. Sidebar release-log control is persistent. No pending auto-update strip was active. |
| Profile edit | LIVE | 040. No save. |
| Logout / account deletion | LIVE affordance + GATED | Buttons visible in General settings; neither clicked. |
| Provider add/edit | LIVE | 012–018 cover provider list, menus, add catalog, custom form, templates. |
| Provider pricing | PARTIAL LIVE + SOURCE | Balance/usage/recharge visible; custom form has no universal pricing editor. Price strings exist in app. |
| MCP add/edit | LIVE + GATED | 019–020 cover list and HTTP/SSE add form. Existing server edit and Save need configured data/credentials and were not used. |
| SSH add/key | LIVE + GATED | 021–023. Key manager/agent/private-key/pinning controls visible; no key, save, or connect. |
| Contact management | SOURCE + GATED | 041 captures the unconnected WeChat gate. Contact UI requires a connected channel; app strings contain contact references. |
| Goal creation | LIVE + GATED | 038. Start disabled while empty and never executed. |
| Goal advanced limits | LIVE | 039: max turns, token budget, no-progress turns. |
| Slash command picker | SOURCE / NOT TRIGGERED | app strings contain slash paths. Composer was not typed into to avoid creating/transmitting task content. |
| Mention picker | SOURCE / NOT TRIGGERED | app strings contain mention paths; runtime picker not triggered. |
| File/folder attachment | LIVE affordance | 027/036 show Add file/image/video and Add directory. System pickers were not opened. |
| Extra read-only directories | LIVE affordance | 027/036. No directory added. |
| Fast mode | SOURCE | Fast-related strings and version diff exist; no explicit fast toggle was exposed in the captured model selector. |
| Permission menu | LIVE | 025 shows Default, Auto approve, Full access. No mode changed. |
| Permission confirmation | SOURCE + GATED | Requires a running tool request; 6 exact strings in app. |
| Queue panel | SOURCE + GATED | Requires queued work; 77 queue strings in app. |
| AskUserQuestion | SOURCE + GATED | Requires an agent tool call; 13 exact strings in app. |
| Plan review | LIVE partial + SOURCE | 024 gives plan details; dedicated review state needs a live plan handoff. |
| Rollback / rewind | LIVE affordance + GATED | Main timeline has precise Undo and Review; Git savepoints are off. No undo/rewind was invoked. |
| Interruption | SOURCE + GATED | Requires active generation; no task was started or stopped. |
| Credential switch | SOURCE + GATED | Provider menu exposes credential reacquisition/rotation; actions were not invoked. |
| Generated-file cards | LIVE | 002 and later main-task states show changed-file cards, counts, paths, Review and Undo. |
| Media lightbox | SOURCE / CONTEXT-GATED | Needs a message containing media. Not available in current selected task. |
| Mermaid | SOURCE / CONTEXT-GATED | 304 app strings; no current message exposed a Mermaid card. |
| Model lightbox | SOURCE / CONTEXT-GATED | Current model selector captured in 026; separate model lightbox needs suitable content/state. |
| Share selection | LIVE | 029. |
| Long-chat share selection | LIVE | 030 selected 23 messages. Final image output blocked. |
| Rollback preview | SOURCE + GATED | Requires enabled Git savepoints and a candidate rewind. |
| Orca Workers | SOURCE / ABSENT CURRENT CONTEXT | One exact string exists, but it is absent from live Add Tab menu 035. Likely capability/context gated. |
| iOS Simulator | SOURCE / ABSENT CURRENT CONTEXT | Packaged ios-simulator resource and 170 strings exist, but no Add Tab item in 035. |
| Workbench pop-out | LIVE | 033. |
| Workbench maximize | LIVE affordance | Visible in 031/032; not clicked because detach already verified the alternate layout. |
| Resource usage standalone window | LIVE | 033. |
| Database fatal page | SOURCE + GATED | Needs DB failure/corruption; intentionally not induced. |
| Crash page / recovery | SOURCE + GATED | Needs process crash; intentionally not induced. |
| Provider/network error state | SOURCE + GATED | No invalid credentials or network tests were submitted. |
| Global find/search bar | LIVE | 042. |
| Search filters | LIVE | 043. |
| In-task find bar | SOURCE / NOT REACHED | “Find” strings exist, but no dedicated task find overlay was exposed in this traversal. |
| Mobile download | LIVE affordance | Persistent sidebar button visible; download not started. |
| Release/update log | LIVE | 001 and persistent sidebar control. |

## 0.1.44 baseline coverage

The existing baseline is useful for broad structural comparison:

- Chat/main shell: 00–07, 29–34, 49–52.
- Project/session menus: 53–58.
- Right workbench: 35–48.
- Automation: 08–10 and 62–64.
- Plugins/skills: 11–13 and 59–61.
- Settings: 15–28.
- Search: 14 and 65.
- Seven SVG wireframes and accompanying README/index.

This new set does not copy those 67 files. It focuses on 0.1.45 deltas and previously missing secondary forms, menus and gates.

## Baseline quality defects

The 0.1.44 reference has confirmed asset mismatches:

1. screenshots/31-chat-shell-return.png and screenshots/32-chat-shell-return.png are byte-identical:
   ac871a749342ebb91d3207c255531a838007330e25bb96a84b26f875beca1732
2. cropped/15-settings-home.png, cropped/31-chat-shell-return.png, and cropped/32-chat-shell-return.png are all byte-identical:
   06a7b11faa355f326a0b02347ca8a5223e91a075660268c3c0fc5778c5f67376
3. linework/15-settings-home.png, linework/31-chat-shell-return.png, and linework/32-chat-shell-return.png are all byte-identical:
   478d865fefb584007fe15db7de2cb4f5ca466c6785a7dbc70704b91adb400e4

Therefore baseline item 15 is a wrong crop/linework mapping, and 31/32 represent only one unique source state. The baseline was left unchanged to preserve provenance. Corrective action is to recrop 15 from its original screenshot and retain only one of 31/32 (or replace the missing state with a new capture).

## Deliberate safety exclusions

The following actions were never performed:

- Send a task/message, start a Goal, create automation, or run a plugin.
- Download/copy/share a conversation image or upload a file.
- Save profile changes, logout, delete an account, connect IM, or manage contacts.
- Enter/test API keys, Bearer tokens, SSH hosts, keys, or connect remote systems.
- Change permission modes, accept OS/app permissions, terminate a process.
- Archive/delete tasks or projects, undo/rewind code, corrupt a DB, or induce a crash.

## Residual coverage gaps

The live set is high-confidence for static, safely reachable 0.1.45 UI. It is intentionally incomplete for event-driven motion, authenticated first-run flows, provider/remote error handling, populated Subagent/Orca/iOS states, media renderers, and destructive confirmation dialogs. Those require a disposable test account/workspace, seeded fixtures, mocked runtime events, or a dedicated debug build.
