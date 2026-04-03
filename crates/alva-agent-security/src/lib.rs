// INPUT:  guard, permission, sensitive_paths, authorized_roots, sandbox, rules, cache, modes, classifier
// OUTPUT: SecurityGuard, SecurityDecision, PermissionManager, PermissionDecision, SensitivePathFilter,
//         AuthorizedRoots, SandboxConfig, SandboxMode, PermissionRules, RuleDecision, PermissionRule,
//         PermissionCache, CachedDecision, PermissionMode, BashClassifier, CommandClassification
// POS:    Crate root — declares security modules and re-exports the public API.

pub mod guard;
pub mod permission;
pub mod sensitive_paths;
pub mod authorized_roots;
pub mod sandbox;
pub mod rules;
pub mod cache;
pub mod modes;
pub mod classifier;
pub(crate) mod path_utils;

pub use guard::{SecurityGuard, SecurityDecision};
pub use permission::{PermissionManager, PermissionDecision};
pub use sensitive_paths::SensitivePathFilter;
pub use authorized_roots::AuthorizedRoots;
pub use sandbox::{SandboxConfig, SandboxMode};
pub use rules::{PermissionRule, PermissionRules, RuleDecision};
pub use cache::{PermissionCache, CachedDecision};
pub use modes::PermissionMode;
pub use classifier::{BashClassifier, CommandClassification};
