// INPUT:  std::sync::Arc, async_trait, alva_kernel_abi::ToolLockRegistry, crate::extension::{Extension, ExtensionContext}
// OUTPUT: ToolLockRegistryExtension
// POS:    Publishes a single shared `ToolLockRegistry` on the bus so sub-agent parallel spawns contend on one lock map.

//! Publishes a shared `ToolLockRegistry` onto the bus.
//!
//! Without this extension, each agent builds its own lock state — meaning
//! two sub-agents editing the same file concurrently don't see each other's
//! writes. With it, all agents (main + sub-agents) share one registry and
//! serialize correctly on conflicting resource keys.
//!
//! This extension is opt-in but essentially free to install: the registry
//! holds only a lazily populated HashMap of per-key `RwLock`s and a single
//! global `RwLock`. No background tasks, no periodic work.

use std::sync::Arc;

use async_trait::async_trait;

use alva_kernel_abi::ToolLockRegistry;

use crate::extension::{Extension, ExtensionContext};

/// Extension that provides a shared [`ToolLockRegistry`] on the bus.
pub struct ToolLockRegistryExtension {
    registry: Arc<ToolLockRegistry>,
}

impl ToolLockRegistryExtension {
    /// Fresh empty registry.
    pub fn new() -> Self {
        Self {
            registry: Arc::new(ToolLockRegistry::new()),
        }
    }

    /// Share an existing registry (useful when the outer host wants to
    /// observe the same lock map as the agent loop).
    pub fn with_registry(registry: Arc<ToolLockRegistry>) -> Self {
        Self { registry }
    }
}

impl Default for ToolLockRegistryExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for ToolLockRegistryExtension {
    fn name(&self) -> &str {
        "tool-lock-registry"
    }

    fn description(&self) -> &str {
        "Provides a shared ToolLockRegistry on the bus so the agent loop and \
         sub-agents serialize correctly on conflicting resource keys."
    }

    async fn configure(&self, ctx: &ExtensionContext) {
        ctx.bus_writer.provide::<ToolLockRegistry>(self.registry.clone());
    }
}
