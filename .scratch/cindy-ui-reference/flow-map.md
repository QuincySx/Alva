# Cindy 0.1.45 Flow Map

## Global application flow

```mermaid
flowchart TD
  Launch["Launch Cindy"] --> Gate{"Session / migration state"}
  Gate -->|"Existing signed-in session"| Shell["Persistent main shell"]
  Gate -->|"Signed out"| Login["Login / skip-login gate (source-only)"]
  Gate -->|"Legacy data"| Migration["Migration gate (source-only)"]
  Shell --> Update["0.1.45 release notes overlay"]
  Update --> Shell
  Shell --> NewTask["New task draft"]
  Shell --> Task["Existing task"]
  Shell --> Automation["Automation"]
  Shell --> Plugins["Plugins / skills"]
  Shell --> Search["Global search"]
  Shell --> Settings["Settings"]
  Task --> Workbench["Right workbench"]
  Workbench --> Detached["Detached sidebar window"]
  Detached -->|"Merge back"| Workbench
```

Login, skip-login, and migration branches exist in the packaged application but were not entered because the live account is already authenticated and logging out or mutating migration state would violate the read-only boundary.

## New task flow

```mermaid
flowchart LR
  New["New"] --> Target["Conversation / project target"]
  Target --> Composer["Empty composer"]
  Composer --> Actions["Action menu"]
  Actions --> Attach["Add file / image / video"]
  Actions --> Goal["New Goal"]
  Actions --> Plan["Plan mode"]
  Actions --> Collab["Collaboration mode"]
  Actions --> Plugin["Installed plugin"]
  Actions --> Ref["Read-only directory"]
  Composer --> Permission["Permission mode"]
  Composer --> Engine["Engine"]
  Composer --> Model["Model + reasoning"]
  Composer --> Voice["Voice input"]
  Composer --> Send["Send (blocked in forensics)"]
  Goal --> Limits["Max turns / token budget / no-progress turns"]
  Limits --> Start["Start Goal (blocked)"]
```

Business conditions:

- Send is disabled while the composer is empty.
- Goal Start is disabled until a description exists.
- Goal advanced limits are scoped to the current goal.
- The target picker can remove a project or open a system folder picker; neither branch was taken.
- The 0.1.45 new-task target list contained Conversation and local Projects, not a phone device.

## Existing task flow

```mermaid
flowchart TD
  Task["Task timeline"] --> Header["Header task menu"]
  Header --> SafeOps["Pin / rename / move / copy link / open window / export"]
  Header --> RiskOps["Archive / delete (blocked)"]
  Task --> PlanChip["Plan progress chip"]
  PlanChip --> PlanPopover["Ordered steps + current step"]
  Task --> MessageActions["Per-message actions"]
  MessageActions --> Share["Share as image selection"]
  Share --> One["Default one message"]
  Share --> All["Select all: 23 messages"]
  One --> Output["Download / copy image (blocked)"]
  All --> Output
  Task --> Changes["Changed-file card"]
  Changes --> Review["Review"]
  Changes --> Undo["Precise undo (blocked)"]
  Task --> QuestionNav["Previous-question navigation"]
  Task --> Workbench["Open right workbench"]
```

0.1.45 task-menu delta: “Open in split” is absent; “Open in new window” remains. Dragging a task out to create a window was not performed because it changes window/session layout, but the packaged app contains a session-drag-release helper and the equivalent explicit menu path is visible.

## Workbench flow

```mermaid
flowchart TD
  Open["Expand"] --> Tabs["Tab strip"]
  Tabs --> Review["Review"]
  Tabs --> File["File / SVG preview"]
  Tabs --> Sub["Subagent"]
  Tabs --> BG["Background tasks"]
  Tabs --> Usage["Resource usage"]
  Tabs --> Browser["Browser"]
  Tabs --> Terminal["Terminal"]
  Usage --> SelectProc["Select process"]
  SelectProc --> Terminate["Terminate process (blocked)"]
  Tabs --> Add["Add tab catalog"]
  Open --> Max["Maximize"]
  Open --> Detach["Open sidebar in new window"]
  Detach --> Warm["Tabs and resource table restored"]
  Warm --> Merge["Merge back to main window"]
```

Current-context constraints:

- Subagent showed its persistent empty state because this task has no Subagent.
- Orca Workers and iOS Simulator strings/assets exist in the app package, but neither appeared in the current Add Tab catalog.
- Terminal and Browser were not interacted with beyond catalog inspection.
- Maximize was visible but not needed; detach/merge was fully verified.

## Settings flows

```mermaid
flowchart TD
  Settings["Settings"] --> General["General"]
  General --> Profile["Edit profile"]
  General --> Logout["Logout (blocked)"]
  General --> DeleteAccount["Delete account (blocked)"]
  Settings --> Providers["Model providers"]
  Providers --> ProviderMenu["Provider menu"]
  Providers --> ModelMenu["Model menu"]
  Providers --> AddProvider["Add provider catalog"]
  AddProvider --> Custom["Custom endpoint"]
  Custom --> Test["Test connection / get models (blocked)"]
  Custom --> SaveProvider["Save (blocked)"]
  Settings --> Tools["Tools"]
  Tools --> AddMCP["Add MCP HTTP/SSE"]
  AddMCP --> SaveMCP["Save / authenticate (blocked)"]
  Settings --> Remote["Remote connections"]
  Remote --> SSH["SSH hosts"]
  SSH --> AddSSH["Add SSH host"]
  AddSSH --> Connect["Save / connect (blocked)"]
  Settings --> IM["IM Bots"]
  IM --> WeChat["Personal WeChat setup"]
  WeChat --> Auth["Tencent authorization / connect (blocked)"]
  Auth --> Contacts["Contacts management (gated)"]
```

## Automation flow

```mermaid
flowchart LR
  List["Automation list / templates"] --> Editor["Template editor"]
  Editor --> Kind["AI or Script"]
  Editor --> Trigger["Automatic or Manual"]
  Trigger --> Once["One-time"]
  Trigger --> Cron["Cron / relative schedule"]
  Editor --> Session["New each run / continuous / bound task"]
  Editor --> Project["Project / worktree"]
  Editor --> Route["Engine / model"]
  Editor --> Notify["Silent / Feishu / WeCom"]
  Editor --> Create["Create (blocked)"]
```

## Runtime-only and error flows

These states need runtime prerequisites and were not synthetically forced:

- AskUserQuestion, permission confirmation, plan review, queue panel.
- Plan running breathing dot and plan failure reconciliation.
- Streaming text animation and suppression of replay when switching tasks.
- Interruption, credential switching, rollback preview, generated files, media/Mermaid/model lightboxes.
- Database fatal page, crash recovery page, provider/network errors.
- Signed-out login/skip flow and data migration flow.

They remain documented in coverage.md as source-only or context-gated branches rather than being mislabeled as live-captured UI.
