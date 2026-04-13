// INPUT:  security, plan_mode
// OUTPUT: SecurityMiddleware, ApprovalRequest, ApprovalNotifier, PlanModeMiddleware, PlanModeControl
// POS:    Tool-call gate middleware living next to the security domain types they enforce.

pub mod plan_mode;
pub mod security;

pub use plan_mode::{PlanModeControl, PlanModeMiddleware};
pub use security::{ApprovalNotifier, ApprovalRequest, SecurityMiddleware};
