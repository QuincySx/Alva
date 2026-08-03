// INPUT:  the shared workspace path and Hub-managed worktree roots
// OUTPUT: managed git worktrees, adoption previews, and adoption/discard
// POS:    Isolation boundary for high-risk WorkerTasks. Every destructive
//         command here targets a Hub-created worktree; the shared workspace
//         is only ever read, or written by an explicit adoption.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorktreeHandle {
    pub path: String,
    pub branch: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorktreeChange {
    pub path: String,
    /// git porcelain status, e.g. "M", "A", "D", "??".
    pub status: String,
}

/// What adoption would do, shown before anything touches the shared
/// workspace.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AdoptionPreview {
    pub changes: Vec<WorktreeChange>,
    /// Files the shared workspace also modified — adopting would clobber
    /// them, so they are surfaced before the user decides.
    pub conflicts: Vec<String>,
    /// Whether the patch applies cleanly to the shared workspace right now.
    pub applies_cleanly: bool,
    pub verification: Vec<String>,
}

fn git(cwd: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(cwd)
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Is this workspace a git repository root with at least one commit?
/// Isolation needs a commit to branch from, and the workspace must be the
/// repository toplevel: a worktree always spans the whole repo, and
/// `git apply` from a subdirectory SILENTLY IGNORES paths outside it, which
/// would drop a worker's work while reporting success.
pub fn supports_isolation(workspace: &Path) -> bool {
    isolation_error(workspace).is_none()
}

fn isolation_error(workspace: &Path) -> Option<String> {
    let Ok(toplevel) = git(workspace, &["rev-parse", "--show-toplevel"]) else {
        return Some("isolated workers need a git repository".to_string());
    };
    if git(workspace, &["rev-parse", "HEAD"]).is_err() {
        return Some("isolated workers need at least one commit to branch from".to_string());
    }
    let toplevel = PathBuf::from(toplevel.trim());
    let same = match (toplevel.canonicalize(), workspace.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    };
    (!same).then(|| {
        format!(
            "isolated workers need the workspace to be the repository root ({})",
            toplevel.display()
        )
    })
}

/// Create a managed worktree for one worker, on its own branch, under the
/// Hub-private worktree root (never inside the shared workspace).
pub fn create_worker_worktree(
    workspace: &Path,
    worktree_root: &Path,
    worker_id: &str,
) -> Result<WorktreeHandle, String> {
    if let Some(error) = isolation_error(workspace) {
        return Err(error);
    }
    std::fs::create_dir_all(worktree_root)
        .map_err(|error| format!("create Hub worktree root: {error}"))?;
    let short: String = worker_id.chars().take(8).collect();
    let branch = format!("hub/worker-{short}");
    let path = worktree_root.join(format!("worker-{short}"));
    if path.exists() {
        return Err(format!(
            "worktree {} already exists; discard it first",
            path.display()
        ));
    }
    git(
        workspace,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            &path.to_string_lossy(),
            "HEAD",
        ],
    )?;
    Ok(WorktreeHandle {
        path: path.to_string_lossy().into_owned(),
        branch,
    })
}

/// Everything the worker changed inside its worktree, tracked or not.
pub fn worktree_changes(worktree: &Path) -> Result<Vec<WorktreeChange>, String> {
    let porcelain = git(worktree, &["status", "--porcelain"])?;
    Ok(porcelain
        .lines()
        .filter_map(|line| {
            let (status, path) = line.split_at(line.char_indices().nth(3)?.0);
            Some(WorktreeChange {
                path: path.trim().to_string(),
                status: status.trim().to_string(),
            })
        })
        .collect())
}

/// Read-only: what adoption would change, which files the shared workspace
/// also touched, and whether the patch still applies. Runs no mutation.
pub fn adoption_preview(
    workspace: &Path,
    worktree: &Path,
    verification: Vec<String>,
) -> Result<AdoptionPreview, String> {
    let changes = worktree_changes(worktree)?;
    let shared_dirty: Vec<String> = worktree_changes(workspace)?
        .into_iter()
        .map(|change| change.path)
        .collect();
    let conflicts = changes
        .iter()
        .filter(|change| shared_dirty.contains(&change.path))
        .map(|change| change.path.clone())
        .collect::<Vec<_>>();
    let applies_cleanly = match build_patch(worktree) {
        Ok(patch) if patch.trim().is_empty() => changes.is_empty(),
        // --check never writes; it only reports whether apply would succeed.
        Ok(patch) => apply_patch(workspace, &patch, true).is_ok(),
        Err(_) => false,
    };
    Ok(AdoptionPreview {
        changes,
        conflicts,
        applies_cleanly,
        verification,
    })
}

/// Apply the worktree's work to the shared workspace. The ONLY function
/// here that writes to the shared workspace, and only when called after an
/// explicit user/lead adoption.
pub fn adopt_into_workspace(workspace: &Path, worktree: &Path) -> Result<Vec<String>, String> {
    if worktree_changes(worktree)?.is_empty() {
        return Ok(Vec::new());
    }
    let patch = build_patch(worktree)?;
    if patch.trim().is_empty() {
        return Err("the worker produced no adoptable diff".to_string());
    }
    apply_patch(workspace, &patch, true)
        .map_err(|error| format!("adoption would not apply cleanly: {error}"))?;
    // What the patch actually carries, resolved BEFORE applying, so the
    // reported list can never claim work that was silently skipped.
    let adopted = patch_paths(workspace, &patch)?;
    apply_patch(workspace, &patch, false)?;
    Ok(adopted)
}

/// Remove a managed worktree and its branch. Never touches the shared
/// workspace: the path is validated to be the Hub-created worktree.
pub fn discard_worktree(
    workspace: &Path,
    worktree: &Path,
    branch: &str,
    managed_root: &Path,
) -> Result<(), String> {
    // `git worktree remove --force` happily deletes ANY linked worktree of
    // the repo, uncommitted work included. Only ever aim it at a path we
    // created: canonicalized, inside the Hub root, on a hub/worker-* branch.
    if !branch.starts_with("hub/worker-") {
        return Err(format!("refusing to discard unmanaged branch '{branch}'"));
    }
    let canonical_workspace = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.into());
    let canonical_root = managed_root
        .canonicalize()
        .unwrap_or_else(|_| managed_root.into());
    let canonical_worktree = match worktree.canonicalize() {
        Ok(path) => path,
        // Already gone: nothing to remove, and nothing to endanger.
        Err(_) => return Ok(()),
    };
    if canonical_worktree == canonical_workspace {
        return Err("refusing to discard the shared workspace".to_string());
    }
    if !canonical_worktree.starts_with(&canonical_root) {
        return Err(format!(
            "refusing to discard {} — it is not a Hub-managed worktree",
            canonical_worktree.display()
        ));
    }
    let worktree = canonical_worktree.as_path();
    if !worktree.exists() {
        return Ok(());
    }
    git(
        workspace,
        &["worktree", "remove", "--force", &worktree.to_string_lossy()],
    )?;
    // Branch deletion is best-effort: the worktree is already gone.
    let _ = git(workspace, &["branch", "-D", branch]);
    Ok(())
}

/// Tracked edits plus untracked files, as one patch. Uses a THROWAWAY index
/// (GIT_INDEX_FILE) so neither previewing nor adopting mutates the worker's
/// own index while it may still be running.
fn build_patch(worktree: &Path) -> Result<String, String> {
    let index_path = std::env::temp_dir().join(format!("alva-hub-index-{}", uuid::Uuid::new_v4()));
    let _ = std::fs::remove_file(&index_path);
    let result = (|| {
        git_with_index(worktree, &index_path, &["read-tree", "HEAD"])?;
        git_with_index(worktree, &index_path, &["add", "--intent-to-add", "--all"])?;
        git_with_index(worktree, &index_path, &["diff", "HEAD", "--binary"])
    })();
    let _ = std::fs::remove_file(&index_path);
    result
}

fn git_with_index(cwd: &Path, index: &Path, arguments: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(cwd)
        .env("GIT_INDEX_FILE", index)
        .output()
        .map_err(|error| format!("failed to run git: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Files the patch itself touches — the truth about what adoption moved,
/// as opposed to what `git status` merely listed.
fn patch_paths(workspace: &Path, patch: &str) -> Result<Vec<String>, String> {
    let numstat = apply_capture(workspace, patch, &["--numstat"])?;
    Ok(numstat
        .lines()
        .filter_map(|line| line.split(char::is_whitespace).nth(2))
        .map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
        .collect())
}

fn apply_patch(workspace: &Path, patch: &str, check_only: bool) -> Result<(), String> {
    let extra: &[&str] = if check_only { &["--check"] } else { &[] };
    apply_capture(workspace, patch, extra).map(|_| ())
}

/// Run `git apply` with the patch on stdin, returning its stdout.
fn apply_capture(workspace: &Path, patch: &str, extra: &[&str]) -> Result<String, String> {
    use std::io::Write;
    use std::process::Stdio;

    let mut arguments = vec!["apply"];
    arguments.extend_from_slice(extra);
    let mut child = Command::new("git")
        .args(&arguments)
        .current_dir(workspace)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to run git apply: {error}"))?;
    child
        .stdin
        .as_mut()
        .ok_or("git apply stdin unavailable")?
        .write_all(patch.as_bytes())
        .map_err(|error| format!("failed to send patch: {error}"))?;
    let output = child
        .wait_with_output()
        .map_err(|error| format!("failed to wait for git apply: {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Hub-private root holding every managed worktree.
pub fn worktree_root(data_dir: &Path) -> PathBuf {
    data_dir.join("worktrees")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        git(path, &["init"]).unwrap();
        git(path, &["config", "user.email", "hub@example.com"]).unwrap();
        git(path, &["config", "user.name", "Hub"]).unwrap();
        // The developer's global config may sign commits; test repos must not.
        git(path, &["config", "commit.gpgsign", "false"]).unwrap();
        std::fs::write(path.join("shared.txt"), "original\n").unwrap();
        git(path, &["add", "."]).unwrap();
        git(path, &["commit", "-m", "base"]).unwrap();
        dir
    }

    #[test]
    fn isolated_work_never_reaches_the_shared_workspace_until_adoption() {
        if !git_available() {
            return;
        }
        let shared = repo();
        let roots = tempfile::tempdir().unwrap();
        let handle =
            create_worker_worktree(shared.path(), roots.path(), "worker-abcdef123").unwrap();
        let worktree = PathBuf::from(&handle.path);
        // The worktree is outside the shared workspace.
        assert!(!worktree.starts_with(shared.path()));

        // The worker edits and adds files in isolation.
        std::fs::write(worktree.join("shared.txt"), "worker edit\n").unwrap();
        std::fs::write(worktree.join("new.txt"), "brand new\n").unwrap();

        // Shared workspace is untouched: same content, no new file.
        assert_eq!(
            std::fs::read_to_string(shared.path().join("shared.txt")).unwrap(),
            "original\n"
        );
        assert!(!shared.path().join("new.txt").exists());
        assert!(worktree_changes(shared.path()).unwrap().is_empty());

        // The preview describes the work without applying it.
        let preview =
            adoption_preview(shared.path(), &worktree, vec!["tests pass".into()]).unwrap();
        let changed: Vec<_> = preview
            .changes
            .iter()
            .map(|change| change.path.as_str())
            .collect();
        assert!(changed.contains(&"shared.txt"));
        assert!(changed.contains(&"new.txt"));
        assert!(preview.conflicts.is_empty());
        assert!(preview.applies_cleanly);
        assert_eq!(preview.verification, vec!["tests pass".to_string()]);
        // Still untouched after previewing.
        assert_eq!(
            std::fs::read_to_string(shared.path().join("shared.txt")).unwrap(),
            "original\n"
        );

        // Explicit adoption is what moves the work across.
        let adopted = adopt_into_workspace(shared.path(), &worktree).unwrap();
        assert!(adopted.contains(&"shared.txt".to_string()));
        assert_eq!(
            std::fs::read_to_string(shared.path().join("shared.txt")).unwrap(),
            "worker edit\n"
        );
        assert_eq!(
            std::fs::read_to_string(shared.path().join("new.txt")).unwrap(),
            "brand new\n"
        );
    }

    #[test]
    fn preview_surfaces_conflicts_with_concurrent_shared_edits() {
        if !git_available() {
            return;
        }
        let shared = repo();
        let roots = tempfile::tempdir().unwrap();
        let handle = create_worker_worktree(shared.path(), roots.path(), "conflict1").unwrap();
        let worktree = PathBuf::from(&handle.path);
        std::fs::write(worktree.join("shared.txt"), "worker version\n").unwrap();
        // Somebody edited the same file in the shared workspace.
        std::fs::write(shared.path().join("shared.txt"), "human version\n").unwrap();

        let preview = adoption_preview(shared.path(), &worktree, Vec::new()).unwrap();
        assert_eq!(preview.conflicts, vec!["shared.txt".to_string()]);
        assert!(!preview.applies_cleanly);
        // A refused adoption leaves the human's edit intact.
        assert!(adopt_into_workspace(shared.path(), &worktree).is_err());
        assert_eq!(
            std::fs::read_to_string(shared.path().join("shared.txt")).unwrap(),
            "human version\n"
        );
    }

    #[test]
    fn discarding_a_worktree_never_deletes_shared_workspace_data() {
        if !git_available() {
            return;
        }
        let shared = repo();
        let roots = tempfile::tempdir().unwrap();
        let handle = create_worker_worktree(shared.path(), roots.path(), "discard1").unwrap();
        let worktree = PathBuf::from(&handle.path);
        std::fs::write(worktree.join("throwaway.txt"), "experiment\n").unwrap();

        discard_worktree(shared.path(), &worktree, &handle.branch, roots.path()).unwrap();
        assert!(!worktree.exists());
        // The shared workspace kept every file, unchanged.
        assert_eq!(
            std::fs::read_to_string(shared.path().join("shared.txt")).unwrap(),
            "original\n"
        );
        assert!(!shared.path().join("throwaway.txt").exists());
        // Discarding is idempotent, and never targets the workspace itself.
        discard_worktree(shared.path(), &worktree, &handle.branch, roots.path()).unwrap();
        assert!(discard_worktree(shared.path(), shared.path(), "main", roots.path()).is_err());
        assert!(shared.path().join("shared.txt").exists());
    }

    #[test]
    fn discard_refuses_paths_and_branches_it_did_not_create() {
        if !git_available() {
            return;
        }
        let shared = repo();
        let roots = tempfile::tempdir().unwrap();
        // A worktree the USER made, outside the Hub root, with unsaved work.
        let user_root = tempfile::tempdir().unwrap();
        let user_worktree = user_root.path().join("mine");
        git(
            shared.path(),
            &[
                "worktree",
                "add",
                "-b",
                "mine",
                &user_worktree.to_string_lossy(),
                "HEAD",
            ],
        )
        .unwrap();
        std::fs::write(user_worktree.join("notes.txt"), "my uncommitted work\n").unwrap();

        // Not under the Hub root, and not a hub/worker-* branch: refused.
        assert!(discard_worktree(shared.path(), &user_worktree, "mine", roots.path()).is_err());
        assert!(discard_worktree(
            shared.path(),
            &user_worktree,
            "hub/worker-forged",
            roots.path()
        )
        .is_err());
        assert!(user_worktree.join("notes.txt").exists());
        assert_eq!(
            std::fs::read_to_string(user_worktree.join("notes.txt")).unwrap(),
            "my uncommitted work\n"
        );
    }

    #[test]
    fn a_subdirectory_workspace_cannot_be_isolated() {
        if !git_available() {
            return;
        }
        let shared = repo();
        let subdir = shared.path().join("crate-a");
        std::fs::create_dir_all(&subdir).unwrap();
        let roots = tempfile::tempdir().unwrap();
        // git works from the subdir, but adoption would silently drop
        // repo-root files — so isolation is refused outright.
        assert!(git(&subdir, &["rev-parse", "HEAD"]).is_ok());
        assert!(!supports_isolation(&subdir));
        let error = create_worker_worktree(&subdir, roots.path(), "w1").unwrap_err();
        assert!(error.contains("repository root"), "{error}");
    }

    #[test]
    fn preview_does_not_mutate_the_workers_index() {
        if !git_available() {
            return;
        }
        let shared = repo();
        let roots = tempfile::tempdir().unwrap();
        let handle = create_worker_worktree(shared.path(), roots.path(), "readonly1").unwrap();
        let worktree = PathBuf::from(&handle.path);
        std::fs::write(worktree.join("untracked.txt"), "new\n").unwrap();
        let before = git(&worktree, &["status", "--porcelain"]).unwrap();
        let _ = adoption_preview(shared.path(), &worktree, Vec::new()).unwrap();
        // The worker may still be running; its index must be untouched.
        assert_eq!(git(&worktree, &["status", "--porcelain"]).unwrap(), before);
    }

    #[test]
    fn isolation_requires_a_repository_with_a_commit() {
        if !git_available() {
            return;
        }
        let empty = tempfile::tempdir().unwrap();
        let roots = tempfile::tempdir().unwrap();
        assert!(!supports_isolation(empty.path()));
        let error = create_worker_worktree(empty.path(), roots.path(), "w1").unwrap_err();
        assert!(error.contains("git repository"));
    }
}
