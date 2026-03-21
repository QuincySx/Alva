# agent/memory
> Agent 记忆系统 —— FTS + 向量混合搜索

## 地位
为 Agent 提供持久化知识记忆能力，扫描工作区 MEMORY.md 文件建立索引，支持全文搜索和向量语义搜索的加权融合。

## 逻辑
`MemorySqlite` 管理四张表（memory_files、memory_chunks、chunks_fts、embedding_cache），`sync` 模块扫描工作区 MEMORY.md 并分块入库，`EmbeddingProvider` trait 计算向量嵌入，`MemoryService` 提供统一的 store/search/sync 入口。

## 约束
- FTS5 全文搜索使用 BM25 评分
- 向量搜索为暴力余弦相似度（占位），未使用向量索引
- 混合搜索加权：FTS 40% + 向量 60%

## 业务域清单
| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| mod | `mod.rs` | 声明子模块、pub use 再导出 |
| types | `types.rs` | MemoryFile、MemoryChunk、MemoryEntry、SyncReport |
| sqlite | `sqlite.rs` | MemorySqlite：DDL、文件/分块 CRUD、FTS/向量搜索、嵌入缓存 |
| embedding | `embedding.rs` | EmbeddingProvider trait、NoopEmbeddingProvider |
| sync | `sync.rs` | sync_workspace：MEMORY.md 扫描 + 分块 + 嵌入 |
| service | `service.rs` | MemoryService：统一入口、混合搜索融合 |
