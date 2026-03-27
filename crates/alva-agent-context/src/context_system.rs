// INPUT:  std::sync::Arc, crate::plugin::ContextHooks, crate::sdk::ContextHandle, crate::session::SessionAccess
// OUTPUT: pub struct ContextSystem
// POS:    Bundles ContextHooks + ContextHandle + SessionAccess into a single composite, preventing mismatched configuration.
//! ContextSystem — the unified context management entry point.
//!
//! Instead of managing three separate fields (hooks, handle, session),
//! consumers hold a single `ContextSystem` that ensures the trio is
//! always consistent.

use std::sync::Arc;

use crate::plugin::ContextHooks;
use crate::sdk::ContextHandle;
use crate::session::SessionAccess;

/// Bundles the context management trio: hooks (strategy), handle (operations), session (persistence).
///
/// This prevents the mismatch bug where someone swaps `context_hooks` but forgets
/// to update `context_handle`, or sets a session on the wrong agent.
pub struct ContextSystem {
    hooks: Arc<dyn ContextHooks>,
    handle: Arc<dyn ContextHandle>,
    session: Option<Arc<dyn SessionAccess>>,
}

impl ContextSystem {
    pub fn new(
        hooks: Arc<dyn ContextHooks>,
        handle: Arc<dyn ContextHandle>,
    ) -> Self {
        Self {
            hooks,
            handle,
            session: None,
        }
    }

    pub fn with_session(mut self, session: Arc<dyn SessionAccess>) -> Self {
        self.session = Some(session);
        self
    }

    pub fn hooks(&self) -> &dyn ContextHooks {
        self.hooks.as_ref()
    }

    pub fn handle(&self) -> &dyn ContextHandle {
        self.handle.as_ref()
    }

    pub fn session(&self) -> Option<&dyn SessionAccess> {
        self.session.as_deref()
    }

    /// Replace hooks (e.g., swap in a ContextHooksChain).
    pub fn set_hooks(&mut self, hooks: Arc<dyn ContextHooks>) {
        self.hooks = hooks;
    }

    /// Replace session.
    pub fn set_session(&mut self, session: Arc<dyn SessionAccess>) {
        self.session = Some(session);
    }
}

/// Create a default ContextSystem with RulesContextHooks + in-memory store.
impl Default for ContextSystem {
    fn default() -> Self {
        let store = Arc::new(std::sync::Mutex::new(
            crate::store::ContextStore::new(200_000, 180_000),
        ));
        let handle: Arc<dyn ContextHandle> = Arc::new(
            crate::sdk_impl::ContextHandleImpl::new(store),
        );
        let hooks: Arc<dyn ContextHooks> = Arc::new(
            crate::rules_plugin::RulesContextHooks::default(),
        );
        Self::new(hooks, handle)
    }
}
