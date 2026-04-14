//! Default `MemoryExtension` wrapping an in-memory `MemoryService`.
//!
//! Ships an `InMemoryBackend`-backed `MemoryService` so that `BaseAgent`
//! gets memory by default with zero external dependencies. Users who
//! want a different backend (e.g. `MemorySqlite`, Postgres, Redis)
//! should register their own extension with `name() == "memory"`, which
//! transparently replaces this default via the builder's name-based
//! dedup.

use std::sync::Arc;

use alva_agent_core::extension::{Extension, ExtensionContext};
use alva_agent_memory::{InMemoryBackend, MemoryService, NoopEmbeddingProvider};
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

/// The extension that provides a `MemoryService` on the agent bus.
///
/// Publishes `Arc<MemoryService>` as a bus capability during `configure()`
/// so other extensions / middleware / the outer harness can pick it up via
/// `bus.get::<MemoryService>()`.
pub struct MemoryExtension {
    service: Arc<MemoryService>,
}

impl MemoryExtension {
    /// Wrap a caller-supplied `MemoryService`.
    pub fn new(service: MemoryService) -> Self {
        Self {
            service: Arc::new(service),
        }
    }

    /// Access the wrapped service.
    pub fn service(&self) -> &Arc<MemoryService> {
        &self.service
    }
}

impl Default for MemoryExtension {
    fn default() -> Self {
        let backend = Arc::new(InMemoryBackend::new());
        let embedder = Box::new(NoopEmbeddingProvider::new());
        Self::new(MemoryService::with_backend(backend, embedder))
    }
}

#[async_trait]
impl Extension for MemoryExtension {
    fn name(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Memory service (default: in-memory)"
    }

    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        Vec::new()
    }

    async fn configure(&self, ctx: &ExtensionContext) {
        // Register the MemoryService handle on the bus so downstream
        // components can grab it via `bus.get::<MemoryService>()`.
        ctx.bus_writer.provide::<MemoryService>(self.service.clone());
    }
}
