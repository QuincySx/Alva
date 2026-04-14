use std::sync::{Arc, Mutex};

use alva_host_native::middleware::PlanModeControl;

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

/// Bus-published service that holds the current [`PermissionMode`] for the
/// whole agent. `BaseAgent::permission_mode()` / `set_permission_mode()` are
/// thin proxies that query/mutate this service via the bus.
///
/// The service additionally toggles any `PlanModeControl` it was given a
/// handle to, so mode changes transparently flip plan-mode enforcement.
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
