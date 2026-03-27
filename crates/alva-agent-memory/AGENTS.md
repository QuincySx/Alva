# alva-agent-memory

> FTS + vector hybrid search memory system with SQLite storage, file sync, and embedding support.

---

## 业务域清单

| 名称 | 文件/子目录 | 职责 |
|------|------------|------|
| Crate Root | src/lib.rs | Declares all modules and provides the public API via convenience re-exports |
| MemoryError | src/error.rs | Root error enum for the alva-agent-memory crate |
| Domain Types | src/types.rs | Domain types: tracked files, content chunks, search results, and sync reports |
| MemorySqlite | src/sqlite.rs | SQLite storage backend: FTS5 full-text search, brute-force vector search, embedding cache, and file/chunk CRUD |
| EmbeddingProvider | src/embedding.rs | Embedding provider trait for vector search and a no-op placeholder implementation |
| Workspace Sync | src/sync.rs | Scans workspace for MEMORY.md files, chunks content, computes embeddings, and indexes into MemorySqlite |
| MemoryBackend | src/backend.rs | Abstract storage backend trait for pluggable memory implementations |
| MemoryService | src/service.rs | Unified memory service combining FTS + vector hybrid search with weighted score fusion |
