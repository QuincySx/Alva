// POS: ContextSystem container — bundles context hooks and handle only.

use std::sync::Arc;

use super::{ContextHandle, ContextHooks};

/// Bundles context hooks and handle. Durable state is wired through the
/// authoritative AgentSession owned by AgentState.
pub struct ContextSystem {
    hooks: Arc<dyn ContextHooks>,
    handle: Arc<dyn ContextHandle>,
}

impl ContextSystem {
    pub fn new(hooks: Arc<dyn ContextHooks>, handle: Arc<dyn ContextHandle>) -> Self {
        Self { hooks, handle }
    }

    pub fn hooks(&self) -> &dyn ContextHooks {
        self.hooks.as_ref()
    }

    pub fn handle(&self) -> &dyn ContextHandle {
        self.handle.as_ref()
    }

    /// Replace hooks (e.g., swap in a ContextHooksChain).
    pub fn set_hooks(&mut self, hooks: Arc<dyn ContextHooks>) {
        self.hooks = hooks;
    }
}
