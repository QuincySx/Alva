use std::sync::Mutex;

use alva_agent_security::SecurityModeControl;
use alva_host_native::middleware::PlanModeControl;
use alva_kernel_abi::{bus_cap, BusHandle};

/// Controls how the agent handles tool permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    /// All write/execute tools require human approval (default).
    Ask,
    /// All write tools auto-approved; shell commands still need approval.
    AcceptEdits,
    /// Shell commands auto-approved when `BashClassifier` deems them safe
    /// or unknown — only `Destructive` commands are blocked. Trusts the
    /// sandbox; only enable if a sandbox is in effect.
    AcceptShell,
    /// No tools execute — agent can only read and analyze.
    Plan,
}

impl Default for PermissionMode {
    fn default() -> Self {
        PermissionMode::Ask
    }
}

impl PermissionMode {
    /// Map the app-level mode to the underlying security guard mode.
    pub fn to_security_mode(self) -> alva_agent_security::PermissionMode {
        use alva_agent_security::PermissionMode as Sec;
        match self {
            PermissionMode::Ask => Sec::Interactive,
            PermissionMode::AcceptEdits => Sec::Interactive,
            PermissionMode::AcceptShell => Sec::Auto,
            PermissionMode::Plan => Sec::Plan,
        }
    }
}

/// Bus Capability: session-wide permission mode (ask / accept-edits /
/// accept-shell / plan).
///
/// **Provider**: `PermissionModeExtension::configure`
/// (`alva-app-core/src/extension/permission_mode.rs`). Optional — outer
/// apps register the extension only if they want runtime mode toggling.
/// **Consumers**: `BaseAgent::permission_mode()` /
/// `BaseAgent::set_permission_mode()`. CLI / UI talks through those.
/// **Why bus**: keeps the service crate-agnostic. It does not depend
/// on `PlanModeMiddleware` or `SecurityGuard` at construction time;
/// instead `set()` queries the bus for whichever subsystem control
/// handles are present (`dyn PlanModeControl`, `dyn SecurityModeControl`)
/// and fans the change out to each. Adding a third subsystem in the
/// future means publishing a new control trait — this service does not
/// change.
#[bus_cap]
pub struct PermissionModeService {
    mode: Mutex<PermissionMode>,
    bus: BusHandle,
}

impl PermissionModeService {
    /// Construct the service. The initial mode is propagated to whichever
    /// subsystem controls are already on the bus; controls registered
    /// later will be picked up at the next `set()` call.
    pub fn new(initial: PermissionMode, bus: BusHandle) -> Self {
        let svc = Self {
            mode: Mutex::new(initial),
            bus,
        };
        svc.fan_out(initial);
        svc
    }

    pub fn get(&self) -> PermissionMode {
        *self.mode.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set(&self, mode: PermissionMode) {
        {
            let mut m = self.mode.lock().unwrap_or_else(|e| e.into_inner());
            *m = mode;
        }
        self.fan_out(mode);
    }

    fn fan_out(&self, mode: PermissionMode) {
        if let Some(ctrl) = self.bus.get::<dyn PlanModeControl>() {
            ctrl.set_enabled(mode == PermissionMode::Plan);
        }
        if let Some(ctrl) = self.bus.get::<dyn SecurityModeControl>() {
            ctrl.set_mode(mode.to_security_mode());
        }
    }
}
