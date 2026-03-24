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
        let resolved = path
            .canonicalize()
            .unwrap_or_else(|_| Self::normalize(path));

        // Workspace is always authorized
        if resolved.starts_with(&self.workspace) {
            return Ok(());
        }

        // Check extra roots
        for root in &self.extra_roots {
            if resolved.starts_with(root) {
                return Ok(());
            }
        }

        Err(format!(
            "path '{}' is outside all authorized roots (workspace: {})",
            resolved.display(),
            self.workspace.display()
        ))
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
        assert!(roots.check(Path::new("/projects/myapp/src/main.rs")).is_ok());
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
        assert!(roots.check(Path::new("/tmp/srow-output/results.json")).is_ok());
    }

    #[test]
    fn remove_extra_root() {
        let mut roots = AuthorizedRoots::new(PathBuf::from("/projects/myapp"));
        roots.add_root(PathBuf::from("/tmp/srow-output"));
        roots.remove_root(Path::new("/tmp/srow-output"));
        assert!(roots.check(Path::new("/tmp/srow-output/results.json")).is_err());
    }
}
