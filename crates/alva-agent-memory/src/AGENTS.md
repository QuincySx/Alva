# alva-agent-memory/src
> FTS + vector 混合搜索记忆系统的源码实现层

## 地位
`alva-agent-memory` crate 的全部源码。对外通过 `lib.rs` 的 re-exports 提供 MemoryService、MemoryBackend trait、MemorySqlite、EmbeddingProvider 等公共 API。被 `alva-agent-runtime` 在 native feature 下集成。

## 逻辑
1. `types.rs` 定义领域模型（MemoryFile / MemoryChunk / MemoryEntry / SyncReport）。
2. `backend.rs` 声明 `MemoryBackend` trait，抽象存储后端的 CRUD + 搜索接口。
3. `sqlite.rs` 实现 `MemorySqlite`：基于 FTS5 全文搜索 + 暴力向量搜索 + embedding 缓存。
4. `embedding.rs` 定义 `EmbeddingProvider` trait 及 `NoopEmbeddingProvider` 占位实现。
5. `sync.rs` 扫描工作区 MEMORY.md 文件，分块、计算 embedding、写入 SQLite。
6. `service.rs` 组合以上模块，提供 `MemoryService` 统一入口：CRUD + 加权分数融合的混合搜索。
7. `error.rs` 定义 `MemoryError` 统一错误枚举。

## 约束
- SQLite 后端使用 `tokio_rusqlite` 异步适配，不可在同步上下文直接调用。
- `EmbeddingProvider` 生产环境须接入 OpenAI 兼容的 `/embeddings` 端点，`NoopEmbeddingProvider` 仅供测试。
- `sync_workspace` 仅扫描 `MEMORY.md` 文件，非此文件名的内容不会被索引。
- `MemoryBackend` trait 使用 `async_trait`，实现时须满足 `Send + Sync`。

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate Root | lib.rs | 声明所有模块并通过 re-exports 提供公共 API |
| MemoryBackend | backend.rs | 抽象存储后端 trait，支持可插拔实现 |
| EmbeddingProvider | embedding.rs | Embedding 提供者 trait 及 NoopEmbeddingProvider 占位实现 |
| MemoryError | error.rs | 统一错误枚举（Storage / Embedding / Sync / IO） |
| MemoryService | service.rs | 统一入口：CRUD + FTS/向量加权混合搜索 |
| MemorySqlite | sqlite.rs | SQLite 存储后端：FTS5 全文搜索、暴力向量搜索、embedding 缓存、文件/块 CRUD |
| sync | sync.rs | 工作区文件同步：扫描 MEMORY.md、分块、embedding、写入存储 |
| Domain Types | types.rs | 领域类型：MemoryFile / MemoryChunk / MemoryEntry / SyncReport |
