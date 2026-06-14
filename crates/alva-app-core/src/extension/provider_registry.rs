// INPUT:  std::sync::Arc, async_trait, alva_kernel_abi::ProviderRegistry, crate::extension::{Extension, ExtensionContext}
// OUTPUT: ProviderRegistryExtension
// POS:    Opt-in Extension that provides an `Arc<ProviderRegistry>` onto the bus so `AgentSpawnTool` can resolve per-spawn `model: "provider/id"` specs. Without this extension, the `model` field on `SpawnInput` must be left empty (child inherits the parent's model).

//! `ProviderRegistryExtension` — publishes a user-supplied
//! `ProviderRegistry` on the bus.
//!
//! `AgentSpawnTool` looks up `ProviderRegistry` on the bus when a
//! sub-agent spawn includes a `model: "provider/id"` override. This
//! extension is the only supported way to enable that override: there is
//! no builder setter and no default registration.
//!
//! ```rust,ignore
//! BaseAgent::builder()
//!     .workspace(path)
//!     .extension(Box::new(ProviderRegistryExtension::new(registry)))
//!     .build(model).await?;
//! ```
use std::sync::Arc;

use async_trait::async_trait;

use alva_kernel_abi::ProviderRegistry;

use alva_agent_core::extension::{Plugin, Registrar};

/// Provides an `Arc<ProviderRegistry>` to the bus for dynamic per-spawn
/// model resolution.
pub struct ProviderRegistryExtension {
    registry: Arc<ProviderRegistry>,
}

impl ProviderRegistryExtension {
    /// Wrap a caller-supplied `ProviderRegistry`.
    pub fn new(registry: Arc<ProviderRegistry>) -> Self {
        Self { registry }
    }
}

#[async_trait]
impl Plugin for ProviderRegistryExtension {
    fn name(&self) -> &str {
        "provider-registry"
    }

    fn description(&self) -> &str {
        "Provides a ProviderRegistry to the bus so sub-agents can specify \
         model via SpawnInput.model"
    }

    async fn register(&self, r: &Registrar) {
        r.provide::<ProviderRegistry>(self.registry.clone());
    }
}
