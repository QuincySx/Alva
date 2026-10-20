# 横向支撑 · AEP
> Third-party Plugin JSON-RPC wire and owned bidirectional I/O.

## 地位
Protocol implementation outside the SDK. Product Plugin/bundle/approval/journal policy stays above it.
## 逻辑
Explicit identity and manifest/host grant ceiling precede initialize. Stdio workers service serialized host requests and eight bounded reverse callback workers without callback deadlock. Full frames reach an optional fail-closed observer.
Healthy owner Shutdown closes new Call/Notify admission, requests shutdown with a two-second grace bounded by caller context, drains both streams including final diagnostics, waits actual leader exit and then reaps the group. Missing acknowledgement, hanging exit, nonzero exit or observer failure remains an error and forces owned cleanup; Close remains immediate for cancellation/failure. EOF is accepted only after the exact successful shutdown response, never as independent success evidence.
## 约束
No global discovery/credential inheritance/implicit program install. Native execution uses platform/system/process; an explicit Config.Start may supply an owned Peer instead, mutually exclusive with Process and never a fallback. Factories settle partial resources before returning errors. A non-nil Open result with error retains incomplete cleanup ownership for Close retry. 16 MiB/frame, 8 MiB tool metadata, 1024 tools, 128 callback queue/per-request and 4096 callbacks/client lifetime. Stderr is framed across arbitrary Read/UTF8 boundaries as complete original-byte lines (1MiB cap) or explicit EOF partial; over-budget prefix is labelled truncated and stops execution. Product owns stream-envelope redaction. Cancellation/observer/transport failure closes the exact owned peer without replay. Handlers honor context; Close preserves even late observer errors, while CleanupComplete separately proves worker and execution cleanup. API is not a secure bundle loader or approved Plugin by itself.
## 业务域清单
| 文件 | 职责 |
|---|---|
| types.go | Version, handshake, tool/action, explicit grants, observer/handler与owned Peer/Config.Start |
| messages.go / messages_test.go | 外部AEP type-tagged DTO与内部wire的真实协议映射；不接受kind/旧别名，保留工具与显式媒体/opaque状态 |
| client.go | Actual owned stdio handshake, requests, callbacks and close |
| client_test.go | Real local subprocess callback, complete output, grant/cancel/observer regressions |
| transport_test.go | Exclusive execution selection、真实握手/join、typed-nil与失败后owner保留 |
| python/ | Optional Python author SDK for this Go protocol; real loader interoperability, explicit local import, no Rust dependency |
| javascript/ | 无npm运行时依赖的Node作者SDK、TypeScript声明、完整bundle示例和真实runtime测试 |
