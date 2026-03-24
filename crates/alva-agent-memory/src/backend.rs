// INPUT:  crate::error, crate::types, async_trait
// OUTPUT: MemoryBackend (trait)
// POS:    Abstract storage backend for the memory subsystem, enabling pluggable implementations.
//! Abstract storage backend trait for the memory subsystem.
//!
//! Default implementation: [`crate::sqlite::MemorySqlite`].
//! For testing, use [`crate::sqlite::MemorySqlite::open_in_memory()`].

use async_trait::async_trait;

use crate::error::MemoryError;
use crate::types::{MemoryEntry, MemoryFile};

/// Abstract storage backend for memory operations.
///
/// Separates storage concerns from the higher-level [`crate::service::MemoryService`],
/// allowing alternative implementations (PostgreSQL, in-memory mock, etc.).
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    /// Upsert a tracked file record.
    async fn upsert_file(&self, file: &MemoryFile) -> Result<(), MemoryError>;

    /// Get a tracked file by path.
    async fn get_file(&self, path: &str) -> Result<Option<MemoryFile>, MemoryError>;

    /// Insert a content chunk and return its auto-generated id.
    async fn insert_chunk(
        &self,
        path: &str,
        source: &str,
        start_line: i64,
        end_line: i64,
        hash: &str,
        text: &str,
        embedding: &[f32],
    ) -> Result<i64, MemoryError>;

    /// Delete all chunks for a given file path.
    async fn delete_chunks_for_path(&self, path: &str) -> Result<(), MemoryError>;

    /// Full-text search across memory chunks.
    async fn fts_search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<MemoryEntry>, MemoryError>;

    /// Vector similarity search against stored embeddings.
    async fn vector_search(
        &self,
        query_embedding: &[f32],
        max_results: usize,
    ) -> Result<Vec<MemoryEntry>, MemoryError>;

    /// Cache an embedding result.
    async fn cache_embedding(
        &self,
        model: &str,
        hash: &str,
        embedding: &[f32],
    ) -> Result<(), MemoryError>;
}

// ---------------------------------------------------------------------------
// Tests — validate the MemoryBackend trait contract via MemorySqlite
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::MemorySqlite;

    /// Helper: create an in-memory backend behind `dyn MemoryBackend`.
    async fn backend() -> Box<dyn MemoryBackend> {
        Box::new(MemorySqlite::open_in_memory().await.unwrap())
    }

    /// Helper: insert a prerequisite file record so foreign-key constraints pass.
    async fn seed_file(b: &dyn MemoryBackend, path: &str) {
        b.upsert_file(&MemoryFile {
            path: path.into(),
            source: "test".into(),
            hash: "h".into(),
            mtime: "2025-01-01".into(),
            size: 100,
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn trait_upsert_and_get_file() {
        let b = backend().await;

        let file = MemoryFile {
            path: "/a.rs".into(),
            source: "ws".into(),
            hash: "aaa".into(),
            mtime: "2025-01-01".into(),
            size: 42,
        };
        b.upsert_file(&file).await.unwrap();

        let fetched = b.get_file("/a.rs").await.unwrap().unwrap();
        assert_eq!(fetched.path, "/a.rs");
        assert_eq!(fetched.hash, "aaa");
        assert_eq!(fetched.size, 42);

        // Upsert again with new hash
        let updated = MemoryFile {
            hash: "bbb".into(),
            ..file
        };
        b.upsert_file(&updated).await.unwrap();
        let fetched = b.get_file("/a.rs").await.unwrap().unwrap();
        assert_eq!(fetched.hash, "bbb");
    }

    #[tokio::test]
    async fn trait_get_file_missing_returns_none() {
        let b = backend().await;
        assert!(b.get_file("/nonexistent").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn trait_insert_chunk_and_fts_search() {
        let b = backend().await;
        seed_file(b.as_ref(), "/search.md").await;

        b.insert_chunk("/search.md", "ws", 1, 10, "h1", "Rust async runtime tokio", &[])
            .await
            .unwrap();
        b.insert_chunk("/search.md", "ws", 11, 20, "h2", "Python data science pandas", &[])
            .await
            .unwrap();

        let results = b.fts_search("Rust async", 10).await.unwrap();
        assert!(!results.is_empty());
        assert!(results[0].text.contains("Rust"));
    }

    #[tokio::test]
    async fn trait_insert_chunk_and_vector_search() {
        let b = backend().await;
        seed_file(b.as_ref(), "/vec.md").await;

        b.insert_chunk("/vec.md", "ws", 1, 5, "h1", "alpha", &[1.0, 0.0, 0.0])
            .await
            .unwrap();
        b.insert_chunk("/vec.md", "ws", 6, 10, "h2", "beta", &[0.0, 1.0, 0.0])
            .await
            .unwrap();

        // Query close to alpha
        let results = b.vector_search(&[0.95, 0.05, 0.0], 10).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].text, "alpha");
    }

    #[tokio::test]
    async fn trait_delete_chunks_for_path() {
        let b = backend().await;
        seed_file(b.as_ref(), "/del.md").await;

        b.insert_chunk("/del.md", "ws", 1, 5, "h1", "deletable content", &[])
            .await
            .unwrap();

        // Verify chunk exists via FTS
        let before = b.fts_search("deletable", 10).await.unwrap();
        assert_eq!(before.len(), 1);

        b.delete_chunks_for_path("/del.md").await.unwrap();

        let after = b.fts_search("deletable", 10).await.unwrap();
        assert!(after.is_empty());
    }

    #[tokio::test]
    async fn trait_cache_embedding() {
        let b = backend().await;

        // cache_embedding should not error
        b.cache_embedding("model-v1", "hash-abc", &[0.1, 0.2, 0.3])
            .await
            .unwrap();

        // Calling again with same key should succeed (upsert behavior)
        b.cache_embedding("model-v1", "hash-abc", &[0.4, 0.5, 0.6])
            .await
            .unwrap();
    }
}
