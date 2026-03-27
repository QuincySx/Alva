// INPUT:  crate::agent::persistence
// OUTPUT: pub mod memory, pub use SqliteStorage
// POS:    Module declaration for storage adapters and SQLite re-export.
pub mod memory;

// Re-export SQLite storage from the persistence module for convenience.
#[allow(unused_imports)]
pub use crate::agent::persistence::SqliteStorage;
