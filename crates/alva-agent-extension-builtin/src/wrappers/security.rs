//! Default `SecurityExtension` wrapping `SecurityMiddleware::for_workspace`.
//!
//! Ships the standard sandbox security middleware so `BaseAgent` is locked
//! down by default. Users who want different security semantics should
//! register their own extension with `name() == "security"`, which
//! replaces this default via the builder's name-based dedup.

use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use alva_agent_core::extension::{Extension, ExtensionContext, HostAPI};
use alva_agent_security::{SandboxMode, SecurityGuard, SecurityMiddleware};
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;
use tokio::sync::Mutex;

/// The extension that wires the harness-default `SecurityMiddleware` into
/// the agent stack.
///
/// - During `activate()` it constructs the middleware from the workspace +
///   sandbox mode and registers it with the host. The middleware itself
///   later picks up the bus during its own `configure()` lifecycle.
/// - During `configure()` it publishes the underlying `Arc<Mutex<SecurityGuard>>`
///   on the bus so the outer harness (e.g. `BaseAgent::resolve_permission`)
///   can resolve interactive permission prompts.
pub struct SecurityExtension {
    workspace: PathBuf,
    sandbox_mode: SandboxMode,
    /// Built lazily during `activate()`. Wrapped in a `Mutex` because the
    /// `Extension` trait takes `&self` everywhere and we need to stash the
    /// guard handle for `configure()` to publish.
    guard: StdMutex<Option<Arc<Mutex<SecurityGuard>>>>,
}

impl SecurityExtension {
    /// Build a SecurityExtension for the given workspace + sandbox mode.
    pub fn for_workspace(workspace: impl Into<PathBuf>, sandbox_mode: SandboxMode) -> Self {
        Self {
            workspace: workspace.into(),
            sandbox_mode,
            guard: StdMutex::new(None),
        }
    }
}

#[async_trait]
impl Extension for SecurityExtension {
    fn name(&self) -> &str {
        "security"
    }

    fn description(&self) -> &str {
        "Sandbox security middleware (default)"
    }

    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        Vec::new()
    }

    fn activate(&self, api: &HostAPI) {
        let mw = SecurityMiddleware::for_workspace(&self.workspace, self.sandbox_mode.clone());
        // Stash the guard handle so configure() can publish it.
        if let Ok(mut slot) = self.guard.lock() {
            *slot = Some(mw.guard());
        }
        api.middleware(Arc::new(mw));
    }

    async fn configure(&self, ctx: &ExtensionContext) {
        // Publish the SecurityGuard handle on the bus so external callers
        // (e.g. CLI/UI permission resolvers) can find it without going
        // through a hardcoded BaseAgent accessor.
        let guard_opt = self.guard.lock().ok().and_then(|g| g.clone());
        if let Some(guard) = guard_opt {
            ctx.bus_writer.provide::<Mutex<SecurityGuard>>(guard);
        }
    }
}
