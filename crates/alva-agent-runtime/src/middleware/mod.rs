// INPUT:  security, plan_mode, checkpoint
// OUTPUT: SecurityMiddleware, PlanModeMiddleware, CheckpointMiddleware
// POS:    Domain-specific middleware implementations — lives here because they depend on domain crates.
pub mod checkpoint;
pub mod plan_mode;
pub mod security;
pub use checkpoint::{CheckpointCallback, CheckpointCallbackRef, CheckpointMiddleware};
pub use plan_mode::PlanModeMiddleware;
pub use security::{ApprovalNotifier, ApprovalRequest, SecurityMiddleware};
