// INPUT:  std::path
// OUTPUT: normalize_path
// POS:    Shared path utilities — best-effort normalization for paths that may not exist yet.

use std::path::{Path, PathBuf};

/// Best-effort normalization for paths that may not exist yet.
///
/// Resolves `.` and `..` components without touching the filesystem.
/// Used by both `SecurityGuard` and `SensitivePathFilter`.
pub(crate) fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}
