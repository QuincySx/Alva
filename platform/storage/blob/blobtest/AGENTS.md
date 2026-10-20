# Immutable blob contract testing
> Backend-neutral public acceptance helpers.
## 地位
Optional test-only support; runtime packages never import testing helpers.
## 逻辑
Run obtains a fresh caller-selected blob.Store for identity/ranges and competing concurrent publications. Memory and directory adapters invoke the identical suite.
## 约束
Factory owns cleanup. No concrete adapter, file, SQL, network or account dependency. Does not certify durability, cancellation during unknown publication, outage behavior or Close. Minimum fixture capacity: four objects, 64KiB.
## 业务域清单
| 文件 | 职责 |
|---|---|
| contract.go | Exported Run with detached bytes, cancellation, range, idempotency and concurrent immutable publication cases |
