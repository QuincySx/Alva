# L7 · Devtools MCP command
> Explicit Hub/native UI diagnostics over stdio with separately granted credentials; read-only by default.

## 地位
Thin process/secret lifetime owner over apps/hub/devtools or independent apps/debug; no model, Agent or storage assembly.

## 逻辑
Exactly one of --hub/--ui selects a canonical numeric-loopback address. Exactly one of --token-fd3..10 (Unix ABI) or --token-file selects an explicit credential. The file must already exist, be private/current-owner, regular and not symlink/hardlinked; it is opened read-only and never discovered/created. Read a bounded token before MCP starts; stdin/stdout remain exclusively JSON-RPC. Owner cancellation closes the real stdio connection and joins requests. UI mode exposes ui_inspect/ui_capture by default; --ui-controls adds declared action/log-level/diagnostic-shutdown tools, while the actual UI server separately enforces controls/execution opt-in and ordinary product permissions.

## 约束
No token body in argv/environment/URL/output, file-path discovery or credentials/config migration. Hub mode remains read-only. UI Send only uses the actual declared product callback with independent server execution grant, never a second Agent path. Token descriptor must be a pipe/regular file, never terminal/socket. Errors are generic and never echo protocol frames/private causes. Protocol supports the pinned official SDK; no old Rust debug protocol bridge.

## 业务域清单
| 文件 | 职责 |
|---|---|
| main.go | Explicit Hub/UI and fd/private-file parsing, cancellable bounded secret read and owned stdio MCP lifecycle |
| main_test.go | Actual child fd grant and portable private-file UI stdio, protocol-only stdout, malformed secret and cancellation regressions |
