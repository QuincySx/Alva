// INPUT:  std::path
// OUTPUT: AuthorizedRoots
// POS:    Manages the set of authorized directories the Agent can access, defaulting to workspace root.
use std::path::{Path, PathBuf};

/// Manages the set of directories the agent is authorized to access.
///
/// The primary root is always `workspace_path`. Additional roots can be
/// added (e.g., shared output directories, temp directories, globally
/// configured data dirs).
pub struct AuthorizedRoots {
    workspace: PathBuf,
    extra_roots: Vec<PathBuf>,
}

impl AuthorizedRoots {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            extra_roots: Vec::new(),
        }
    }

    /// Add an additional authorized root directory.
    pub fn add_root(&mut self, root: PathBuf) {
        if !self.extra_roots.contains(&root) {
            self.extra_roots.push(root);
        }
    }

    /// Remove a previously added extra root.
    pub fn remove_root(&mut self, root: &Path) {
        self.extra_roots.retain(|r| r != root);
    }

    /// Check whether `path` falls inside any authorized root.
    /// Returns `Ok(())` on success, `Err(reason)` on denial.
    pub fn check(&self, path: &Path) -> Result<(), String> {
        // Resolve BOTH the candidate and every root the same way, so the
        // comparison is between like-for-like. The old code canonicalized only
        // when the full path existed and fell back to a purely lexical
        // normalize for new files — which let `workspace/link/new_file`, where
        // `link` is a real symlink to an outside directory, pass as "inside
        // workspace" while the write landed outside. `resolve_for_check`
        // canonicalizes the nearest existing ancestor (following that symlink)
        // and re-attaches the not-yet-existing tail, so the escape is caught.
        let resolved = Self::resolve_for_check(path);

        if resolved.starts_with(Self::resolve_for_check(&self.workspace)) {
            return Ok(());
        }

        for root in &self.extra_roots {
            if resolved.starts_with(Self::resolve_for_check(root)) {
                return Ok(());
            }
        }

        Err(format!(
            "path '{}' is outside all authorized roots (workspace: {})",
            resolved.display(),
            self.workspace.display()
        ))
    }

    /// Resolve a path for containment checking.
    ///
    /// If the whole path exists, `canonicalize` resolves every symlink. If it
    /// does not (a new file, or a path under one), canonicalize the nearest
    /// existing ancestor — which follows any real symlink in the existing
    /// prefix — and re-attach the remaining components. `/` always
    /// canonicalizes, so a fully synthetic path (nothing on disk to point
    /// anywhere) reattaches its whole tail under the resolved root and matches
    /// the previous lexical behavior; there is no symlink to follow in that
    /// case anyway.
    fn resolve_for_check(path: &Path) -> PathBuf {
        if let Ok(canonical) = path.canonicalize() {
            return canonical;
        }
        let mut tail: Vec<std::ffi::OsString> = Vec::new();
        let mut cursor = path;
        while let Some(parent) = cursor.parent() {
            let Some(name) = cursor.file_name() else {
                // Trailing `..`/`.` — nothing lexically safe to re-attach.
                return Self::normalize(path);
            };
            tail.push(name.to_os_string());
            if let Ok(mut base) = parent.canonicalize() {
                for name in tail.iter().rev() {
                    base.push(name);
                }
                return base;
            }
            cursor = parent;
        }
        Self::normalize(path)
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn all_roots(&self) -> Vec<&Path> {
        let mut roots: Vec<&Path> = vec![&self.workspace];
        roots.extend(self.extra_roots.iter().map(|p| p.as_path()));
        roots
    }

    /// Best-effort normalization for paths that don't exist yet.
    fn normalize(path: &Path) -> PathBuf {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_workspace_paths() {
        let roots = AuthorizedRoots::new(PathBuf::from("/projects/myapp"));
        assert!(roots
            .check(Path::new("/projects/myapp/src/main.rs"))
            .is_ok());
        assert!(roots.check(Path::new("/projects/myapp")).is_ok());
    }

    #[test]
    fn denies_outside_workspace() {
        let roots = AuthorizedRoots::new(PathBuf::from("/projects/myapp"));
        assert!(roots.check(Path::new("/etc/passwd")).is_err());
        assert!(roots.check(Path::new("/projects/other/file.txt")).is_err());
    }

    #[test]
    fn allows_extra_root() {
        let mut roots = AuthorizedRoots::new(PathBuf::from("/projects/myapp"));
        roots.add_root(PathBuf::from("/tmp/srow-output"));
        assert!(roots
            .check(Path::new("/tmp/srow-output/results.json"))
            .is_ok());
    }

    #[test]
    fn remove_extra_root() {
        let mut roots = AuthorizedRoots::new(PathBuf::from("/projects/myapp"));
        roots.add_root(PathBuf::from("/tmp/srow-output"));
        roots.remove_root(Path::new("/tmp/srow-output"));
        assert!(roots
            .check(Path::new("/tmp/srow-output/results.json"))
            .is_err());
    }

    /// A real symlink inside the workspace pointing at an outside directory
    /// must not let a NEW file (which does not exist yet, so the old code fell
    /// back to lexical normalize) be treated as inside the workspace. Building
    /// the symlink on a real filesystem is the point — the escape only exists
    /// because the write target resolves through the link at the OS level.
    #[cfg(unix)]
    #[test]
    fn new_file_through_workspace_symlink_is_denied() {
        let workspace = tempfile::tempdir().expect("workspace");
        let outside = tempfile::tempdir().expect("outside");
        // workspace/escape -> /outside (a real, resolvable symlink)
        let link = workspace.path().join("escape");
        std::os::unix::fs::symlink(outside.path(), &link).expect("create symlink");

        let roots = AuthorizedRoots::new(workspace.path().to_path_buf());

        // The attacker writes a not-yet-existing file *through* the link; on
        // disk this lands in `outside`, so it must be refused.
        let escaping_new_file = link.join("stolen.txt");
        assert!(
            roots.check(&escaping_new_file).is_err(),
            "writing through a workspace symlink to an outside dir must be denied"
        );

        // A genuine new file directly in the workspace still passes.
        let legit_new_file = workspace.path().join("notes.txt");
        assert!(
            roots.check(&legit_new_file).is_ok(),
            "a new file directly inside the workspace must still be allowed"
        );
    }
}
