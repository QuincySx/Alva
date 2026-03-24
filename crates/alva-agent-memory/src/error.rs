// INPUT:  thiserror, std::io
// OUTPUT: MemoryError
// POS:    Root error enum for the alva-memory crate.
//! Error types for the memory subsystem.

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("embedding error: {0}")]
    Embedding(String),
    #[error("sync error: {0}")]
    Sync(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
