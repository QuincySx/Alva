//! Permission extension — session-wide permission mode infrastructure.
//!
//! Single domain: how the agent decides whether to run a tool. This
//! extension owns:
//!   1. `PermissionModeService` — the orchestrator that holds the
//!      app-level mode (Ask / AcceptEdits / AcceptShell / Plan) and
//!      fans changes out to whichever subsystem control handles are
//!      registered on the bus (`dyn PlanModeControl`,
//!      `dyn SecurityModeControl`, …).
//!   2. `PlanModeMiddleware` — the concrete enforcer that blocks
//!      non-read-only tools when Plan mode is active. Registered as
//!      `dyn PlanModeControl` so the service above can flip it.
//!
//! Plan mode is one form of permission mode, so co-locating the
//! orchestrator with this concrete enforcer is the right granularity:
//! one extension, one domain. Other subsystems (e.g. `SecurityExtension`)
//! plug in by publishing their own control trait on the bus — no
//! compile-time coupling.

use std::sync::Arc;

use async_trait::async_trait;

use alva_host_native::middleware::{PlanModeControl, PlanModeMiddleware};

use crate::base_agent::{PermissionMode, PermissionModeService};

use super::{Extension, ExtensionContext, HostAPI};

pub struct PermissionExtension {
    middleware: Arc<PlanModeMiddleware>,
    initial: PermissionMode,
}

impl PermissionExtension {
    pub fn new() -> Self {
        Self {
            middleware: Arc::new(PlanModeMiddleware::new(false)),
            initial: PermissionMode::default(),
        }
    }

    pub fn with_initial(mut self, mode: PermissionMode) -> Self {
        self.initial = mode;
        self
    }
}

impl Default for PermissionExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for PermissionExtension {
    fn name(&self) -> &str {
        "permission"
    }
    fn description(&self) -> &str {
        "Session-wide permission mode service + Plan-mode enforcement"
    }

    fn activate(&self, api: &HostAPI) {
        api.middleware(self.middleware.clone());
    }

    async fn configure(&self, ctx: &ExtensionContext) {
        ctx.bus_writer
            .provide::<dyn PlanModeControl>(self.middleware.clone());

        let service = Arc::new(PermissionModeService::new(self.initial, ctx.bus.clone()));
        ctx.bus_writer.provide(service);
    }
}
