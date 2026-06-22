// INPUT:  std::sync::atomic, alva_kernel_abi::bus_cap, crate::modes::PermissionMode
// OUTPUT: SecurityModeControl, SecurityModeHandle
// POS:    Atomic-backed, lock-free toggle for the security guard's permission mode.
//         Lets callers in different crates flip the mode without holding the
//         guard's tokio Mutex.

use std::sync::atomic::{AtomicU8, Ordering};

use alva_kernel_abi::bus_cap;

use crate::modes::PermissionMode;

/// Bus Capability: lock-free knob for `SecurityGuard.permission_mode`.
///
/// **Provider**: `SecurityPlugin::register`
/// (`alva-agent-extension-builtin/src/wrappers/security.rs`). The
/// concrete impl is `SecurityModeHandle`, the same `Arc` the guard reads
/// through, so a `set_mode` call is observed by the next
/// `check_tool_call` without further synchronization.
/// **Consumers**: `PermissionModeService::set` in `alva-app-core` —
/// fans out the app-level mode change to whichever subsystems registered
/// a handle.
/// **Why bus**: `SecurityGuard` lives in `alva-agent-security`,
/// `PermissionModeService` lives in `alva-app-core`. The bus is the only
/// seam that lets the two cooperate without a compile-time dependency
/// between them.
#[bus_cap]
pub trait SecurityModeControl: Send + Sync {
    fn set_mode(&self, mode: PermissionMode);
    fn get_mode(&self) -> PermissionMode;
}

/// Atomic, `Clone`-via-`Arc` handle that backs both the
/// `SecurityModeControl` bus capability and `SecurityGuard`'s mode
/// reads. Storing the discriminant in an `AtomicU8` keeps reads
/// lock-free on the hot path inside `check_tool_call`.
#[derive(Debug)]
pub struct SecurityModeHandle {
    mode: AtomicU8,
}

impl SecurityModeHandle {
    pub fn new(initial: PermissionMode) -> Self {
        Self {
            mode: AtomicU8::new(initial as u8),
        }
    }
}

impl Default for SecurityModeHandle {
    fn default() -> Self {
        Self::new(PermissionMode::default())
    }
}

impl SecurityModeControl for SecurityModeHandle {
    fn set_mode(&self, mode: PermissionMode) {
        self.mode.store(mode as u8, Ordering::Relaxed);
    }

    fn get_mode(&self) -> PermissionMode {
        PermissionMode::from_u8(self.mode.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let h = SecurityModeHandle::new(PermissionMode::Interactive);
        assert_eq!(h.get_mode(), PermissionMode::Interactive);
        h.set_mode(PermissionMode::Auto);
        assert_eq!(h.get_mode(), PermissionMode::Auto);
        h.set_mode(PermissionMode::Plan);
        assert_eq!(h.get_mode(), PermissionMode::Plan);
        h.set_mode(PermissionMode::Bypass);
        assert_eq!(h.get_mode(), PermissionMode::Bypass);
        h.set_mode(PermissionMode::Default);
        assert_eq!(h.get_mode(), PermissionMode::Default);
    }

    #[test]
    fn shared_arc_observes_writes() {
        use std::sync::Arc;
        let h = Arc::new(SecurityModeHandle::new(PermissionMode::Default));
        let h2 = h.clone();
        h.set_mode(PermissionMode::Auto);
        assert_eq!(h2.get_mode(), PermissionMode::Auto);
    }
}
