// INPUT:  (none)
// OUTPUT: pub mod error, types, repository, loader, injector, store, fs
// POS:    Root module for the alva-protocol-skill crate.
//         Provides Skill discovery, loading, injection, and in-memory store.

pub mod error;
pub mod types;
pub mod repository;
pub mod loader;
pub mod injector;
pub mod store;
// fs module uses tokio::fs + walkdir — gated to non-wasm as well as the
// `fs` feature. Wasm callers should implement skill loading over their
// own storage (fetch + browser cache, etc.).
#[cfg(all(feature = "fs", not(target_family = "wasm")))]
pub mod fs;
pub mod memory;
