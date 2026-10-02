// INPUT:  a project workspace path
// OUTPUT: read-only worktree discovery and unwrapped WorktreeDiscoveryReply IPC DTO
// POS:    H05. Projects aggregate the worktrees the USER already has; this
//         module only ever runs read-only git plumbing, never add/remove/
//         prune/lock. Hub-managed worker worktrees (worktree.rs, hub/worker-*)
//         are labelled so the product can distinguish user worktrees from
//         Hub-created ones — discovery is NOT worker registration.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: String,
    pub head: Option<String>,
    /// Branch name as git reports it (e.g. "refs/heads/main"); null when
    /// detached. This is a GIT branch — distinct from an EngineBranch.
    pub branch: Option<String>,
    pub detached: bool,
    pub bare: bool,
    pub is_main_worktree: bool,
    pub locked: Option<String>,
    pub prunable: Option<String>,
    /// True when the worktree is one Hub created for an isolated worker.
    pub managed_by_hub: bool,
    /// True when the directory still exists on disk (git keeps prunable
    /// rows for deleted directories).
    pub exists_on_disk: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum WorktreeDiscoveryError {
    /// Not a failure for a project: non-Git directories are valid projects
    /// with an empty worktree list.
    NotGitRepository,
    /// The configured workspace directory is missing (failed drive, moved
    /// project). The UI offers a repair path; discovery never guesses.
    MissingDirectory { path: String },
    Git { detail: String },
}

/// The IPC success body is an array or a structured product state, never a
/// nested Rust Result's externally tagged {"Ok": ...}/{"Err": ...} envelope.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum WorktreeDiscoveryReply {
    Worktrees(Vec<WorktreeInfo>),
    State(WorktreeDiscoveryError),
}

impl From<Result<Vec<WorktreeInfo>, WorktreeDiscoveryError>> for WorktreeDiscoveryReply {
    fn from(result: Result<Vec<WorktreeInfo>, WorktreeDiscoveryError>) -> Self {
        match result { Ok(worktrees) => Self::Worktrees(worktrees), Err(state) => Self::State(state) }
    }
}

impl WorktreeDiscoveryError {
    pub fn message(&self) -> String {
        match self {
            Self::NotGitRepository => "not a git repository".into(),
            Self::MissingDirectory { path } => format!("workspace directory is missing: {path}"),
            Self::Git { detail } => format!("git worktree discovery failed: {detail}"),
        }
    }
}

/// Read-only enumeration of one repository's worktrees. Pass the workspace
/// itself (the main worktree is part of the result, marked
/// `is_main_worktree`).
pub fn discover_worktrees(
    workspace: &Path,
    hub_worktree_root: Option<&Path>,
) -> Result<Vec<WorktreeInfo>, WorktreeDiscoveryError> {
    let metadata = std::fs::symlink_metadata(workspace).map_err(|error| {
        WorktreeDiscoveryError::MissingDirectory {
            path: workspace.display().to_string(),
        }
    })?;
    if metadata.is_symlink() || !metadata.is_dir() {
        return Err(WorktreeDiscoveryError::MissingDirectory {
            path: workspace.display().to_string(),
        });
    }

    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(workspace)
        .output()
        .map_err(|error| WorktreeDiscoveryError::Git {
            detail: format!("failed to run git: {error}"),
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // "not a git repository" (and worktree-list's own variant) is an
        // expected state for a non-Git project, not an error to surface.
        if stderr.contains("not a git repository") {
            return Err(WorktreeDiscoveryError::NotGitRepository);
        }
        return Err(WorktreeDiscoveryError::Git {
            detail: stderr.trim().to_string(),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut worktrees = parse_worktree_list(&stdout);

    // Canonicalize inside the Hub root check only — user paths stay as git
    // reported them so the UI shows what git shows.
    for worktree in &mut worktrees {
        worktree.managed_by_hub = is_hub_managed(worktree, hub_worktree_root);
        worktree.exists_on_disk = PathBuf::from(&worktree.path).symlink_metadata().is_ok();
    }
    // `worktree list` output is already deduplicated by git; sort for stable
    // UI ordering (main first, then alphabetical).
    worktrees.sort_by(|left, right| {
        right
            .is_main_worktree
            .cmp(&left.is_main_worktree)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(worktrees)
}

fn is_hub_managed(worktree: &WorktreeInfo, hub_worktree_root: Option<&Path>) -> bool {
    if worktree.branch.as_deref().is_some_and(|branch| branch.starts_with("refs/heads/hub/worker-")) {
        return true;
    }
    let Some(root) = hub_worktree_root else {
        return false;
    };
    let path = PathBuf::from(&worktree.path);
    let canonical = match (path.canonicalize(), root.canonicalize()) {
        (Ok(path), Ok(root)) => path.starts_with(&root),
        _ => false,
    };
    canonical
}

/// Parse `git worktree list --porcelain` records. Paths are NOT quoted in
/// porcelain output, so spaces and non-ASCII survive verbatim; every
/// attribute line is bounded, unknown lines are ignored (forward-compatible).
fn parse_worktree_list(output: &str) -> Vec<WorktreeInfo> {
    let mut worktrees: Vec<WorktreeInfo> = Vec::new();
    let mut current: Option<WorktreeInfo> = None;
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            if let Some(previous) = current.take() {
                worktrees.push(previous);
            }
            current = Some(WorktreeInfo {
                path: path.to_string(),
                head: None,
                branch: None,
                detached: false,
                bare: false,
                // The first record is the main worktree (git documents this
                // order; it is how `worktree list` prints it).
                is_main_worktree: worktrees.is_empty(),
                locked: None,
                prunable: None,
                managed_by_hub: false,
                exists_on_disk: true,
            });
            continue;
        }
        let Some(entry) = current.as_mut() else { continue };
        if let Some(head) = line.strip_prefix("HEAD ") {
            entry.head = Some(head.to_string());
        } else if let Some(branch) = line.strip_prefix("branch ") {
            entry.branch = Some(branch.to_string());
        } else if line == "detached" {
            entry.detached = true;
        } else if line == "bare" {
            entry.bare = true;
        } else if let Some(reason) = line.strip_prefix("locked") {
            entry.locked = Some(reason.trim().to_string());
        } else if let Some(reason) = line.strip_prefix("prunable") {
            entry.prunable = Some(reason.trim().to_string());
        }
    }
    if let Some(previous) = current.take() {
        worktrees.push(previous);
    }
    worktrees
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

    fn repo_with_commit(path: &Path) {
        Command::new("git").args(["init"]).current_dir(path).status().unwrap();
        Command::new("git").args(["config", "user.email", "hub@example.com"]).current_dir(path).status().unwrap();
        Command::new("git").args(["config", "user.name", "Hub"]).current_dir(path).status().unwrap();
        Command::new("git").args(["config", "commit.gpgsign", "false"]).current_dir(path).status().unwrap();
        std::fs::write(path.join("file.txt"), "content\n").unwrap();
        Command::new("git").args(["add", "."]).current_dir(path).status().unwrap();
        Command::new("git").args(["commit", "-m", "base"]).current_dir(path).status().unwrap();
    }

    #[test]
    fn porcelain_parser_reads_paths_with_spaces_and_unicode() {
        let parsed = parse_worktree_list(
            "worktree /repo/main\nHEAD abcdef1234567890\nbranch refs/heads/main\n\n\
             worktree /Users/dev/My Docs/工作树 feature\nHEAD 0123456789abcdef\ndetached\n\n",
        );
        assert_eq!(parsed.len(), 2);
        assert!(parsed[0].is_main_worktree);
        assert_eq!(parsed[0].branch.as_deref(), Some("refs/heads/main"));
        assert!(!parsed[0].detached);
        assert_eq!(parsed[1].path, "/Users/dev/My Docs/工作树 feature");
        assert!(parsed[1].detached);
        assert!(parsed[1].branch.is_none());
        assert!(!parsed[1].is_main_worktree);
    }

    #[test]
    fn parser_captures_locked_prunable_and_bare_attributes() {
        let parsed = parse_worktree_list(
            "worktree /repo\nHEAD abc\nbranch refs/heads/main\n\n\
             worktree /repo/bare\nbare\n\n\
             worktree /gone\nHEAD def\nbranch refs/heads/old\nlocked clean-up pending\nprunable stale dir\n\n",
        );
        assert_eq!(parsed.len(), 3);
        assert!(parsed[1].bare);
        assert_eq!(parsed[2].locked.as_deref(), Some("clean-up pending"));
        assert_eq!(parsed[2].prunable.as_deref(), Some("stale dir"));
    }

    #[test]
    fn discovers_main_and_linked_user_worktrees_read_only() {
        if !git_available() {
            return;
        }
        let parent = tempfile::tempdir().unwrap();
        let main = parent.path().join("main repo");
        std::fs::create_dir(&main).unwrap();
        repo_with_commit(&main);

        // A USER-linked worktree with a space + unicode path, outside Hub roots.
        let linked = parent.path().join("linked 工作树");
        Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                "feature/x",
                &linked.to_string_lossy(),
                "HEAD",
            ])
            .current_dir(&main)
            .status()
            .unwrap();

        let discovered = discover_worktrees(&main, None).unwrap();
        assert_eq!(discovered.len(), 2, "{discovered:?}");
        assert!(discovered[0].is_main_worktree);
        let main_branch = discovered[0].branch.as_deref().expect("main worktree has a branch");
        assert!(
            main_branch == "refs/heads/master" || main_branch == "refs/heads/main",
            "unexpected main branch {main_branch}"
        );
        let linked_info = discovered
            .iter()
            .find(|info| info.path.ends_with("linked 工作树"))
            .expect("linked worktree listed verbatim");
        assert_eq!(linked_info.branch.as_deref(), Some("refs/heads/feature/x"));
        assert!(!linked_info.managed_by_hub);
        assert!(linked_info.exists_on_disk);

        // Read-only: discovery never prunes, locks, or removes anything.
        let again = discover_worktrees(&main, None).unwrap();
        assert_eq!(again.len(), discovered.len());
        assert!(linked.exists());
    }

    #[test]
    fn labels_hub_managed_worker_worktrees() {
        if !git_available() {
            return;
        }
        let parent = tempfile::tempdir().unwrap();
        let main = parent.path().join("main");
        std::fs::create_dir(&main).unwrap();
        repo_with_commit(&main);

        let hub_root = parent.path().join("hub-worktrees");
        std::fs::create_dir(&hub_root).unwrap();
        Command::new("git")
            .args([
                "worktree",
                "add",
                "-b",
                "hub/worker-abcd1234",
                &hub_root.join("worker-abcd1234").to_string_lossy(),
                "HEAD",
            ])
            .current_dir(&main)
            .status()
            .unwrap();

        let discovered = discover_worktrees(&main, Some(&hub_root)).unwrap();
        let managed = discovered
            .iter()
            .find(|info| info.managed_by_hub)
            .expect("hub worker worktree labelled managed");
        assert!(managed.branch.as_deref().unwrap().starts_with("refs/heads/hub/worker-"));
        // The user's main worktree stays unmanaged.
        assert!(!discovered[0].managed_by_hub);
    }

    #[test]
    fn reports_detached_head_worktrees() {
        if !git_available() {
            return;
        }
        let parent = tempfile::tempdir().unwrap();
        let main = parent.path().join("main");
        std::fs::create_dir(&main).unwrap();
        repo_with_commit(&main);
        let linked = parent.path().join("detached-wt");
        Command::new("git")
            .args(["worktree", "add", "--detach", &linked.to_string_lossy(), "HEAD"])
            .current_dir(&main)
            .status()
            .unwrap();

        let discovered = discover_worktrees(&main, None).unwrap();
        let detached = discovered
            .iter()
            .find(|info| info.path.ends_with("detached-wt"))
            .unwrap();
        assert!(detached.detached);
        assert!(detached.branch.is_none());
    }

    #[test]
    fn non_git_directory_and_missing_directory_have_explicit_states() {
        let plain = tempfile::tempdir().unwrap();
        let error = discover_worktrees(plain.path(), None).unwrap_err();
        assert_eq!(error, WorktreeDiscoveryError::NotGitRepository);

        let missing = plain.path().join("does-not-exist");
        let error = discover_worktrees(&missing, None).unwrap_err();
        assert!(matches!(error, WorktreeDiscoveryError::MissingDirectory { .. }));
    }

    #[test]
    fn ipc_reply_is_an_array_or_structured_state_without_result_wrapping() {
        let success = WorktreeDiscoveryReply::from(Ok(Vec::new()));
        assert_eq!(serde_json::to_value(success).unwrap(), serde_json::json!([]));
        let missing = WorktreeDiscoveryReply::from(Err(WorktreeDiscoveryError::MissingDirectory { path: "/tmp/missing-fixture".into() }));
        assert_eq!(serde_json::to_value(missing).unwrap(), serde_json::json!({ "code": "missing_directory", "path": "/tmp/missing-fixture" }));
        let plain = WorktreeDiscoveryReply::from(Err(WorktreeDiscoveryError::NotGitRepository));
        assert_eq!(serde_json::to_value(plain).unwrap(), serde_json::json!({ "code": "not_git_repository" }));
    }
}
