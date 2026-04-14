// INPUT:  sqlite (rusqlite), sync (walkdir + tokio::fs), extract (tokio::fs) — all native-only
// OUTPUT: MemorySqlite, sync_workspace, ExtractionConfig / ExtractedMemory / MemoryType
// POS:    Crate root — heavy native memory impls extracted from alva-agent-memory so the trait crate stays wasm32-clean.

//! `alva-app-extension-memory` — native-only memory backends and
//! workspace integration.
//!
//! Extracted from `alva-agent-memory` so the agent layer keeps only
//! the `MemoryBackend` trait + lean fallback, per the architectural
//! rule that heavy domain crates belong at the app extension layer.
//!
//! Contents (native only):
//! - `sqlite::MemorySqlite` — rusqlite/FTS5 + vector backend
//! - `sync::sync_workspace` — walks `MEMORY.md` files under a
//!   workspace root and indexes them into the store
//! - `extract::*` — memory-facts extraction helpers backed by
//!   `tokio::fs`
//!
//! On wasm32 the entire crate compiles to an empty library. Wasm apps
//! that need memory should implement `alva_agent_memory::MemoryBackend`
//! over IndexedDB (via `web_sys` or `indexed_db_futures`) inside their
//! own code — neither this crate nor the agent-memory crate will
//! provide a wasm implementation.

#[cfg(not(target_family = "wasm"))]
pub mod extract;
#[cfg(not(target_family = "wasm"))]
pub mod sqlite;
#[cfg(not(target_family = "wasm"))]
pub mod sync;

#[cfg(not(target_family = "wasm"))]
pub use extract::{ExtractedMemory, ExtractionConfig, MemoryType};
#[cfg(not(target_family = "wasm"))]
pub use sqlite::MemorySqlite;
#[cfg(not(target_family = "wasm"))]
pub use sync::{sync_workspace, SyncConfig};
