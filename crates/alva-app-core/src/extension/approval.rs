//! Approval extension — bus-publishes an [`ApprovalNotifier`] so middleware
//! that needs human approval (e.g. `SecurityMiddleware`) can look it up via
//! `bus.get::<ApprovalNotifier>()`.
//!
//! Construct via [`ApprovalExtension::with_channel`] to simultaneously obtain
//! the receiver half of the approval-request channel; the caller owns the
//! receiver and processes `ApprovalRequest`s (UI prompt, auto-approve, etc.).

use std::sync::{Arc, Mutex};

use alva_agent_core::extension::{Extension, ExtensionContext, HostAPI};
use alva_host_native::middleware::{ApprovalNotifier, ApprovalRequest};
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Approval extension. Publishes an `ApprovalNotifier` on the bus during
/// `configure()` so that security middleware can reach the UI.
pub struct ApprovalExtension {
    notifier: Mutex<Option<ApprovalNotifier>>,
}

impl ApprovalExtension {
    /// Create a new `ApprovalExtension` together with the receiver half of
    /// its approval-request channel. The extension publishes the notifier
    /// on the bus during `configure()`; the caller retains the receiver and
    /// processes approval requests however it wants (CLI prompt, auto-approve,
    /// GUI dialog, …).
    pub fn with_channel() -> (Self, mpsc::UnboundedReceiver<ApprovalRequest>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let notifier = ApprovalNotifier { tx };
        (
            Self {
                notifier: Mutex::new(Some(notifier)),
            },
            rx,
        )
    }
}

#[async_trait]
impl Extension for ApprovalExtension {
    fn name(&self) -> &str {
        "approval"
    }

    fn description(&self) -> &str {
        "Human approval flow notifier"
    }

    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        Vec::new()
    }

    fn activate(&self, _api: &HostAPI) {}

    async fn configure(&self, ctx: &ExtensionContext) {
        // Take the notifier out exactly once; subsequent `configure()` calls
        // (should not happen) are no-ops.
        let notifier = self
            .notifier
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(notifier) = notifier {
            ctx.bus_writer.provide(Arc::new(notifier));
        }
    }
}
