// INPUT: HubStore session workspace, capability-relative filesystem, sensitive path rules
// OUTPUT: list_workspace_files/read_workspace_file and bounded read-only DTOs
// POS: Hub product file browser boundary shared by Tauri and HTTP.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use alva_agent_security::SensitivePathFilter;
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, OpenOptions};
use serde::Serialize;

use crate::HubStore;

const MAX_ENTRIES: usize = 2000;
const MAX_PREVIEW: usize = 256 * 1024;

#[derive(Debug, Serialize)]
pub struct WorkspaceEntry {
    pub name: String,
    pub path: String,
    pub kind: &'static str,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceDirectory {
    pub path: String,
    pub entries: Vec<WorkspaceEntry>,
    pub truncated: bool,
}

#[derive(Debug, Serialize)]
pub struct WorkspacePreview {
    pub path: String,
    pub kind: &'static str,
    pub text: Option<String>,
    pub total_bytes: u64,
    pub truncated: bool,
}

pub(crate) fn session_workspace(store: &HubStore, session: &str) -> Result<PathBuf, String> {
    store
        .get_session(session)?
        .map(|detail| PathBuf::from(detail.session.workspace))
        .ok_or_else(|| "workspace session not found".into())
}

pub(crate) fn relative_path(path: &str) -> Result<PathBuf, String> {
    if path.len() > 4096 || path.contains('\\') || path.contains('\0') {
        return Err("invalid workspace path".into());
    }
    let result = PathBuf::from(path);
    if result
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err("workspace path must be relative without traversal".into());
    }
    Ok(result)
}

// Reject metadata and secret containers too: a sensitive name must not become
// an allowed directory whose children bypass the filename-only default rule.
pub(crate) fn allowed_path(root: &Path, relative: &Path) -> bool {
    static FILTER: std::sync::OnceLock<SensitivePathFilter> = std::sync::OnceLock::new();
    let filter = FILTER.get_or_init(SensitivePathFilter::default_rules);
    let mut current = root.to_path_buf();
    if filter.check(&current).is_some() {
        return false;
    }
    for part in relative.components() {
        let name = part.as_os_str().to_string_lossy().to_lowercase();
        if matches!(
            name.as_str(),
            ".git" | ".ssh" | ".gnupg" | ".aws" | ".azure" | ".kube" | ".gcloud" | ".docker"
        ) {
            return false;
        }
        current.push(part);
        if filter.check(&current).is_some() {
            return false;
        }
    }
    true
}

pub(crate) fn open_directory(root: &Path, relative: &Path) -> Result<Dir, String> {
    if !allowed_path(root, relative) {
        return Err("workspace path is protected".into());
    }
    let mut dir = Dir::open_ambient_dir(root, cap_std::ambient_authority())
        .map_err(|_| "workspace directory unavailable")?;
    // One component at a time, no-follow at every open. Checking canonical
    // paths then reopening by string would leave a symlink-swap race.
    for part in relative.components() {
        dir = dir
            .open_dir_nofollow(part.as_os_str())
            .map_err(|_| "workspace directory unavailable or symbolic link")?;
    }
    Ok(dir)
}

pub fn list_workspace_files(
    store: &HubStore,
    session: &str,
    path: &str,
) -> Result<WorkspaceDirectory, String> {
    let root = session_workspace(store, session)?;
    let relative = relative_path(path)?;
    let dir = open_directory(&root, &relative)?;
    let iterator = dir
        .entries()
        .map_err(|_| "workspace directory cannot be listed")?;
    let mut entries = Vec::new();
    let mut truncated = false;
    for (index, entry) in iterator.enumerate() {
        if index == MAX_ENTRIES {
            truncated = true;
            break;
        }
        let entry = entry.map_err(|_| "workspace entry unavailable")?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let child = relative.join(&name);
        if !allowed_path(&root, &child) {
            continue;
        }
        let kind = match entry.file_type() {
            Ok(kind) if kind.is_symlink() => "symlink",
            Ok(kind) if kind.is_dir() => "directory",
            Ok(kind) if kind.is_file() => "file",
            _ => "unavailable",
        };
        entries.push(WorkspaceEntry {
            name,
            path: child.to_string_lossy().into_owned(),
            kind,
        });
    }
    entries.sort_by(|a, b| (a.kind != "directory", &a.name).cmp(&(b.kind != "directory", &b.name)));
    Ok(WorkspaceDirectory {
        path: path.into(),
        entries,
        truncated,
    })
}

pub fn read_workspace_file(
    store: &HubStore,
    session: &str,
    path: &str,
) -> Result<WorkspacePreview, String> {
    let root = session_workspace(store, session)?;
    let relative = relative_path(path)?;
    if !allowed_path(&root, &relative) {
        return Err("workspace path is protected".into());
    }
    let name = relative.file_name().ok_or("select a workspace file")?;
    let dir = open_directory(&root, relative.parent().unwrap_or(Path::new("")))?;
    let mut options = OpenOptions::new();
    options.read(true).follow(FollowSymlinks::No);
    #[cfg(unix)]
    {
        use cap_std::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = dir
        .open_with(name, &options)
        .map_err(|_| "workspace file unavailable or symbolic link")?;
    let metadata = file
        .metadata()
        .map_err(|_| "workspace file metadata unavailable")?;
    if !metadata.is_file() {
        return Err("workspace preview requires a regular file".into());
    }
    let mut bytes = Vec::new();
    file.take((MAX_PREVIEW + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "workspace file cannot be read")?;
    let truncated = bytes.len() > MAX_PREVIEW;
    bytes.truncate(MAX_PREVIEW);
    // A UTF-8 character straddling the preview limit is not binary data.
    if truncated {
        if let Err(error) = std::str::from_utf8(&bytes) {
            if error.error_len().is_none() {
                bytes.truncate(error.valid_up_to());
            }
        }
    }
    let text = String::from_utf8(bytes)
        .ok()
        .filter(|text| !text.contains('\0'));
    Ok(WorkspacePreview {
        path: path.into(),
        kind: if text.is_some() { "text" } else { "binary" },
        text,
        total_bytes: metadata.len(),
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineTarget, PermissionProfile};

    fn fixture() -> (tempfile::TempDir, HubStore, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let store = HubStore::open(temp.path().join("private/hub.db")).unwrap();
        store
            .prepare_new_turn(
                "files",
                &workspace,
                "test",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        (temp, store, workspace)
    }

    #[test]
    fn nested_unicode_preview_limits_and_binary_are_truthful() {
        let (_temp, store, root) = fixture();
        std::fs::create_dir(root.join("目录")).unwrap();
        std::fs::write(
            root.join("目录/你好.txt"),
            "<script>not executed</script>🙂",
        )
        .unwrap();
        assert_eq!(
            list_workspace_files(&store, "files", "").unwrap().entries[0].kind,
            "directory"
        );
        assert_eq!(
            read_workspace_file(&store, "files", "目录/你好.txt")
                .unwrap()
                .text
                .unwrap(),
            "<script>not executed</script>🙂"
        );
        std::fs::write(root.join("large.txt"), "文".repeat(MAX_PREVIEW)).unwrap();
        let preview = read_workspace_file(&store, "files", "large.txt").unwrap();
        assert!(preview.truncated);
        assert!(preview.text.unwrap().len() <= MAX_PREVIEW);
        std::fs::write(root.join("binary"), [0, 255, 3]).unwrap();
        assert_eq!(
            read_workspace_file(&store, "files", "binary").unwrap().kind,
            "binary"
        );
    }

    #[test]
    fn missing_directory_and_listing_limit_are_explicit() {
        let (_temp, store, root) = fixture();
        assert!(list_workspace_files(&store, "files", "missing").is_err());
        for index in 0..=MAX_ENTRIES {
            std::fs::write(root.join(format!("file-{index}")), "").unwrap();
        }
        let listing = list_workspace_files(&store, "files", "").unwrap();
        assert!(listing.truncated);
        assert_eq!(listing.entries.len(), MAX_ENTRIES);
    }

    #[test]
    fn session_and_sensitive_paths_are_not_client_authorized() {
        let (_temp, store, root) = fixture();
        // Denial is checked before opening anything; no secret fixture needed.
        std::fs::create_dir(root.join(".git")).unwrap();
        for path in [
            "../outside",
            "/etc/passwd",
            ".env",
            ".git/config",
            "a/../../x",
            "a\\b",
        ] {
            assert!(
                read_workspace_file(&store, "files", path).is_err(),
                "{path}"
            );
        }
        assert!(list_workspace_files(&store, "missing", "").is_err());
        assert!(list_workspace_files(&store, "files", "")
            .unwrap()
            .entries
            .is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_file_and_directory_symlinks_and_special_files() {
        use std::os::unix::fs::symlink;
        let (temp, store, root) = fixture();
        std::fs::write(temp.path().join("outside"), "outside").unwrap();
        symlink(temp.path(), root.join("escape")).unwrap();
        symlink(temp.path().join("outside"), root.join("link")).unwrap();
        assert!(read_workspace_file(&store, "files", "link").is_err());
        assert!(read_workspace_file(&store, "files", "escape/outside").is_err());
        assert!(list_workspace_files(&store, "files", "escape").is_err());
        let socket = std::os::unix::net::UnixListener::bind(root.join("socket")).unwrap();
        assert!(read_workspace_file(&store, "files", "socket").is_err());
        drop(socket);
    }
}
