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
/// **Provider**: `PermissionPlugin::register`
/// (`alva-app-core/src/extension/permission.rs`). Optional — outer
/// apps register the plugin only if they want runtime mode toggling.
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

#[cfg(test)]
mod tests {
    //! Tests for PermissionMode — Default + to_security_mode mapping.
    //!
    //! Security-relevant: Default must be the most restrictive variant
    //! (Ask), and the mapping to the underlying security PermissionMode
    //! has to stay correct or users will think they're in `Ask` while
    //! actually running in `Auto` (silent privilege escalation).
    use super::*;

    // -- Default = Ask (safe-by-default) ----------------------------------

    #[test]
    fn default_is_ask_for_safe_by_default() {
        // Pin: the most restrictive variant is the default. Anything
        // else is a silent reduction in safety surface area when a
        // caller forgets to override.
        assert_eq!(PermissionMode::default(), PermissionMode::Ask);
    }

    // -- to_security_mode: 4-variant mapping ------------------------------
    //
    // Each app-level variant maps to a specific security crate variant.
    // Wrong arms produce silent privilege escalation: e.g. mapping Ask
    // to security::Auto would mean every tool runs without prompting
    // even though the app reports "Ask".

    #[test]
    fn ask_maps_to_security_interactive() {
        assert_eq!(
            PermissionMode::Ask.to_security_mode(),
            alva_agent_security::PermissionMode::Interactive
        );
    }

    #[test]
    fn accept_edits_maps_to_security_interactive() {
        // Pin the subtlety: AcceptEdits is STILL Interactive in the
        // security mode (the auto-approval of *edits* happens at the
        // approval-notifier level, not by relaxing the security mode
        // for everything). A future refactor that "simplifies"
        // AcceptEdits → Auto would silently allow shell as well.
        assert_eq!(
            PermissionMode::AcceptEdits.to_security_mode(),
            alva_agent_security::PermissionMode::Interactive
        );
    }

    #[test]
    fn accept_shell_maps_to_security_auto() {
        // This is the "trust the sandbox" mapping. Pin so a refactor
        // that accidentally maps AcceptShell back to Interactive
        // re-introduces prompts for every bash command.
        assert_eq!(
            PermissionMode::AcceptShell.to_security_mode(),
            alva_agent_security::PermissionMode::Auto
        );
    }

    #[test]
    fn plan_maps_to_security_plan() {
        // Plan stays Plan on both sides — symmetric naming, but pin
        // anyway because a future cleanup might merge or rename it.
        assert_eq!(
            PermissionMode::Plan.to_security_mode(),
            alva_agent_security::PermissionMode::Plan
        );
    }

    // -- PartialEq + Copy semantics ---------------------------------------

    #[test]
    fn copy_and_eq_round_trip() {
        // Trivial smoke pin: PermissionMode is `Copy + PartialEq` so
        // it can be cheaply moved through Mutex<PermissionMode> /
        // bus calls without surprise. A refactor dropping Copy would
        // ripple into BaseAgent.permission_mode() ergonomics.
        let a = PermissionMode::AcceptShell;
        let b = a;
        assert_eq!(a, b);
        assert_ne!(a, PermissionMode::Plan);
    }

    #[test]
    fn variants_are_mutually_distinct() {
        // Pin: the 4 variants must all compare different to each other.
        // A refactor that collapses, say, AcceptEdits == AcceptShell
        // would silently change the fan_out behavior — Plan mode is
        // detected via `mode == PermissionMode::Plan` in fan_out().
        let all = [
            PermissionMode::Ask,
            PermissionMode::AcceptEdits,
            PermissionMode::AcceptShell,
            PermissionMode::Plan,
        ];
        for i in 0..all.len() {
            for j in (i + 1)..all.len() {
                assert_ne!(
                    all[i], all[j],
                    "PermissionMode variants {:?} and {:?} must be distinct",
                    all[i], all[j]
                );
            }
        }
    }
}
