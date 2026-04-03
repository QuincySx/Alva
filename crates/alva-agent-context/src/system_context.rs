// INPUT:  std::path::Path, std::collections::HashMap, tokio::process::Command, tokio::fs
// OUTPUT: get_system_context(), get_user_context(), get_git_status(), detect_main_branch(), load_context_files()
// POS:    Context providers matching Claude Code's context.ts — collects git status, environment info,
//         user context files (CLAUDE.md, AGENTS.md), and current date for system prompt injection.
//! System and user context providers.
//!
//! Gathers workspace-level context (git status, environment) and user-level
//! context (CLAUDE.md, AGENTS.md, current date) for injection into the system prompt.
//!
//! Results should be cached for the duration of a conversation — these are
//! collected once at session start and refreshed on explicit request.

use std::collections::HashMap;
use std::path::Path;

/// Collect system-level context for the given workspace.
///
/// Currently gathers:
/// - Git repository status (branch, modified files, recent commits)
///
/// Results are returned as a key-value map suitable for template rendering
/// or direct injection into the system prompt.
pub async fn get_system_context(workspace: &Path) -> HashMap<String, String> {
    let mut context = HashMap::new();

    if let Some(git_status) = get_git_status(workspace).await {
        context.insert("gitStatus".to_string(), git_status);
    }

    context
}

/// Collect user-level context for the given workspace.
///
/// Currently gathers:
/// - Current date string
/// - Contents of CLAUDE.md / AGENTS.md / .alva/context.md files
pub async fn get_user_context(workspace: &Path) -> HashMap<String, String> {
    let mut context = HashMap::new();

    // Current date
    context.insert(
        "currentDate".to_string(),
        format!(
            "Today's date is {}.",
            chrono::Local::now().format("%Y-%m-%d")
        ),
    );

    // Load context files (CLAUDE.md, AGENTS.md, etc.)
    if let Some(claude_md) = load_context_files(workspace).await {
        context.insert("claudeMd".to_string(), claude_md);
    }

    context
}

/// Get formatted git status for the workspace.
///
/// Returns `None` if the workspace is not a git repository.
/// Truncates status output at 2000 characters to keep context size bounded.
async fn get_git_status(workspace: &Path) -> Option<String> {
    // 1. Current branch
    let branch = tokio::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(workspace)
        .output()
        .await
        .ok()?;

    let branch = String::from_utf8_lossy(&branch.stdout).trim().to_string();
    if branch.is_empty() {
        return None;
    }

    // 2. Working tree status (short format)
    let status = tokio::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(workspace)
        .output()
        .await
        .ok()?;
    let status_text = String::from_utf8_lossy(&status.stdout);
    let status_text = if status_text.len() > 2000 {
        format!("{}...(truncated)", &status_text[..2000])
    } else {
        status_text.to_string()
    };

    // 3. Recent commits (last 5)
    let log = tokio::process::Command::new("git")
        .args(["log", "--oneline", "-5"])
        .current_dir(workspace)
        .output()
        .await
        .ok()?;
    let log_text = String::from_utf8_lossy(&log.stdout);

    // 4. Detect main branch
    let main_branch = detect_main_branch(workspace).await;

    Some(format!(
        "Current branch: {}\n\nMain branch (you will usually use this for PRs): {}\n\nStatus:\n{}\n\nRecent commits:\n{}",
        branch,
        main_branch,
        status_text.trim(),
        log_text.trim()
    ))
}

/// Detect the main branch name ("main" or "master").
///
/// Checks "main" first, then falls back to "master". If neither exists,
/// defaults to "main".
async fn detect_main_branch(workspace: &Path) -> String {
    for branch in &["main", "master"] {
        let result = tokio::process::Command::new("git")
            .args(["rev-parse", "--verify", branch])
            .current_dir(workspace)
            .output()
            .await;
        if let Ok(output) = result {
            if output.status.success() {
                return branch.to_string();
            }
        }
    }
    "main".to_string()
}

/// Load context files from the workspace directory.
///
/// Searches for files in priority order:
/// 1. `CLAUDE.md`
/// 2. `AGENTS.md`
/// 3. `.claude/settings.json`
/// 4. `.alva/context.md`
///
/// Returns `None` if no context files are found.
async fn load_context_files(workspace: &Path) -> Option<String> {
    let mut contents = Vec::new();

    let context_files = [
        "CLAUDE.md",
        "AGENTS.md",
        ".claude/settings.json",
        ".alva/context.md",
    ];

    for file in &context_files {
        let path = workspace.join(file);
        if path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                contents.push(format!(
                    "Contents of {}:\n\n{}",
                    file, content
                ));
            }
        }
    }

    if contents.is_empty() {
        None
    } else {
        Some(contents.join("\n\n---\n\n"))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn get_user_context_includes_date() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = get_user_context(tmp.path()).await;

        assert!(ctx.contains_key("currentDate"));
        let date = &ctx["currentDate"];
        assert!(date.starts_with("Today's date is "));
        assert!(date.ends_with('.'));
    }

    #[tokio::test]
    async fn get_user_context_loads_claude_md() {
        let tmp = tempfile::TempDir::new().unwrap();
        tokio::fs::write(tmp.path().join("CLAUDE.md"), "# My Project\nSome rules here.")
            .await
            .unwrap();

        let ctx = get_user_context(tmp.path()).await;

        assert!(ctx.contains_key("claudeMd"));
        let md = &ctx["claudeMd"];
        assert!(md.contains("CLAUDE.md"));
        assert!(md.contains("Some rules here."));
    }

    #[tokio::test]
    async fn get_user_context_no_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = get_user_context(tmp.path()).await;

        // Should have currentDate but no claudeMd
        assert!(ctx.contains_key("currentDate"));
        assert!(!ctx.contains_key("claudeMd"));
    }

    #[tokio::test]
    async fn get_system_context_non_git_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        let ctx = get_system_context(tmp.path()).await;

        // Non-git directory should not have gitStatus
        assert!(!ctx.contains_key("gitStatus"));
    }

    #[tokio::test]
    async fn detect_main_branch_fallback() {
        let tmp = tempfile::TempDir::new().unwrap();
        // In a non-git directory, should default to "main"
        let branch = detect_main_branch(tmp.path()).await;
        assert_eq!(branch, "main");
    }
}
