use std::sync::{Arc, Mutex};

use alva_host_native::middleware::PlanModeControl;
use alva_kernel_abi::bus_cap;

/// Controls how the agent handles tool permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// All write/execute tools require human approval (default).
    Ask,
    /// All write tools auto-approved; shell commands still need approval.
    AcceptEdits,
    /// No tools execute — agent can only read and analyze.
    Plan,
}

impl Default for PermissionMode {
    fn default() -> Self {
        PermissionMode::Ask
    }
}

/// Bus Capability: session-wide permission mode (ask / accept-edits / plan).
///
/// **Provider**: `PlanModeExtension::configure`
/// (`alva-app-core/src/extension/plan_mode.rs`). Exactly one production
/// producer; the outer app registers this extension to opt in.
/// **Consumers**: `BaseAgent::permission_mode()` /
/// `BaseAgent::set_permission_mode()` read and write through the bus;
/// the CLI / UI talks through those accessors.
/// **Why bus**: The permission-mode state is set from the outer harness
/// (CLI flags, UI buttons) but consulted by middleware in an entirely
/// different crate (`alva-agent-security::PlanModeMiddleware`). Threading
/// an `Arc` through every layer's constructor would leak the concept
/// into crates that shouldn't need to know about it.
///
/// The service additionally toggles any `PlanModeControl` it was given a
/// handle to, so mode changes transparently flip plan-mode enforcement.
#[bus_cap]
pub struct PermissionModeService {
    mode: Mutex<PermissionMode>,
    plan_ctrl: Option<Arc<dyn PlanModeControl>>,
}

impl PermissionModeService {
    pub fn new(initial: PermissionMode, plan_ctrl: Option<Arc<dyn PlanModeControl>>) -> Self {
        // Make sure plan-mode middleware starts in sync with our initial value.
        if let Some(ctrl) = plan_ctrl.as_ref() {
            ctrl.set_enabled(initial == PermissionMode::Plan);
        }
        Self {
            mode: Mutex::new(initial),
            plan_ctrl,
        }
    }

    pub fn get(&self) -> PermissionMode {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set(&self, mode: PermissionMode) {
        {
            let mut m = self.mode.lock().unwrap_or_else(|e| e.into_inner());
            *m = mode;
        }
        if let Some(ctrl) = self.plan_ctrl.as_ref() {
            ctrl.set_enabled(mode == PermissionMode::Plan);
        }
    }
}
