// INPUT:  guard, permission, sensitive_paths, authorized_roots, sandbox, rules, cache, modes, classifier, middleware, pending_actions
// OUTPUT: SecurityGuard, SecurityDecision, PermissionManager, PermissionDecision, SensitivePathFilter,
//         AuthorizedRoots, SandboxConfig, SandboxEnforcement, SandboxMode, PermissionRules, RuleDecision, PermissionRule,
//         PermissionCache, CachedDecision, PermissionMode, BashClassifier, CommandClassification,
//         SecurityMiddleware, ApprovalNotifier, ApprovalRequest, PlanModeMiddleware, PlanModeControl,
//         PendingAction, ResolveStatus, pending_actions, EVENT_REQUIRES_ACTION, EVENT_REQUIRES_ACTION_RESOLVED
// POS:    Crate root — declares security modules and re-exports the public API.

pub mod authorized_roots;
pub mod cache;
pub mod classifier;
pub mod guard;
pub mod middleware;
pub mod mode_control;
pub mod modes;
pub(crate) mod path_utils;
pub mod pending_actions;
pub mod permission;
pub mod rules;
pub mod sandbox;
pub mod sensitive_paths;
pub mod url_info;

pub use authorized_roots::AuthorizedRoots;
pub use cache::{CachedDecision, PermissionCache};
pub use classifier::{BashClassifier, CommandClassification};
pub use guard::{SecurityDecision, SecurityGuard};
pub use middleware::{
    ApprovalNotifier, ApprovalRequest, PlanModeControl, PlanModeMiddleware, SecurityMiddleware,
};
pub use mode_control::{SecurityModeControl, SecurityModeHandle};
pub use modes::PermissionMode;
pub use pending_actions::{
    pending_actions, PendingAction, ResolveStatus, EVENT_REQUIRES_ACTION,
    EVENT_REQUIRES_ACTION_RESOLVED,
};
pub use permission::{PermissionDecision, PermissionManager};
pub use rules::{PermissionRule, PermissionRules, RuleDecision};
pub use sandbox::{SandboxConfig, SandboxEnforcement, SandboxMode};
pub use sensitive_paths::SensitivePathFilter;
