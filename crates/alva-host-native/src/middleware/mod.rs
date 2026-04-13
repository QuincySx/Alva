// INPUT:  checkpoint (host-local), alva_agent_security::middleware, alva_agent_context::middleware
// OUTPUT: CheckpointMiddleware, SecurityMiddleware, PlanModeMiddleware, CompactionMiddleware (re-exported)
// POS:    Host-native middleware aggregation — checkpoint stays here (host-level persistence);
//         security/plan_mode/compaction are owned by their respective L3 boxes and re-exported for ergonomic access.
pub mod checkpoint;

pub use checkpoint::{CheckpointCallback, CheckpointCallbackRef, CheckpointMiddleware};

// Re-exports from L3 boxes — keeps `crate::middleware::SecurityMiddleware` etc. callsites working.
pub use alva_agent_context::middleware::CompactionMiddleware;
pub use alva_agent_security::middleware::{
    ApprovalNotifier, ApprovalRequest, PlanModeControl, PlanModeMiddleware, SecurityMiddleware,
};
