//! Middleware extensions — capability packages for production middleware.

use std::sync::Arc;

use alva_agent_core::extension::Extension;
use alva_agent_core::middleware::Middleware;

use crate::middleware::{CheckpointMiddleware, CompactionMiddleware, PlanModeMiddleware};

pub struct GuardrailsExtension;
impl Extension for GuardrailsExtension {
    fn name(&self) -> &str { "guardrails" }
    fn description(&self) -> &str { "Loop detection, dangling tool check, tool timeout" }
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![
            Arc::new(alva_agent_core::builtins::LoopDetectionMiddleware::new()),
            Arc::new(alva_agent_core::builtins::DanglingToolCallMiddleware::new()),
            Arc::new(alva_agent_core::builtins::ToolTimeoutMiddleware::default()),
        ]
    }
}

pub struct CompactionExtension;
impl Extension for CompactionExtension {
    fn name(&self) -> &str { "compaction" }
    fn description(&self) -> &str { "Auto-summarize old messages when context window is full" }
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![Arc::new(CompactionMiddleware::default())]
    }
}

pub struct CheckpointExtension;
impl Extension for CheckpointExtension {
    fn name(&self) -> &str { "checkpoint" }
    fn description(&self) -> &str { "Create file backups before write operations" }
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![Arc::new(CheckpointMiddleware::new())]
    }
}

pub struct PlanModeExtension;
impl Extension for PlanModeExtension {
    fn name(&self) -> &str { "plan-mode" }
    fn description(&self) -> &str { "Block write/execute tools in plan mode" }
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![Arc::new(PlanModeMiddleware::new(false))]
    }
}

/// Full production middleware stack.
pub struct ProductionExtension;
impl Extension for ProductionExtension {
    fn name(&self) -> &str { "production" }
    fn description(&self) -> &str { "Full production middleware stack" }
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![
            Arc::new(alva_agent_core::builtins::LoopDetectionMiddleware::new()),
            Arc::new(alva_agent_core::builtins::DanglingToolCallMiddleware::new()),
            Arc::new(alva_agent_core::builtins::ToolTimeoutMiddleware::default()),
            Arc::new(CompactionMiddleware::default()),
            Arc::new(CheckpointMiddleware::new()),
            Arc::new(PlanModeMiddleware::new(false)),
        ]
    }
}
