// INPUT:  error, types, embedding, hash, backend, service
// OUTPUT: MemoryBackend, MemoryError, MemoryService, MemoryEntry, MemoryChunk, MemoryFile, SyncReport, EmbeddingProvider, NoopEmbeddingProvider
// POS:    Crate root — lean memory trait + types. Heavy impls (sqlite, workspace sync, extract) moved to alva-app-extension-memory.
//! Agent memory — lean abstraction layer.
//!
//! This crate defines the `MemoryBackend` trait, the `MemoryService`
//! facade, and the shared value types. Heavy native-only
//! implementations (SQLite storage, workspace MEMORY.md sync, memory
//! fact extraction) live in `alva-app-extension-memory` so this crate
//! stays minimal and wasm32-clean.
//!
//! Wasm consumers should implement `MemoryBackend` themselves over
//! IndexedDB / localStorage / server APIs.

pub mod backend;
pub mod embedding;
pub mod error;
pub mod hash;
pub mod service;
pub mod types;

pub use backend::MemoryBackend;
pub use embedding::{EmbeddingProvider, NoopEmbeddingProvider};
pub use error::MemoryError;
pub use service::MemoryService;
pub use types::{MemoryChunk, MemoryEntry, MemoryFile, SyncReport};
