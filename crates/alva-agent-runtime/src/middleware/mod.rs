// INPUT:  security, plan_mode, checkpoint
// OUTPUT: SecurityMiddleware, PlanModeMiddleware, CheckpointMiddleware
// POS:    Domain-specific middleware implementations — lives here because they depend on domain crates.
pub mod checkpoint;
pub mod compaction;
pub mod plan_mode;
pub mod security;

/// Tools that perform write/execute operations.
/// Used by CheckpointMiddleware and PlanModeMiddleware.
pub const WRITE_TOOL_NAMES: &[&str] = &["create_file", "file_edit", "execute_shell"];
pub use checkpoint::{CheckpointCallback, CheckpointCallbackRef, CheckpointMiddleware};
pub use compaction::{CompactionConfig, CompactionMiddleware};
pub use plan_mode::PlanModeMiddleware;
pub use security::{ApprovalNotifier, ApprovalRequest, SecurityMiddleware};
