# 横向支撑 · storage
> Narrow byte ports and native durable implementations; never an SDK or product-session authority merger.

## 地位
Platform storage consumed by harness/product assembly; SDK packages do not depend on this tree. Pure blob contracts and memory adapters have no native dependencies.
## 逻辑
`sqlite` owns private database opening and process locks. `session` owns native typed execution facts and artifact authorization/hash metadata. Artifact bytes can remain inline or use an explicit `blob.Store`; typed fact JSON remains in the atomic fact transaction. Hub product facts use a separate database and their own consumer-defined stores.
## 约束
No legacy import, environment credential discovery, global hash-based artifact reads, or automatic tool replay. A read-only inspection does not construct an Agent or restore runtime state. OS-specific security gaps remain explicit.
Cloud bytes do not imply atomic append/CAS; immutable upload precedes metadata commit and uncertain/orphan objects are never automatically retried/deleted. Host owns adapter lifecycle and declares persistent versus process-lifetime retention. Storage mounts grant no local execution/filesystem rights.
## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| SQLite host | sqlite/ | Private files, explicit single-connection configuration and exclusive owner lock |
| Immutable bytes | blob/ | Two-method portable port with directory and bounded volatile memory adapters; no SQL, SDK or runtime policy |
| Native sessions | session/ | Atomic typed facts, fixed-water bounded conversation primitives (no model selection policy), recovery and owned artifact bytes |
| Persistent memory | memory/ | Opt-in namespaced SQLite FTS5/vector backend, not a second transcript |
