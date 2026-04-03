// INPUT:  error, types, sqlite, embedding, sync, service
// OUTPUT: MemoryError, MemoryService, MemoryEntry, MemoryChunk, MemoryFile, SyncReport, EmbeddingProvider, MemorySqlite
// POS:    Crate root — declares all modules and provides the public API via convenience re-exports.
//! Agent memory — FTS + vector hybrid search, file sync, embedding support.

pub mod backend;
pub mod error;
pub mod types;
pub mod sqlite;
pub mod embedding;
pub mod hash;
pub mod sync;
pub mod service;
pub mod extract;

pub use backend::MemoryBackend;
pub use error::MemoryError;
pub use service::MemoryService;
pub use types::{MemoryEntry, MemoryChunk, MemoryFile, SyncReport};
pub use embedding::{EmbeddingProvider, NoopEmbeddingProvider};
pub use sqlite::MemorySqlite;
pub use sync::SyncConfig;
pub use extract::{ExtractionConfig, ExtractedMemory, MemoryType};
