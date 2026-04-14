//! Plan mode extension — blocks non-read-only tools when plan mode is active.
//!
//! Runtime toggle is exposed via the bus as `dyn PlanModeControl`, allowing
//! `BaseAgent::set_permission_mode()` to toggle it without a typed reference.

use std::sync::Arc;

use async_trait::async_trait;

use alva_host_native::middleware::{PlanModeControl, PlanModeMiddleware};

use crate::base_agent::{PermissionMode, PermissionModeService};

use super::{Extension, ExtensionContext, HostAPI};

pub struct PlanModeExtension {
    middleware: Arc<PlanModeMiddleware>,
}

impl PlanModeExtension {
    pub fn new() -> Self {
        Self {
            middleware: Arc::new(PlanModeMiddleware::new(false)),
        }
    }
}

impl Default for PlanModeExtension {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Extension for PlanModeExtension {
    fn name(&self) -> &str { "plan-mode" }
    fn description(&self) -> &str { "Plan mode (read-only tool restriction, runtime toggle)" }

    fn activate(&self, api: &HostAPI) {
        api.middleware(self.middleware.clone());
    }

    async fn configure(&self, ctx: &ExtensionContext) {
        // Register PlanModeControl on bus for runtime toggle
        ctx.bus_writer
            .provide::<dyn PlanModeControl>(self.middleware.clone());

        // Publish a PermissionModeService that owns the current permission
        // mode for the whole agent. It wraps our PlanModeMiddleware so mode
        // changes transparently toggle plan-mode enforcement.
        let service = Arc::new(PermissionModeService::new(
            PermissionMode::Ask,
            Some(self.middleware.clone() as Arc<dyn PlanModeControl>),
        ));
        ctx.bus_writer.provide(service);
    }
}
