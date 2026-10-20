# CI infrastructure
> Verification for the maintained Go implementation.

## 地位
Repository CI declarations, not product runtime or account automation.
## 逻辑
The repository root is the Go module root. Workflows validate the Go SDK, actual ownership fixtures, architecture, portable builds and native commands. Rust CI has been archived with the retired implementation.
## 约束
Read-only repository permissions; no accounts, credentials, live MCP, automatic releases or publishing. External actions remain pinned. Workflow declarations do not prove remote execution.
## 业务域清单
| 目录 | 职责 |
|---|---|
| workflows | Go source/runtime/build gates, including actual portable-tool/durable/telemetry/blob-memory Node/WASIp1 execution, external SDK modules and reusable storage conformance |
