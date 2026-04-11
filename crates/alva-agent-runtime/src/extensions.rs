//! Middleware extensions — capability packages for production middleware.

use std::sync::Arc;

use alva_agent_core::extension::{Extension, ExtensionAPI};

use crate::middleware::{
    CheckpointMiddleware, CompactionMiddleware, PlanModeMiddleware,
};

/// Production guardrails: loop detection + dangling tool check + timeout.
pub struct GuardrailsExtension;

impl Extension for GuardrailsExtension {
    fn name(&self) -> &str { "guardrails" }
    fn description(&self) -> &str { "Loop detection, dangling tool check, tool timeout" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_middleware(Arc::new(alva_agent_core::builtins::LoopDetectionMiddleware::new()));
        api.add_middleware(Arc::new(alva_agent_core::builtins::DanglingToolCallMiddleware::new()));
        api.add_middleware(Arc::new(alva_agent_core::builtins::ToolTimeoutMiddleware::default()));
    }
}

/// Context compaction (auto-summarize when context is full).
pub struct CompactionExtension;

impl Extension for CompactionExtension {
    fn name(&self) -> &str { "compaction" }
    fn description(&self) -> &str { "Auto-summarize old messages when context window is full" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_middleware(Arc::new(CompactionMiddleware::default()));
    }
}

/// File checkpointing (backup before writes).
pub struct CheckpointExtension;

impl Extension for CheckpointExtension {
    fn name(&self) -> &str { "checkpoint" }
    fn description(&self) -> &str { "Create file backups before write operations" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_middleware(Arc::new(CheckpointMiddleware::new()));
    }
}

/// Plan mode (block write tools when enabled).
pub struct PlanModeExtension;

impl Extension for PlanModeExtension {
    fn name(&self) -> &str { "plan-mode" }
    fn description(&self) -> &str { "Block write/execute tools in plan mode (read-only)" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_middleware(Arc::new(PlanModeMiddleware::new(false)));
    }
}

/// Full production middleware stack.
/// Equivalent to: guardrails + compaction + checkpoint + plan-mode.
pub struct ProductionExtension;

impl Extension for ProductionExtension {
    fn name(&self) -> &str { "production" }
    fn description(&self) -> &str { "Full production middleware: guardrails, compaction, checkpoint, plan mode" }
    fn activate(&self, api: &mut ExtensionAPI) {
        api.add_middleware(Arc::new(alva_agent_core::builtins::LoopDetectionMiddleware::new()));
        api.add_middleware(Arc::new(alva_agent_core::builtins::DanglingToolCallMiddleware::new()));
        api.add_middleware(Arc::new(alva_agent_core::builtins::ToolTimeoutMiddleware::default()));
        api.add_middleware(Arc::new(CompactionMiddleware::default()));
        api.add_middleware(Arc::new(CheckpointMiddleware::new()));
        api.add_middleware(Arc::new(PlanModeMiddleware::new(false)));
    }
}
