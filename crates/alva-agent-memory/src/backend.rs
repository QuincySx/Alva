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

// Trait-contract tests live with the concrete impl that exercises them —
// see `alva-app-extension-memory::sqlite` tests, which hit the same API
// through MemorySqlite. alva-agent-memory is intentionally abstract and
// keeps no heavy integration tests.
