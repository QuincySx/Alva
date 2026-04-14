// INPUT:  alva_kernel_abi::{ToolFs, AgentError}, ignore::WalkBuilder
// OUTPUT: walk_dir (async, ToolFs-based), walk_dir_filtered (sync, gitignore-aware)
// POS:    Directory traversal helpers shared by find_files / grep_search / list_files.

use alva_kernel_abi::{AgentError, ToolFs};

// ---------------------------------------------------------------------------
// walk_dir — recursive directory traversal via ToolFs
// ---------------------------------------------------------------------------

/// Recursively list all file paths under a directory via [`ToolFs`].
///
/// Hidden entries (names starting with `.`) are excluded when
/// `include_hidden` is `false`. Depth is measured from the initial `root`;
/// passing `max_depth = Some(0)` returns files directly inside `root`.
pub async fn walk_dir(
    fs: &dyn ToolFs,
    root: &str,
    max_depth: Option<usize>,
    include_hidden: bool,
) -> Result<Vec<String>, AgentError> {
    let mut results = Vec::new();
    // Stack entries: (directory path, current depth)
    let mut stack: Vec<(String, usize)> = vec![(root.to_string(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if let Some(max) = max_depth {
            if depth > max {
                continue;
            }
        }
        let entries = fs.list_dir(&dir).await?;
        for entry in entries {
            if !include_hidden && entry.name.starts_with('.') {
                continue;
            }
            let full = format!("{}/{}", dir.trim_end_matches('/'), entry.name);
            if entry.is_dir {
                stack.push((full, depth + 1));
            } else {
                results.push(full);
            }
        }
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// walk_dir_filtered — .gitignore-aware directory traversal
// ---------------------------------------------------------------------------

/// Recursively list all file paths under a directory, respecting .gitignore rules.
///
/// Uses the `ignore` crate which handles:
/// - .gitignore at all directory levels
/// - .git/info/exclude
/// - Global gitignore
/// - Hidden file exclusion (when include_hidden is false)
///
/// This is synchronous because the ignore crate uses std::fs internally.
pub fn walk_dir_filtered(
    root: &str,
    max_depth: Option<usize>,
    include_hidden: bool,
) -> Result<Vec<String>, AgentError> {
    use ignore::WalkBuilder;

    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(!include_hidden)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true);

    if let Some(depth) = max_depth {
        builder.max_depth(Some(depth + 1));
    }

    let mut results = Vec::new();
    for entry in builder.build() {
        let entry = entry.map_err(|e| AgentError::ToolError {
            tool_name: "walk_dir_filtered".into(),
            message: format!("walk error: {}", e),
        })?;
        if entry.file_type().map_or(true, |ft| ft.is_dir()) {
            continue;
        }
        if let Some(path) = entry.path().to_str() {
            results.push(path.to_string());
        }
    }
    Ok(results)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_walk_dir_filtered_respects_gitignore() {
        let dir = TempDir::new().expect("tempdir");
        let root = dir.path();

        std::fs::write(root.join("keep.rs"), "fn main() {}").unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("target/debug/binary"), "bin").unwrap();
        std::fs::create_dir_all(root.join("node_modules/foo")).unwrap();
        std::fs::write(root.join("node_modules/foo/index.js"), "js").unwrap();
        std::fs::write(root.join(".gitignore"), "target/\nnode_modules/\n").unwrap();
        // ignore crate needs a .git dir to activate gitignore
        std::fs::create_dir(root.join(".git")).unwrap();

        let results = walk_dir_filtered(root.to_str().unwrap(), None, false).unwrap();
        assert!(results.iter().any(|p| p.ends_with("keep.rs")));
        assert!(!results.iter().any(|p| p.contains("target")));
        assert!(!results.iter().any(|p| p.contains("node_modules")));
    }
}
