// INPUT:  std::sync::Arc, alva_kernel_abi::context::ContextSystem, crate implementations
// OUTPUT: re-exports ContextSystem, provides default_context_system()
// POS:    Re-exports ContextSystem from alva_kernel_abi::context and provides a default constructor function.
//! ContextSystem — re-exported from `alva_kernel_abi::context` with a default constructor.

use std::sync::Arc;

// Re-export the struct from alva_kernel_abi::context
pub use alva_kernel_abi::context::ContextSystem;

use alva_kernel_abi::context::{ContextHandle, ContextHooks};

/// Create a default ContextSystem with RulesContextHooks + in-memory ContextStore.
///
/// This is the recommended way to get a working context system for development
/// and testing. For production, construct a `ContextSystem` manually with your
/// preferred implementations.
pub fn default_context_system() -> ContextSystem {
    let store = Arc::new(std::sync::Mutex::new(
        crate::store::ContextStore::new(200_000, 180_000),
    ));
    let handle: Arc<dyn ContextHandle> = Arc::new(
        crate::sdk_impl::ContextHandleImpl::new(store),
    );
    let hooks: Arc<dyn ContextHooks> = Arc::new(
        crate::rules_plugin::RulesContextHooks::default(),
    );
    ContextSystem::new(hooks, handle)
}
