//! Domain types for the memory subsystem.

use serde::{Deserialize, Serialize};

/// A tracked file in the memory store.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFile {
    pub path: String,
    pub source: String,
    pub hash: String,
    pub mtime: String,
    pub size: i64,
}

/// A chunk of content derived from a memory file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChunk {
    pub id: i64,
    pub path: String,
    pub source: String,
    pub start_line: i64,
    pub end_line: i64,
    pub hash: String,
    pub text: String,
    /// Embedding vector (empty if not yet computed).
    pub embedding: Vec<f32>,
}

/// A search result from the memory system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub chunk_id: i64,
    pub path: String,
    pub text: String,
    pub score: f64,
    pub start_line: i64,
    pub end_line: i64,
}

/// Report returned after a workspace sync operation.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncReport {
    pub files_scanned: usize,
    pub files_added: usize,
    pub files_updated: usize,
    pub files_unchanged: usize,
    pub chunks_created: usize,
}
