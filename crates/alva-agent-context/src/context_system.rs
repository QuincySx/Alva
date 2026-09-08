// INPUT:  std::sync::Arc, alva_kernel_abi::scope::context::ContextSystem, crate implementations
// OUTPUT: re-exports ContextSystem, provides default_context_system()
// POS:    Re-exports ContextSystem from alva_kernel_abi::context and provides a default constructor function.
//! ContextSystem — re-exported from `alva_kernel_abi::context` with a default constructor.

use std::sync::Arc;

// Re-export the struct from alva_kernel_abi::context
pub use alva_kernel_abi::scope::context::ContextSystem;

use alva_kernel_abi::scope::context::{ContextHandle, ContextHooks};

/// Create a default ContextSystem with RulesContextHooks + in-memory ContextStore.
///
/// This is the recommended way to get a working context system for development
/// and testing. For production, construct a `ContextSystem` manually with your
/// preferred implementations.
pub fn default_context_system() -> ContextSystem {
    let store = Arc::new(std::sync::Mutex::new(crate::store::ContextStore::new(
        200_000, 180_000,
    )));
    let handle: Arc<dyn ContextHandle> = Arc::new(crate::sdk_impl::ContextHandleImpl::new(store));
    let hooks: Arc<dyn ContextHooks> = Arc::new(crate::rules_plugin::RulesContextHooks::default());
    // 注入/压缩的执行器挂在上下文系统上（ticket 10）：内核只问时机，落地
    // 归上下文能力库。
    ContextSystem::new(hooks, handle)
        .with_apply(Arc::new(crate::context_apply::DefaultContextApply))
}

#[cfg(test)]
mod tests {
    //! Tests for the `default_context_system()` factory.
    //!
    //! Silent changes to the budget numbers, the hooks plugin, or the
    //! default wiring contract would shift behavior for
    //! every caller of `default_context_system()` without compile-time
    //! signal — those callers are the recommended path for dev / test
    //! according to the docstring.
    use super::*;

    #[test]
    fn returns_a_usable_context_system_without_panicking() {
        // Smoke: factory just compiles + runs.
        let _system = default_context_system();
    }

    #[test]
    fn wires_rules_context_hooks_as_default_plugin() {
        // Pin: RulesContextHooks is the fallback path (no LLM). Wiring
        // something else here would silently change every fallback
        // session's compression behavior.
        let system = default_context_system();
        assert_eq!(
            system.hooks().name(),
            "rules-context-plugin",
            "default must wire RulesContextHooks (name 'rules-context-plugin')"
        );
    }

    #[test]
    fn budget_reports_two_hundred_thousand_model_window() {
        // Pin the literal 200_000 model_window — changing this
        // silently halves (or doubles) every dev / test session's
        // total context quota.
        let system = default_context_system();
        let budget = system.handle().budget("default-agent");
        assert_eq!(budget.model_window, 200_000, "model_window must be 200_000");
    }

    #[test]
    fn budget_reports_one_hundred_eighty_thousand_budget_tokens() {
        // Pin budget_tokens = 180_000. Together with model_window =
        // 200_000 above, this leaves 20_000 tokens of headroom — the
        // documented "auto-compact ratio" working space. Drifting
        // these numbers changes when auto-compact fires.
        let system = default_context_system();
        let budget = system.handle().budget("default-agent");
        assert_eq!(
            budget.budget_tokens, 180_000,
            "budget_tokens must be 180_000"
        );
    }

    #[test]
    fn fresh_system_has_zero_used_tokens_and_full_remaining() {
        // Pin: fresh store starts at 0 used / full remaining /
        // usage_ratio = 0. Without this, callers can't trust "no
        // session activity yet" as a precondition.
        let system = default_context_system();
        let budget = system.handle().budget("default-agent");
        assert_eq!(budget.used_tokens, 0);
        assert_eq!(budget.remaining_tokens, budget.budget_tokens);
        assert_eq!(budget.usage_ratio, 0.0);
    }
}
