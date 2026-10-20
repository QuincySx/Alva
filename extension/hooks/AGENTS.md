# L4 · Lifecycle hooks
> Explicit approved native commands contributed through ordinary typed SDK middleware.

## 地位
Harness extension over typed function-hook adapters and PrepareTool; does not import engine or move process policy into SDK.
## 逻辑
Constructor clones only explicit approved executable/argv/cwd/environment entries without startup. Every matched invocation first reports actual unknown activity and commits a native hook_intent before execution. platform/host/command concurrently captures bounded stdin/stdout/stderr and actually reaps the owned group. Same-session artifacts redact both known markers and that exact command’s configured Env values (including unlabelled echoes); incomplete captures omit the unterminated tail with actual byte counts. Artifacts preserve known-secret-redacted evidence with truthful completeness/UTF8 processing; hook_outcome distinguishes success, explicit block, nonblocking exit and infrastructure failure.
PrepareTool receives the invocation-bound ToolPreparationContext and uses its Runtime for native ownership; argument updates preserve the selected tool binding and the kernel recomputes authorization/scheduling against that same invocation catalog.
Seven single-hook adapters use assembly order1500 before ordinary product layers. No PhaseHandler, effect classification or second execution registry is involved; each adapter exposes only its actual hook.
## 约束
No shell inference, ambient credentials, old settings import, global hook discovery, automatic retry or child inheritance. At most 32 configured entries, 128 invocations/32MiB captured I/O per run, 256KiB stdin, 1MiB each stdout/stderr and 1min per command. Nonempty successful stdout must be one strict typed JSON Output object: duplicate/incorrect-case keys, excessive nesting and invalid Unicode reject before control mutation. Exit2/continue=false/permission=deny blocks; other nonzero exits remain explicit nonblocking_error receipts, never succeeded. Spawn, timeout, parse, I/O, record and cleanup errors abort. Hook permission=allow never grants tool rights. UpdatedInput is only permitted in pre-intent PrepareTool and is re-authorized/rescheduled by SDK. Additional context and errors go to model only as small untrusted references (max8 receipts/4KiB per prepare); no raw hook transcript feed or automatic follow-up. General context budget is not re-run after this bounded notice injection. Explicitly configured resource locks are global barriers during each hook command, not an OS sandbox. Native facts and resources retain actual session/run identities. Run end seals ordinary hook admission, cancels and joins independently submitted Notify callbacks before final hooks, and retains ending ownership on join timeout. Dedicated failure/Stop/SessionEnd commands keep their explicit cleanup budgets; concurrent finalizers are serialized without replay. Join timeout is ErrFinalization as well as the context cause, never a successful/pure-cancel terminal. Explicit terminal Close cancels and joins both finalization and execution gates; CleanupComplete proves those callbacks have left. Owner cleanup never replays Stop/SessionEnd; actual Hub attempt resources retain the Hook instance until physical completion.
存储只消费本包按需组合的共享storage能力插口；会话/引用/DTO/错误从同一契约返回，不导入具体session.Repository或SQL实现。后端由上层程序/插件装配选择，组件不回落默认库、不downcast实现，不持有存储Close。

## 业务域清单
| 文件 | 职责 |
|---|---|
| contracts.go | Explicit process/event/config and structured control/evidence values |
| matcher.go | Pure bounded exact/edge-wildcard selection |
| executor.go | Prior activity+intent, bounded native command, sanitized artifacts and truthful outcomes |
| plugin.go | Ordinary typed lifecycle adapters, pre-intent decisions, failure hooks and reference-only context notices |
| boundary_test.go | Real ambiguous control output denial and Notify cancellation/join before run-end ownership release |
| plugin_test.go | Real helper subprocesses/SDK/SQLite, prior activity, effective-arg HITL, block/true-error separation, cancel/finalization, source redaction and limits |
