# CI infrastructure
> Repository verification, not product runtime or account automation.

## 地位
Local workflow definitions. Adding a workflow is not evidence that remote CI ran.
## 逻辑
Existing Rust gates stay independent while Go migration gains its own test/build gates. Go's module root stays go/; commands live in apps/cmd, portable packages in sdk, and concrete integration/platform support stays outside that SDK.
## 约束
Read-only repository permissions; no model accounts, user CLI credentials, live MCP, automatic releases or publishing. Pin external actions to verified official release commits. Never disable module checksum validation.
## 业务域清单
| 目录 | 职责 |
|---|---|
| workflows | Rust/Go verification pipelines |
