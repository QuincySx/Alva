# 横向支撑 · Persistent Memory
> Explicit native SQLite implementation of the independent memory capability.

## 地位
Heavy native backend consumed by a same-name replacement memory Plugin; never the default backend or native session transcript authority.
## 逻辑
One private locked memory.sqlite owns files/chunks, synchronized FTS5/BM25 index and finite float32 embedding cache. Trusted product assembly binds opaque namespaces; every operation filters that fixed namespace. Vector search streams a bounded brute-force scan and retains only top candidates. No automatic embeddings account/network.
## 约束
No old SQLite/config import. Bind is not a UI-supplied authorization claim. Optional FileReplacer atomically commits complete file/chunks/FTS/hash, and SDK Service consumes it; independent CRUD calls are not falsely described as one transaction. Chunk text ≤1 MiB, atomic replacement ≤2048 chunks/20 MiB, vectors ≤4096 finite dimensions, results ≤200/4 MiB, vector scan ≤10000 matching chunks/64 MiB. Budget failure is explicit, not a partial successful ranking. No host files are opened by CRUD and no workspace scanning happens automatically. Same private permissions/platform limitations as platform/storage/sqlite apply.
Store实现共享storage.MemoryStore；Bind只返回sdk/agent/memory.Backend，具体缓存/原子replace能力仍由实现提供。消费者不导入本包来命名句柄或选择SQL。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|---|---|---|
| Store | store.go | Private schema, lifecycle, namespace binding, file/chunk/cache persistence |
| Search | search.go | Real FTS5/BM25, bounded vector cosine and verified float encoding |
| Atomic file checkpoint | replace.go | Complete replacement with typed capability and rollback before hash advancement |
| Regressions | store_test.go | FTS5/vector/cache restart, namespace separation and explicit malformed/budget failures |
