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

#[cfg(test)]
mod tests {
    //! Tests for normalize_path.
    //!
    //! Security-critical: this function is the basis for path
    //! comparison in SensitivePathFilter and SecurityGuard. A wrong
    //! result means a `/etc/../etc/passwd` style traversal slips
    //! through sensitive-path checks because the normalized form
    //! doesn't match what the filter looks for.
    //!
    //! It is a PURE STRING-LEVEL operation — does NOT touch the
    //! filesystem. Symlinks are not followed (intentional; we want
    //! to check what was REQUESTED, not what the path eventually
    //! resolves to on disk).
    use super::*;
    use std::path::PathBuf;

    fn norm(s: &str) -> PathBuf {
        normalize_path(Path::new(s))
    }

    // -- Pass-through paths -----------------------------------------------

    #[test]
    fn empty_path_normalizes_to_empty() {
        assert_eq!(norm(""), PathBuf::new());
    }

    #[test]
    fn simple_absolute_path_passes_through_unchanged() {
        assert_eq!(norm("/foo/bar"), PathBuf::from("/foo/bar"));
    }

    #[test]
    fn simple_relative_path_passes_through_unchanged() {
        assert_eq!(norm("foo/bar"), PathBuf::from("foo/bar"));
    }

    // -- CurDir handling ---------------------------------------------------

    #[test]
    fn dot_component_in_middle_is_stripped() {
        assert_eq!(norm("/foo/./bar"), PathBuf::from("/foo/bar"));
    }

    #[test]
    fn multiple_dots_all_stripped() {
        assert_eq!(norm("/./foo/./bar/./"), PathBuf::from("/foo/bar"));
    }

    // -- ParentDir handling ------------------------------------------------

    #[test]
    fn parent_dir_in_middle_pops_previous_component() {
        // SECURITY-CRITICAL: "/etc/../etc/passwd" must normalize to
        // "/etc/passwd" so that sensitive-path filters can compare
        // against the canonical form. Without this resolution,
        // /etc/../etc/passwd would silently bypass an /etc/passwd
        // entry in the sensitive list.
        assert_eq!(
            norm("/etc/../etc/passwd"),
            PathBuf::from("/etc/passwd"),
            "traversal must resolve so filters can match canonical form"
        );
    }

    #[test]
    fn nested_parent_dirs_resolve_layer_by_layer() {
        assert_eq!(
            norm("/foo/bar/baz/../../qux"),
            PathBuf::from("/foo/qux")
        );
    }

    #[test]
    fn parent_dir_at_relative_root_silently_drops() {
        // Pin current behavior: when `out` is empty and we hit `..`,
        // PathBuf::pop returns false but normalize_path doesn't track
        // that — the component is silently swallowed. So
        // "../etc/passwd" normalizes to "etc/passwd", and
        // "../../etc/passwd" also normalizes to "etc/passwd".
        // SECURITY NOTE: this means a relative path that escapes
        // upward from CWD comes out looking like a peer of CWD. The
        // SensitivePathFilter must join with cwd BEFORE normalizing,
        // not after. Pinned so a refactor doesn't accidentally
        // change the silent-drop into an error-propagation path
        // that callers don't expect.
        assert_eq!(norm("../etc/passwd"), PathBuf::from("etc/passwd"));
        assert_eq!(norm("../../etc/passwd"), PathBuf::from("etc/passwd"));
    }

    #[test]
    fn parent_dir_at_absolute_root_does_not_escape_root() {
        // Pin: "/../etc" pops nothing (RootDir stays), result is
        // "/etc". A bug here could let "/../etc/shadow" normalize
        // to "etc/shadow" (relative), bypassing absolute-path checks.
        assert_eq!(norm("/../etc"), PathBuf::from("/etc"));
    }

    #[test]
    fn trailing_slash_is_normalized_away() {
        // PathBuf treats trailing slash as no extra component;
        // pinned so callers comparing strings know "foo/" and "foo"
        // normalize identically.
        assert_eq!(norm("/foo/bar/"), PathBuf::from("/foo/bar"));
    }

    #[test]
    fn dot_only_path_normalizes_to_empty() {
        // CurDir components are stripped, so "./" alone yields empty.
        assert_eq!(norm("./"), PathBuf::new());
    }

    #[test]
    fn round_trip_already_normalized_path_is_idempotent() {
        // Pin: normalize(normalize(p)) == normalize(p) for any p —
        // the function must be idempotent. Without this, callers
        // that re-normalize defensively would get different results.
        let p = "/usr/local/bin/alva";
        let once = norm(p);
        let twice = normalize_path(&once);
        assert_eq!(once, twice);
    }
}
