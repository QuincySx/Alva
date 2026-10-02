// INPUT: Saved session workspace, bounded read-only Git subprocesses, workspace path rules
// OUTPUT: workspace_git_status/workspace_git_diff and product Git DTOs
// POS: Hub worktree inspection, never inferred from engine branches or tool success.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;

use crate::workspace_files::{allowed_path, relative_path, session_workspace};
use crate::HubStore;

const STATUS_LIMIT: usize = 1024 * 1024;
const DIFF_LIMIT: usize = 256 * 1024;

#[derive(Debug, Serialize)]
pub struct GitFileStatus {
    pub path: String,
    pub original_path: Option<String>,
    pub index: String,
    pub worktree: String,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceGitStatus {
    pub is_repository: bool,
    pub branch: Option<String>,
    pub files: Vec<GitFileStatus>,
    pub hidden_paths: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GitDiffArea {
    Staged,
    Unstaged,
    Untracked,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceGitDiff {
    pub path: String,
    pub area: GitDiffArea,
    pub text: String,
    pub truncated: bool,
}

struct GitOutput {
    success: bool,
    bytes: Vec<u8>,
    truncated: bool,
}

async fn git(root: &Path, args: &[&str], limit: usize) -> Result<GitOutput, String> {
    // No inherited GIT_DIR/GIT_INDEX_FILE/GIT_EXTERNAL_DIFF or user-wide
    // config. Repo-defined textconv/external diff/fsmonitor cannot execute.
    let mut child = tokio::process::Command::new("git")
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/local/bin")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .args([
            "--no-pager",
            "--no-optional-locks",
            "--literal-pathspecs",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Git executable or workspace unavailable")?;
    let stdout = child.stdout.take().ok_or("Git output unavailable")?;
    let work = async {
        let mut bytes = Vec::new();
        stdout
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| "Git output cannot be read")?;
        let truncated = bytes.len() > limit;
        if truncated {
            let _ = child.kill().await;
            bytes.truncate(limit);
            return Ok(GitOutput {
                success: true,
                bytes,
                truncated,
            });
        }
        let status = child.wait().await.map_err(|_| "Git process unavailable")?;
        Ok(GitOutput {
            success: status.success(),
            bytes,
            truncated,
        })
    };
    match tokio::time::timeout(Duration::from_secs(10), work).await {
        Ok(result) => result,
        Err(_) => {
            let _ = child.kill().await;
            Err("Git inspection timed out".into())
        }
    }
}

pub async fn workspace_git_status(
    store: &HubStore,
    session: &str,
) -> Result<WorkspaceGitStatus, String> {
    let root = session_workspace(store, session)?;
    let probe = git(&root, &["rev-parse", "--is-inside-work-tree"], 128).await?;
    if !probe.success || probe.bytes != b"true\n" {
        return Ok(WorkspaceGitStatus {
            is_repository: false,
            branch: None,
            files: vec![],
            hidden_paths: false,
        });
    }
    let prefix = git(&root, &["rev-parse", "--show-prefix"], 4096).await?;
    if !prefix.success || prefix.truncated {
        return Err("Git workspace prefix unavailable".into());
    }
    let prefix = String::from_utf8(prefix.bytes).map_err(|_| "Git workspace is not UTF-8")?;
    let prefix = prefix.strip_suffix('\n').unwrap_or(&prefix);
    let branch = git(&root, &["symbolic-ref", "--quiet", "--short", "HEAD"], 4096).await?;
    let branch = if branch.success {
        Some(
            String::from_utf8_lossy(&branch.bytes)
                .trim_end()
                .to_string(),
        )
    } else {
        None
    };
    let output = git(
        &root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=all",
            "--",
            ".",
        ],
        STATUS_LIMIT,
    )
    .await?;
    if !output.success {
        return Err("Git status unavailable".into());
    }
    if output.truncated {
        return Err("Git status exceeds the 1 MiB limit".into());
    }
    let (files, hidden_paths) = parse_status(&root, prefix, &output.bytes)?;
    Ok(WorkspaceGitStatus {
        is_repository: true,
        branch,
        files,
        hidden_paths,
    })
}

fn parse_status(
    root: &Path,
    prefix: &str,
    bytes: &[u8],
) -> Result<(Vec<GitFileStatus>, bool), String> {
    let mut records = bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty());
    let mut files = Vec::new();
    let mut hidden = false;
    while let Some(record) = records.next() {
        if record.len() < 4 || record[2] != b' ' {
            return Err("invalid Git status record".into());
        }
        let rename = matches!(record[0], b'R' | b'C') || matches!(record[1], b'R' | b'C');
        let original = if rename {
            Some(records.next().ok_or("incomplete Git rename record")?)
        } else {
            None
        };
        let visible = |raw: &[u8]| -> Option<String> {
            let path = std::str::from_utf8(raw).ok()?.strip_prefix(prefix)?;
            let relative = relative_path(path).ok()?;
            if !allowed_path(root, &relative) {
                return None;
            }
            Some(path.to_string())
        };
        let path = visible(&record[3..]);
        let original_path = original.and_then(visible);
        if path.is_none() || (rename && original_path.is_none()) {
            hidden = true;
            continue;
        }
        files.push(GitFileStatus {
            path: path.unwrap(),
            original_path,
            index: (record[0] as char).to_string(),
            worktree: (record[1] as char).to_string(),
        });
        if files.len() > 2000 { return Err("Git status exceeds the 2000 path limit".into()); }
    }
    Ok((files, hidden))
}

pub async fn workspace_git_diff(
    store: &HubStore,
    session: &str,
    path: &str,
    area: GitDiffArea,
) -> Result<WorkspaceGitDiff, String> {
    let root = session_workspace(store, session)?;
    let relative = relative_path(path)?;
    if relative.as_os_str().is_empty() || !allowed_path(&root, &relative) {
        return Err("Git path is protected or invalid".into());
    }
    // Validate against the fresh worktree snapshot. This also checks BOTH sides
    // of a rename so a protected source cannot leak through an allowed target.
    let status = workspace_git_status(store, session).await?;
    let entry = status
        .files
        .iter()
        .find(|entry| entry.path == path)
        .ok_or("Git path no longer changed; refresh")?;
    let eligible = match area {
        GitDiffArea::Staged => entry.index != " " && entry.index != "?",
        GitDiffArea::Unstaged => entry.worktree != " " && entry.worktree != "?",
        GitDiffArea::Untracked => entry.index == "?",
    };
    if !eligible {
        return Err("Git change area no longer available; refresh".into());
    }
    if matches!(area, GitDiffArea::Untracked) {
        let store = store.clone();
        let session = session.to_string();
        let path_owned = path.to_string();
        let preview = tokio::task::spawn_blocking(move || {
            crate::read_workspace_file(&store, &session, &path_owned)
        })
        .await
        .map_err(|_| "file reader unavailable")??;
        return Ok(WorkspaceGitDiff {
            path: path.into(),
            area,
            text: preview
                .text
                .unwrap_or_else(|| "Binary or non-UTF-8 file".into()),
            truncated: preview.truncated,
        });
    }
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--ignore-submodules=all",
        "--no-renames",
    ];
    if matches!(area, GitDiffArea::Staged) {
        args.push("--cached");
    }
    args.extend(["--", path]);
    if let Some(original) = entry.original_path.as_deref() {
        args.push(original);
    }
    let output = git(&root, &args, DIFF_LIMIT).await?;
    if !output.success {
        return Err("Git diff unavailable".into());
    }
    Ok(WorkspaceGitDiff {
        path: path.into(),
        area,
        text: String::from_utf8_lossy(&output.bytes).into_owned(),
        truncated: output.truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EngineTarget, PermissionProfile};

    fn command(root: &Path, args: &[&str]) {
        let result = std::process::Command::new("git")
            .current_dir(root)
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(result.status.success(), "Git fixture failed: {:?}", args);
    }

    fn session(store: &HubStore, id: &str, root: &Path) {
        store
            .prepare_new_turn(
                id,
                root,
                "test",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
    }

    #[tokio::test]
    async fn real_worktrees_have_independent_staged_unstaged_and_untracked_views() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir(&root).unwrap();
        command(&root, &["init"]);
        std::fs::write(root.join("tracked.txt"), "base\n").unwrap();
        std::fs::write(root.join("rename-source.txt"), "stable\n").unwrap();
        command(&root, &["add", "tracked.txt", "rename-source.txt"]);
        command(&root, &["commit", "-m", "fixture"]);
        let linked = temp.path().join("linked");
        command(
            &root,
            &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
        );
        let store = HubStore::open(temp.path().join("private/hub.db")).unwrap();
        session(&store, "main", &root);
        session(&store, "linked", &linked);
        std::fs::write(root.join("tracked.txt"), "staged\n").unwrap();
        command(&root, &["add", "tracked.txt"]);
        std::fs::write(root.join("tracked.txt"), "unstaged\n").unwrap();
        std::fs::write(root.join("新 file.txt"), "new file\n").unwrap();
        let status = workspace_git_status(&store, "main").await.unwrap();
        assert_eq!(status.files.len(), 2);
        assert!(status
            .files
            .iter()
            .any(|file| file.index == "M" && file.worktree == "M"));
        assert!(workspace_git_status(&store, "linked")
            .await
            .unwrap()
            .files
            .is_empty());
        assert!(
            workspace_git_diff(&store, "main", "tracked.txt", GitDiffArea::Staged)
                .await
                .unwrap()
                .text
                .contains("+staged")
        );
        assert!(
            workspace_git_diff(&store, "main", "tracked.txt", GitDiffArea::Unstaged)
                .await
                .unwrap()
                .text
                .contains("+unstaged")
        );
        assert_eq!(
            workspace_git_diff(&store, "main", "新 file.txt", GitDiffArea::Untracked)
                .await
                .unwrap()
                .text,
            "new file\n"
        );
        assert!(
            workspace_git_diff(&store, "linked", "tracked.txt", GitDiffArea::Staged)
                .await
                .is_err()
        );
        assert!(
            workspace_git_diff(&store, "main", "../outside", GitDiffArea::Staged)
                .await
                .is_err()
        );
        command(&root, &["mv", "rename-source.txt", "renamed.txt"]);
        let renamed = workspace_git_status(&store, "main").await.unwrap();
        assert!(renamed
            .files
            .iter()
            .any(|file| file.original_path.as_deref() == Some("rename-source.txt")));
    }

    #[tokio::test]
    async fn non_git_is_a_state_and_output_is_bounded() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("workspace");
        std::fs::create_dir(&root).unwrap();
        let store = HubStore::open(temp.path().join("private/hub.db")).unwrap();
        session(&store, "plain", &root);
        assert!(
            !workspace_git_status(&store, "plain")
                .await
                .unwrap()
                .is_repository
        );
        command(&root, &["init"]);
        std::fs::write(root.join("large"), "base\n").unwrap();
        command(&root, &["add", "large"]);
        command(&root, &["commit", "-m", "fixture"]);
        std::fs::write(root.join("large"), "large line\n".repeat(DIFF_LIMIT)).unwrap();
        let diff = workspace_git_diff(&store, "plain", "large", GitDiffArea::Unstaged)
            .await
            .unwrap();
        assert!(diff.truncated);
        assert!(diff.text.len() <= DIFF_LIMIT);
    }

    #[test]
    fn nul_status_parser_preserves_names_and_hides_protected_rename_sources() {
        let temp = tempfile::tempdir().unwrap();
        let (files, hidden) =
            parse_status(temp.path(), "", b"R  visible\0.env\0?? normal\nname\0").unwrap();
        assert!(hidden);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "normal\nname");
    }

    #[tokio::test]
    async fn nested_workspace_deletion_binary_and_literal_pathspecs() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir_all(root.join("nested")).unwrap();
        command(&root, &["init"]);
        std::fs::write(root.join("nested/gone"), "gone\n").unwrap();
        std::fs::write(root.join("nested/binary"), [0, 1]).unwrap();
        std::fs::write(root.join("nested/[literal]"), "before\n").unwrap();
        command(&root, &["add", "nested"]);
        command(&root, &["commit", "-m", "fixture"]);
        std::fs::remove_file(root.join("nested/gone")).unwrap();
        std::fs::write(root.join("nested/binary"), [0, 2]).unwrap();
        std::fs::write(root.join("nested/[literal]"), "after\n").unwrap();
        let store = HubStore::open(temp.path().join("private/hub.db")).unwrap();
        session(&store, "nested", &root.join("nested"));
        let status = workspace_git_status(&store, "nested").await.unwrap();
        assert_eq!(status.files.len(), 3);
        assert!(status.files.iter().any(|file| file.path == "gone" && file.worktree == "D"));
        assert!(workspace_git_diff(&store, "nested", "binary", GitDiffArea::Unstaged).await.unwrap().text.contains("Binary files"));
        let diff = workspace_git_diff(&store, "nested", "[literal]", GitDiffArea::Unstaged).await.unwrap();
        assert!(diff.text.contains("+after"));
        assert!(!diff.text.contains("Binary files"));
    }
}
