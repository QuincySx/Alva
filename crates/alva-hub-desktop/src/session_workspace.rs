// INPUT:  filesystem paths plus UUID generation
// OUTPUT: owner-only Hub-managed workspace creation and durable workspace-kind classification
// POS:    Product boundary between an isolated conversation workspace and a user-selected project.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const SESSION_WORKSPACE_MARKER: &str = ".alva-hub-session-workspace";

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKind {
    Session,
    #[default]
    Project,
}

impl WorkspaceKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Project => "project",
        }
    }

    pub(crate) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "session" => Ok(Self::Session),
            "project" => Ok(Self::Project),
            other => Err(format!("unknown Hub workspace kind '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorkspaceBinding {
    pub kind: WorkspaceKind,
    pub path: PathBuf,
}

/// Create one workspace only after the user chooses the independent-session
/// mode. Managed workspaces are never cleaned up implicitly.
pub fn create_managed_session_workspace(root: &Path) -> Result<WorkspaceBinding, String> {
    ensure_private_directory(root, "Hub session workspace root")?;

    for _ in 0..8 {
        let path = root.join(format!("session-{}", uuid::Uuid::new_v4()));
        match fs::create_dir(&path) {
            Ok(()) => {
                make_owner_only(&path)?;
                let marker = path.join(SESSION_WORKSPACE_MARKER);
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options
                    .open(&marker)
                    .map_err(|error| format!("create Hub session workspace marker: {error}"))?;
                file.write_all(b"1\n")
                    .map_err(|error| format!("write Hub session workspace marker: {error}"))?;
                file.sync_all()
                    .map_err(|error| format!("sync Hub session workspace marker: {error}"))?;
                return Ok(WorkspaceBinding {
                    kind: WorkspaceKind::Session,
                    path,
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("create Hub session workspace: {error}")),
        }
    }

    Err("could not allocate a unique Hub session workspace".into())
}

pub(crate) fn workspace_kind_for_path(path: &Path) -> WorkspaceKind {
    let marker = path.join(SESSION_WORKSPACE_MARKER);
    match fs::symlink_metadata(marker) {
        Ok(metadata) if metadata.file_type().is_file() => WorkspaceKind::Session,
        _ => WorkspaceKind::Project,
    }
}

fn ensure_private_directory(path: &Path, label: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!("{label} must be a real directory"));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(|error| format!("create {label}: {error}"))?;
            let metadata = fs::symlink_metadata(path)
                .map_err(|error| format!("inspect newly created {label}: {error}"))?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!("{label} must be a real directory"));
            }
        }
        Err(error) => return Err(format!("inspect {label}: {error}")),
    }
    make_owner_only(path)
}

fn make_owner_only(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|error| format!("protect {}: {error}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_unique_owner_only_session_workspaces() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("session-workspaces");

        let first = create_managed_session_workspace(&root).unwrap();
        let second = create_managed_session_workspace(&root).unwrap();

        assert_eq!(first.kind, WorkspaceKind::Session);
        assert_ne!(first.path, second.path);
        assert_eq!(workspace_kind_for_path(&first.path), WorkspaceKind::Session);
        assert_eq!(
            workspace_kind_for_path(parent.path()),
            WorkspaceKind::Project
        );
        assert!(first.path.join(SESSION_WORKSPACE_MARKER).is_file());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&root).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&first.path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlink_workspace_root() {
        use std::os::unix::fs::symlink;

        let parent = tempfile::tempdir().unwrap();
        let actual = parent.path().join("actual");
        fs::create_dir(&actual).unwrap();
        let linked = parent.path().join("linked");
        symlink(&actual, &linked).unwrap();

        let error = create_managed_session_workspace(&linked).unwrap_err();
        assert!(error.contains("real directory"));
    }
}
