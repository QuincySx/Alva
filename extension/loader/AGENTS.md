# L4 · External Plugin loader
> Explicit third-party snapshots, real AEP Plugin hooks and a reference-first capability gateway.

## 地位
Heavy native extension above SDK. AEP wire is independently owned in integration/protocol/aep.
## 逻辑
Read/validate explicit alva.toml, pin source descriptors, copy only bounded regular files into an owner-only random snapshot, verify expected manifest and optional full digest before any execution. OpenPlugin requires explicit host approval, actual native authority and a successful pre-launch product-risk callback; its snapshot is not an OS sandbox. Seven real hooks use pre-intent arguments/effective scheduling and expiring read-only event handles. One gateway searches small catalog metadata and calls selected remote tools; schemas and results stay references. It verifies the exact registered Tool/name/nonempty call before approval, caps the entire input at 256KiB, and rejects ambiguous JSON. Run cancellation and explicit Close reach approval; the dispatch gate joins it before snapshot cleanup.
The pre-intent hook receives ToolPreparationContext and forwards only its actual Runtime to AEP event handling; author argument updates do not re-resolve a name against the live registry or change the invocation's selected tool.
runtime=javascript/python selects an explicit native interpreter. runtime=wasm and a .wasm entry selects platform/sandbox/plugin with no interpreter/args/env. Both use the same AEP client, hooks, host grants, Guard, journal and references; WASIp1 receives no preopens/env/network. Every non-nil OpenPlugin result owns the bundle, including incomplete-cleanup errors, and remains with the caller until Close actually settles it. Nil leaves bundle ownership with the caller.
## 约束
No global scan, installation, ambient credentials or automatic startup. 64 KiB manifest, 8 MiB/file, 16 MiB/bundle, 256 files/512 entries/depth16. Symlink/special-file/path escapes reject. Go digest format is domain-separated canonical JSON v1, no historical Rust-pin converter or implicit old-data import. Journal is full canonical-redacted I/O in bounded 64MiB/4096-record memory and the same native SQLite authority; budget/record/timeout/cancel errors stop without reconnect/replay. Invalid author actions are journaled and ignored, never applied. Native plugin code has host access; process approval is not per-tool approval. Close reaps the child before removing its exact generated snapshot; no user source is removed. Product next-turn settings/enablement are wired. Windows protected-DACL staging is implemented; actual platform acceptance is recorded separately. host/memory.* and request_approval are not offered (same as the former real host); log/notification/metric calls are durable facts, not a claim of full UI notification delivery or external telemetry routing.
存储只消费本包按需组合的共享storage能力插口；会话/引用/DTO/错误从同一契约返回，不导入具体session.Repository或SQL实现。后端由上层程序/插件装配选择，组件不回落默认库、不downcast实现，不持有存储Close。

## 业务域清单
| 文件 | 职责 |
|---|---|
| manifest.go | Full TOML parser, explicit JS/Python/WASIp1 runtime/entry/capability constraints |
| bundle.go | Bounded snapshot, source change detection, digest and owned cleanup |
| owner_unix.go | Real current-user UID check |
| owner_windows.go | Pinned parent SameFile复核与protected current-user-only DACL；复制文件继承已校验私有父目录权限 |
| owner_other.go | Explicit platform gap, not fake chmod privacy |
| bundle_test.go | Real copied snapshot/pin/cleanup and unsafe-source regressions |
| plugin.go | Native/embedded approved launch、同一Plugin装配与partial-error owner；CleanupComplete separates physical cleanup from retained errors |
| events.go | Seven hooks/action validation, effective args and output mutation |
| host.go | Expiring native handles, bounded message/reference reads and durable host facts |
| gateway.go | One progressive catalog/call Tool with real Guard and reference-only results |
| journal.go | Full redacted in-memory/durable I/O with hard budgets and small receipts |
| boundary_test.go | Actual dispatched identity, total input bound and Close cancellation/join of approval |
| plugin_test.go | Real helper + Go SDK + Python/JS/WASM作者实际hooks与SQLite effective args、过期句柄/替换/取消/引用/risk |
| portable_test.go | JS/WASM实际Tool成功/失败、进入后取消/物理join、无ambient环境/宿主文件与拒绝runtime混用 |
| testdata/ | 明确Python/JavaScript作者SDK fixture，无用户代码或全局配置 |

Process journal sanitization includes immutable explicit configured Env values (unlabelled/JSON-escaped/multiline echoes), scoped to the exact MCP server or AEP plugin; no ambient environment discovery. Truncated captured bodies are omitted rather than retaining partial credentials.
