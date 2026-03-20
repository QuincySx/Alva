//! Agent memory — FTS + vector hybrid search, file sync, embedding support.

pub mod types;
pub mod sqlite;
pub mod embedding;
pub mod sync;
pub mod service;

pub use service::MemoryService;
pub use types::{MemoryEntry, MemoryChunk, MemoryFile, SyncReport};
