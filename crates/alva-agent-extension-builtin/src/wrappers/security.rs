//! Default `SecurityExtension` wrapping `SecurityMiddleware::for_workspace`.
//!
//! Ships the standard sandbox security middleware so `BaseAgent` is locked
//! down by default. Users who want different security semantics should
//! register their own extension with `name() == "security"`, which
//! replaces this default via the builder's name-based dedup.

use std::path::PathBuf;
use std::sync::Arc;

use alva_agent_core::extension::{Plugin, Registrar};
use alva_agent_security::{
    SandboxMode, SecurityGuard, SecurityMiddleware, SecurityModeControl,
};
use async_trait::async_trait;
use tokio::sync::Mutex;

/// The plugin that wires the harness-default `SecurityMiddleware` into
/// the agent stack.
///
/// During `register()` it constructs the middleware from the workspace +
/// sandbox mode, registers it with the host, and publishes the underlying
/// `Arc<Mutex<SecurityGuard>>` and `SecurityModeControl` on the bus so the
/// outer harness (e.g. `BaseAgent::resolve_permission`) can resolve
/// interactive permission prompts.
pub struct SecurityExtension {
    workspace: PathBuf,
    sandbox_mode: SandboxMode,
}

impl SecurityExtension {
    /// Build a SecurityExtension for the given workspace + sandbox mode.
    pub fn for_workspace(workspace: impl Into<PathBuf>, sandbox_mode: SandboxMode) -> Self {
        Self {
            workspace: workspace.into(),
            sandbox_mode,
        }
    }
}

#[async_trait]
impl Plugin for SecurityExtension {
    fn name(&self) -> &str {
        "security"
    }

    fn description(&self) -> &str {
        "Sandbox security middleware (default)"
    }

    async fn register(&self, r: &Registrar) {
        let mw = SecurityMiddleware::for_workspace(&self.workspace, self.sandbox_mode.clone());
        // Obtain the guard handle before moving `mw` into the Arc.
        let guard = mw.guard();
        r.middleware(Arc::new(mw));

        // Publish the lock-free mode control so PermissionModeService can flip
        // the security mode without holding the guard's mutex.
        let mode_handle = guard.lock().await.mode_handle();
        r.provide::<dyn SecurityModeControl>(mode_handle);
        // Publish the SecurityGuard handle on the bus so external callers
        // (e.g. CLI/UI permission resolvers) can find it without going
        // through a hardcoded BaseAgent accessor.
        r.provide::<Mutex<SecurityGuard>>(guard);
    }
}
