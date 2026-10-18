# L4 · Diagnostic tooling
> Real on-demand approved command profiles with a replaceable manager, not a fake empty LSP scaffold.

## 地位
Optional native extension; Manager is consumed by the actual project_diagnostics Tool and optionally published as a narrow typed capability. SDK contains no diagnostic/process policy.
## 逻辑
New copies explicit profiles or consumes an explicitly supplied Manager. Registration/start only binds actual session/run; no program starts until an approved run action. Native execution reports prior unknown activity and commits same-session diagnostic_intent before platform/host/command. Outcome preserves actual exit/reap/capture state and original-source sanitized stdout/stderr references.
## 约束
No ambient language/binary/server/account discovery, arbitrary model-selected argv, package installation, automatic scan/LSP startup, parent-history grants or replay. Profiles 1..16 with opaque IDs, timeout≤5min, 1MiB each stdout/stderr, 32 invocations/32MiB captured I/O per actual run; tool input/output metadata≤4KiB. Catalog pagination is at most5 entries with an explicit cursor and dynamic byte bound. All run actions are Unknown effects requiring Guard/HITL; metadata listing is read-only. Actual Tool scheduling is SerialGlobal/ManagesOwnTimeout; hosts share SDK ToolLockRegistry for cross-Agent locking. Same real session authority/cwd/run/actual-dispatched-tool is required even when consuming Manager directly. Manager checks actual call action/profile, so a list-call approval cannot execute another profile. A custom Manager's nil error still requires consistent started/reaped/complete exit evidence plus exact retained artifact metadata in this actual native session; invalid/foreign/forged source references fail. Callback cancellation and current run binding are rechecked before accepting reports. All nil-capable typed-nil Managers reject before Catalog. Tool and direct native Manager use the same private strict bounded invocation decoder; empty CallID and ambiguous JSON reject. Exit1 is an explicit model-recoverable nonzero command result, not zero diagnostics; transport/timeout/capture/store/cleanup failures remain fatal. No parser is provided and no empty parsed diagnostic list is fabricated. Known fields and each exact profile’s explicit Env values are redacted, including unlabelled echoes. Incomplete captures omit the unterminated tail and state original/omitted byte counts; JSON normalization/invalidUTF8 replacement is stated in every source reference. Full LSP server semantic queries remain a separate future capability, not claimed complete here.
An invocation waiting for permission/manager admission is checked again after the execution gate; ending or replacing its actual run never lets a stale waiter start a command.
## 业务域清单
| 文件 | 职责 |
|---|---|
| plugin.go | Actual manager contract, explicit profiles, session-bound Tool, permission and assembly |
| invocation.go | Shared private exact action/profile decoder for Tool and direct native Manager; no extra public abstraction |
| boundary_test.go | Actual/foreign/forged custom Manager evidence, nil-map Manager and late cancellation regressions |
| runner.go | Prior activity/facts, native command and truthful same-session source evidence |
| plugin_test.go | Actual SDK/process/SQLite, coalesced HITL, prior activity, complete tails, errors, redaction, manager consumer/empty-evidence denial and bounded immutable catalog |
