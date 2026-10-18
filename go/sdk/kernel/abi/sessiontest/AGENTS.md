# SDK session backend conformance
> Reusable contract tests for backend authors, with no concrete storage dependency.

## 地位
Test support alongside the L1 session contract; runtime code never imports testing helpers.
## 逻辑
Run creates a fresh PersistentSession through the caller's factory for each case. Memory and native SQLite invoke the same suite.
## 约束
Checks atomic rejection, identity, ownership, protocol matching, exclusive cursors and cancellation. Does not assert disk durability, close behavior, resource limits or a backend's declared redaction policy. Factory owns cleanup; small noncredential fixtures have identical semantics across implementations.
## 业务域清单
| 文件 | 职责 |
|---|---|
| contract.go | Exported Run conformance entry and shared backend cases |
