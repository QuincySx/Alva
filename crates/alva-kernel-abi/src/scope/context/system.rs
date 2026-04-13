// POS: ContextSystem container — bundles hooks, handle, and session.

use std::sync::Arc;

use super::{ContextHandle, ContextHooks, SessionAccess};

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
