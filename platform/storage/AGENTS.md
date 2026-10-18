# 横向支撑 · storage
> Native durable implementations, never an SDK or product-session authority merger.

## 地位
Native platform implementation consumed by harness/product assembly; SDK packages do not depend on this tree.
## 逻辑
`sqlite` owns private database opening and process locks. `session` owns native typed execution facts and session-scoped artifact bytes. Hub product facts use a separate database and their own store.
## 约束
No legacy import, environment credential discovery, global hash-based artifact reads, or automatic tool replay. A read-only inspection does not construct an Agent or restore runtime state. OS-specific security gaps remain explicit.
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| SQLite host | sqlite/ | Private files, explicit single-connection configuration and exclusive owner lock |
| Native sessions | session/ | Atomic typed facts, bounded metadata/text reads, recovery and owned artifact bytes |
| Persistent memory | memory/ | Opt-in namespaced SQLite FTS5/vector backend, not a second transcript |
