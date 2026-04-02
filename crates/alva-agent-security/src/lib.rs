// INPUT:  guard, permission, sensitive_paths, authorized_roots, sandbox
// OUTPUT: SecurityGuard, SecurityDecision, PermissionManager, PermissionDecision, SensitivePathFilter, AuthorizedRoots, SandboxConfig, SandboxMode
// POS:    Crate root — declares security modules and re-exports the public API.

pub mod guard;
pub mod permission;
pub mod sensitive_paths;
pub mod authorized_roots;
pub mod sandbox;
pub(crate) mod path_utils;

pub use guard::{SecurityGuard, SecurityDecision};
pub use permission::{PermissionManager, PermissionDecision};
pub use sensitive_paths::SensitivePathFilter;
pub use authorized_roots::AuthorizedRoots;
pub use sandbox::{SandboxConfig, SandboxMode};
