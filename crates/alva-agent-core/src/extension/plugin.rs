//! Plugin — 装配期跨层捆绑包（取代 Extension）。
//!
//! `Plugin::register()` is called once per assembly, before any tools or
//! model are finalised. Use `Plugin::finalize()` for cross-plugin late
//! wiring that requires reading capabilities provided by other plugins.

use async_trait::async_trait;
use std::sync::Arc;

use alva_kernel_abi::tool::Tool;

use super::registrar::{LateContext, Registrar};

/// A self-contained capability bundle that can register tools, middleware,
/// bus services, system-prompt fragments, and commands into a [`Registrar`].
///
/// Replaces the old `Extension` trait. During the transition period both
/// coexist; `ExtensionAsPlugin` bridges the gap.
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Unique identifier for this plugin (used in logs and diagnostics).
    fn name(&self) -> &str;

    /// Optional human-readable description.
    fn description(&self) -> &str {
        ""
    }

    /// **Assembly phase** — provide-only.
    ///
    /// Register tools / middleware / bus services / system-prompt fragments /
    /// commands into `r`. At this point other plugins may not have run yet,
    /// so do **not** read capabilities from the bus that another plugin will
    /// provide. Use [`finalize`](Self::finalize) for that.
    async fn register(&self, r: &Registrar);

    /// **Late phase** — called after all `register()` calls have finished
    /// and the complete tool set + model are known.
    ///
    /// Use this for dynamic tool discovery and cross-plugin late wiring.
    /// The default implementation returns an empty vec (no late tools).
    async fn finalize(&self, _cx: &LateContext) -> Vec<Arc<dyn Tool>> {
        vec![]
    }
}
