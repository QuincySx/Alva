# Tool Quality Enhancement Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 3 P0 issues (shell output, .gitignore, truncation) and 5 P1 improvements (edit diff, batch edit, grep context, grep glob path, list_files cap) to bring tools to coding-agent quality.

**Architecture:** Create a shared `truncate` module used by all tools. Replace `walk_dir` with `walk_dir_filtered` that respects `.gitignore`. Fix shell to return actual output. Enhance edit/grep with missing features.

**Tech Stack:** Rust, `ignore` crate (for .gitignore-aware walking), `glob` crate, `regex` crate

---

## File Structure

| File | Responsibility |
|------|---------------|
| Create: `src/truncate.rs` | Shared truncation: `truncate_head()`, `truncate_tail()`, constants |
| Modify: `src/local_fs.rs` | Replace `walk_dir` with `.gitignore`-aware version using `ignore` crate |
| Modify: `src/execute_shell.rs` | Return actual output (tail-truncated) to model |
| Modify: `src/grep_search.rs` | Context lines, literal mode, full-path glob, output truncation |
| Modify: `src/find_files.rs` | Use new walk_dir, raise result cap to 1000 |
| Modify: `src/list_files.rs` | Add result cap (500), apply truncation |
| Modify: `src/file_edit.rs` | Diff output, batch edits |
| Modify: `src/read_file.rs` | Use shared truncate module (DRY) |
| Modify: `Cargo.toml` | Add `ignore` dependency |

All files are in `crates/alva-agent-tools/`.

---

### Task 1: Shared Truncation Module

**Files:**
- Create: `crates/alva-agent-tools/src/truncate.rs`
- Modify: `crates/alva-agent-tools/src/lib.rs`
- Modify: `crates/alva-agent-tools/src/read_file.rs` (use shared module)

- [ ] **Step 1: Create `truncate.rs`**

```rust
// crates/alva-agent-tools/src/truncate.rs
//! Shared output truncation for all tools.
//!
//! Two strategies:
//! - `truncate_head`: keep the beginning (for read, grep, find, ls)
//! - `truncate_tail`: keep the end (for bash — errors are usually at the bottom)

/// Default maximum lines in tool output.
pub const MAX_LINES: usize = 2000;
/// Default maximum bytes in tool output.
pub const MAX_BYTES: usize = 50 * 1024; // 50KB
/// Maximum characters per line in grep output.
pub const MAX_LINE_LENGTH: usize = 500;

pub struct TruncateResult {
    pub text: String,
    pub total_lines: usize,
    pub shown_lines: usize,
    pub truncated: bool,
}

/// Keep the beginning of the text. Used by read, grep, find, ls.
pub fn truncate_head(text: &str, max_lines: usize, max_bytes: usize) -> TruncateResult {
    let lines: Vec<&str> = text.lines().collect();
    let total_lines = lines.len();

    if total_lines == 0 {
        return TruncateResult {
            text: String::new(),
            total_lines: 0,
            shown_lines: 0,
            truncated: false,
        };
    }

    let effective_max = max_lines.min(total_lines);
    let mut byte_count = 0;
    let mut end = effective_max;

    for (i, line) in lines.iter().enumerate().take(effective_max) {
        byte_count += line.len() + 1;
        if byte_count > max_bytes {
            end = i + 1;
            break;
        }
    }

    let shown = &lines[..end];
    let shown_lines = shown.len();
    let truncated = end < total_lines;

    TruncateResult {
        text: shown.join("\n"),
        total_lines,
        shown_lines,
        truncated,
    }
}

/// Keep the end of the text. Used by bash (errors are usually at the bottom).
pub fn truncate_tail(text: &str, max_lines: usize, max_bytes: usize) -> TruncateResult {
    let lines: Vec<&str> = text.lines().collect();
    let total_lines = lines.len();

    if total_lines == 0 {
        return TruncateResult {
            text: String::new(),
            total_lines: 0,
            shown_lines: 0,
            truncated: false,
        };
    }

    let effective_max = max_lines.min(total_lines);
    let start_from = total_lines.saturating_sub(effective_max);
    let mut byte_count = 0;
    let mut start = start_from;

    // Walk backwards from the end to find byte limit
    for i in (start_from..total_lines).rev() {
        byte_count += lines[i].len() + 1;
        if byte_count > max_bytes {
            start = i + 1;
            break;
        }
    }

    let shown = &lines[start..];
    let shown_lines = shown.len();
    let truncated = start > 0;

    TruncateResult {
        text: shown.join("\n"),
        total_lines,
        shown_lines,
        truncated,
    }
}

/// Truncate a single line to max length, appending "... [truncated]" if cut.
pub fn truncate_line(line: &str, max_len: usize) -> String {
    if line.len() <= max_len {
        line.to_string()
    } else {
        format!("{}... [truncated]", &line[..max_len])
    }
}
```

- [ ] **Step 2: Add `pub mod truncate;` to `lib.rs`**

In `crates/alva-agent-tools/src/lib.rs`, add after the existing module declarations:
```rust
pub mod truncate;
```

- [ ] **Step 3: Add tests for truncate**

Add to the bottom of `truncate.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_head_no_truncation() {
        let text = "line1\nline2\nline3";
        let r = truncate_head(text, 100, 100_000);
        assert!(!r.truncated);
        assert_eq!(r.shown_lines, 3);
        assert_eq!(r.total_lines, 3);
    }

    #[test]
    fn truncate_head_by_lines() {
        let text = (0..100).map(|i| format!("line{}", i)).collect::<Vec<_>>().join("\n");
        let r = truncate_head(&text, 10, 100_000);
        assert!(r.truncated);
        assert_eq!(r.shown_lines, 10);
        assert_eq!(r.total_lines, 100);
    }

    #[test]
    fn truncate_head_by_bytes() {
        let text = (0..100).map(|_| "x".repeat(100)).collect::<Vec<_>>().join("\n");
        let r = truncate_head(&text, 2000, 500);
        assert!(r.truncated);
        assert!(r.shown_lines < 100);
    }

    #[test]
    fn truncate_tail_keeps_end() {
        let text = (0..100).map(|i| format!("line{}", i)).collect::<Vec<_>>().join("\n");
        let r = truncate_tail(&text, 10, 100_000);
        assert!(r.truncated);
        assert_eq!(r.shown_lines, 10);
        assert!(r.text.contains("line99"));
        assert!(!r.text.contains("line0"));
    }

    #[test]
    fn truncate_line_short() {
        assert_eq!(truncate_line("short", 100), "short");
    }

    #[test]
    fn truncate_line_long() {
        let long = "x".repeat(600);
        let result = truncate_line(&long, 500);
        assert!(result.ends_with("... [truncated]"));
        assert!(result.len() < 600);
    }

    #[test]
    fn empty_input() {
        let r = truncate_head("", 100, 100_000);
        assert!(!r.truncated);
        assert_eq!(r.shown_lines, 0);
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p alva-agent-tools truncate`
Expected: PASS

- [ ] **Step 5: Refactor `read_file.rs` to use shared truncate**

Replace the inline truncation logic in `read_file.rs` with calls to `truncate_head()`. Remove the per-file `MAX_LINES` and `MAX_BYTES` constants (use `truncate::MAX_LINES` and `truncate::MAX_BYTES` instead).

- [ ] **Step 6: Run tests and commit**

Run: `cargo test -p alva-agent-tools`
Expected: PASS

```bash
git add crates/alva-agent-tools/
git commit -m "feat: shared truncation module (truncate_head, truncate_tail)

All tools need output truncation to prevent context overflow.
DRY: single module with two strategies — keep-beginning for
read/grep/find/ls, keep-end for bash (errors at the bottom)."
```

---

### Task 2: .gitignore-Aware walk_dir

**Files:**
- Modify: `crates/alva-agent-tools/Cargo.toml` (add `ignore` dep)
- Modify: `crates/alva-agent-tools/src/local_fs.rs` (new `walk_dir_filtered`)

- [ ] **Step 1: Add `ignore` dependency**

In `crates/alva-agent-tools/Cargo.toml`, under `[dependencies]`:
```toml
ignore = "0.4"
```

- [ ] **Step 2: Add `walk_dir_filtered` to `local_fs.rs`**

Add a new function alongside the existing `walk_dir`:

```rust
/// Recursively list all file paths under a directory, respecting .gitignore rules.
///
/// Uses the `ignore` crate which handles:
/// - .gitignore at all directory levels
/// - .git/info/exclude
/// - Global gitignore ($HOME/.config/git/ignore)
/// - Hidden file exclusion (when `include_hidden` is false)
///
/// Falls back to the ToolFs-based `walk_dir` when the `ignore` crate
/// cannot be used (e.g., remote/sandbox ToolFs).
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
        builder.max_depth(Some(depth + 1)); // ignore crate counts root as depth 0
    }

    let mut results = Vec::new();
    for entry in builder.build() {
        let entry = entry.map_err(|e| AgentError::ToolError {
            tool_name: "walk_dir_filtered".into(),
            message: format!("walk error: {}", e),
        })?;

        // Skip directories — only collect files
        if entry.file_type().map_or(true, |ft| ft.is_dir()) {
            continue;
        }

        if let Some(path) = entry.path().to_str() {
            results.push(path.to_string());
        }
    }

    Ok(results)
}
```

- [ ] **Step 3: Test walk_dir_filtered**

Add test to `local_fs.rs`:
```rust
#[tokio::test]
async fn test_walk_dir_filtered_respects_gitignore() {
    let dir = TempDir::new().expect("create tempdir");
    let root = dir.path();

    // Create files
    std::fs::write(root.join("keep.rs"), "fn main() {}").unwrap();
    std::fs::create_dir_all(root.join("target/debug")).unwrap();
    std::fs::write(root.join("target/debug/binary"), "bin").unwrap();
    std::fs::create_dir_all(root.join("node_modules/foo")).unwrap();
    std::fs::write(root.join("node_modules/foo/index.js"), "js").unwrap();

    // Create .gitignore
    std::fs::write(root.join(".gitignore"), "target/\nnode_modules/\n").unwrap();

    // Init git repo (ignore crate needs .git to activate gitignore)
    std::fs::create_dir(root.join(".git")).unwrap();

    let results = walk_dir_filtered(root.to_str().unwrap(), None, false).unwrap();
    let names: Vec<&str> = results.iter().map(|s| s.as_str()).collect();

    assert!(names.iter().any(|n| n.ends_with("keep.rs")), "should include keep.rs");
    assert!(!names.iter().any(|n| n.contains("target")), "should exclude target/");
    assert!(!names.iter().any(|n| n.contains("node_modules")), "should exclude node_modules/");
}
```

- [ ] **Step 4: Run tests and commit**

Run: `cargo test -p alva-agent-tools walk_dir_filtered`
Expected: PASS

```bash
git add crates/alva-agent-tools/
git commit -m "feat: .gitignore-aware walk_dir_filtered using ignore crate

grep_search and find_files were including node_modules/, target/,
build/ etc in search results. The ignore crate respects .gitignore
at all directory levels, .git/info/exclude, and global gitignore."
```

---

### Task 3: Fix execute_shell Output

**Files:**
- Modify: `crates/alva-agent-tools/src/execute_shell.rs`

- [ ] **Step 1: Update execute_shell to return actual output**

Replace the Ok(result) branch in `execute`:

```rust
Ok(result) => {
    // Report progress events (for real-time UI streaming)
    for line in result.stdout.lines() {
        ctx.report_progress(ProgressEvent::StdoutLine {
            line: line.to_string(),
        });
    }
    for line in result.stderr.lines() {
        ctx.report_progress(ProgressEvent::StderrLine {
            line: line.to_string(),
        });
    }

    // Combine stdout + stderr for the model
    // Use tail truncation — errors are usually at the end
    let mut combined = String::new();
    if !result.stdout.is_empty() {
        combined.push_str(&result.stdout);
    }
    if !result.stderr.is_empty() {
        if !combined.is_empty() {
            combined.push_str("\n--- stderr ---\n");
        }
        combined.push_str(&result.stderr);
    }

    let tr = crate::truncate::truncate_tail(
        &combined,
        crate::truncate::MAX_LINES,
        crate::truncate::MAX_BYTES,
    );

    let mut content = tr.text;
    if tr.truncated {
        content = format!(
            "[Output truncated: showing last {} of {} lines]\n{}",
            tr.shown_lines, tr.total_lines, content
        );
    }
    content.push_str(&format!("\n\nexit_code: {}", result.exit_code));

    Ok(ToolOutput {
        content: vec![ToolContent::text(content)],
        is_error: result.exit_code != 0,
        details: Some(json!({
            "stdout": result.stdout,
            "stderr": result.stderr,
            "exit_code": result.exit_code,
            "truncated": tr.truncated,
        })),
    })
}
```

- [ ] **Step 2: Run workspace check and commit**

Run: `cargo check -p alva-agent-tools`
Expected: PASS

```bash
git add crates/alva-agent-tools/src/execute_shell.rs
git commit -m "fix: execute_shell returns actual output to LLM, not just summary

Previously returned 'exit_code: 0, 15 stdout lines, 3 stderr lines' —
the LLM couldn't see what the command actually output. Now returns the
actual text with tail truncation (2000 lines / 50KB), keeping the end
where errors typically appear."
```

---

### Task 4: grep_search Improvements

**Files:**
- Modify: `crates/alva-agent-tools/src/grep_search.rs`

- [ ] **Step 1: Update grep_search with context, literal, full-path glob, and truncation**

Replace the full `grep_search.rs` with an enhanced version:

Key changes:
1. Add `context: Option<usize>` parameter (lines before/after match)
2. Add `literal: Option<bool>` parameter (treat pattern as literal string, not regex)
3. Fix glob matching to use full relative path (not just filename)
4. Add output truncation via `truncate_head`
5. Add per-line truncation via `truncate_line`
6. Use `walk_dir_filtered` instead of `walk_dir` for .gitignore support

The new parameter schema adds:
```json
"context": {
    "type": "integer",
    "description": "Number of context lines before and after each match, default 0"
},
"literal": {
    "type": "boolean",
    "description": "Treat pattern as literal string instead of regex, default false"
}
```

The new Input struct:
```rust
#[derive(Debug, Deserialize)]
struct Input {
    pattern: String,
    path: Option<String>,
    file_pattern: Option<String>,
    case_insensitive: Option<bool>,
    literal: Option<bool>,
    context: Option<usize>,
    max_results: Option<usize>,
}
```

Literal mode: wrap pattern in `regex::escape()` before compiling.

Context lines: when a match is found at line N, include lines `N-context..N+context` in output, formatted as:
```
path/to/file-10- context line before
path/to/file:11: matching line
path/to/file-12- context line after
```

Glob filter: change from matching `file_name` only to matching `rel_path`:
```rust
if let Some(ref glob) = glob_pattern {
    if !glob.matches(rel_path) {
        continue;
    }
}
```

Apply `truncate_head` to the final output text.
Apply `truncate_line` to each match line.

- [ ] **Step 2: Run tests and commit**

Run: `cargo check -p alva-agent-tools`
Expected: PASS

```bash
git add crates/alva-agent-tools/src/grep_search.rs
git commit -m "feat: grep_search gains context lines, literal mode, full-path glob, truncation

- context: N shows N lines before/after each match
- literal: true treats pattern as literal (no regex escaping needed)
- glob now matches full relative path, not just filename
- output truncated to 2000 lines / 50KB via shared truncate module
- per-line truncation at 500 chars prevents single long lines blowing context
- uses walk_dir_filtered for .gitignore support"
```

---

### Task 5: find_files .gitignore + Cap

**Files:**
- Modify: `crates/alva-agent-tools/src/find_files.rs`

- [ ] **Step 1: Update find_files**

Key changes:
1. Use `walk_dir_filtered` instead of `walk_dir` (for .gitignore support)
2. Raise `MAX_RESULTS` from 200 to 1000
3. Apply `truncate_head` to output

Replace the `walk_dir` call with:
```rust
let all_paths = crate::local_fs::walk_dir_filtered(
    search_root_str,
    None,
    false,
)?;
```

Note: `walk_dir_filtered` is synchronous (the `ignore` crate is sync), so it doesn't need `.await`.

Update `MAX_RESULTS` to 1000.

- [ ] **Step 2: Run tests and commit**

```bash
git add crates/alva-agent-tools/src/find_files.rs
git commit -m "feat: find_files respects .gitignore, raises result cap to 1000

Uses walk_dir_filtered (ignore crate) to exclude gitignored paths.
Raised default cap from 200 to 1000 to match Pi's default."
```

---

### Task 6: list_files Result Cap

**Files:**
- Modify: `crates/alva-agent-tools/src/list_files.rs`

- [ ] **Step 1: Add result cap**

Add a constant:
```rust
const MAX_ENTRIES: usize = 500;
```

After collecting and sorting entries, apply truncation:
```rust
entries.sort();
let total = entries.len();
let truncated = total > MAX_ENTRIES;
if truncated {
    entries.truncate(MAX_ENTRIES);
}

let mut content = entries.join("\n");
if truncated {
    content.push_str(&format!(
        "\n\n[Showing {} of {} entries. Use a more specific path to see more.]",
        MAX_ENTRIES, total
    ));
}

Ok(ToolOutput {
    content: vec![ToolContent::text(content)],
    is_error: false,
    details: Some(json!({
        "total_entries": total,
        "shown": entries.len().min(MAX_ENTRIES),
        "truncated": truncated,
    })),
})
```

- [ ] **Step 2: Run tests and commit**

```bash
git add crates/alva-agent-tools/src/list_files.rs
git commit -m "feat: list_files caps output at 500 entries

Recursive listing on large projects was unbounded, risking
context overflow. Capped at 500 with continuation hint."
```

---

### Task 7: file_edit Diff Output

**Files:**
- Modify: `crates/alva-agent-tools/src/file_edit.rs`

- [ ] **Step 1: Add diff generation to file_edit**

After the successful edit, generate a simple diff showing context:

```rust
// Find the line number where the change starts
let line_num = content[..content.find(&params.old_str).unwrap()]
    .lines()
    .count() + 1;

// Generate a simple before/after diff
let old_lines: Vec<&str> = params.old_str.lines().collect();
let new_lines: Vec<&str> = params.new_str.lines().collect();

let mut diff = format!("--- {}\n+++ {}\n@@ -{},{} +{},{} @@\n",
    params.path, params.path,
    line_num, old_lines.len(),
    line_num, new_lines.len(),
);
for line in &old_lines {
    diff.push_str(&format!("-{}\n", line));
}
for line in &new_lines {
    diff.push_str(&format!("+{}\n", line));
}

Ok(ToolOutput {
    content: vec![ToolContent::text(format!(
        "File edited: {} (line {})\n\n{}",
        params.path, line_num, diff
    ))],
    is_error: false,
    details: Some(json!({
        "path": params.path,
        "first_changed_line": line_num,
        "old_lines": old_lines.len(),
        "new_lines": new_lines.len(),
    })),
})
```

- [ ] **Step 2: Run tests and commit**

```bash
git add crates/alva-agent-tools/src/file_edit.rs
git commit -m "feat: file_edit returns diff output with line numbers

Previously returned only 'File edited: path'. Now returns a
unified diff showing removed/added lines and the first changed
line number, so the LLM can verify the edit was applied correctly."
```

---

### Task 8: file_edit Batch Edits

**Files:**
- Modify: `crates/alva-agent-tools/src/file_edit.rs`

- [ ] **Step 1: Add batch edit support**

Update the Input struct to accept either single edit or batch:

```rust
#[derive(Debug, Deserialize)]
struct SingleEdit {
    old_str: String,
    new_str: String,
}

#[derive(Debug, Deserialize)]
struct Input {
    path: String,
    // Single edit (backward compatible)
    #[serde(default)]
    old_str: Option<String>,
    #[serde(default)]
    new_str: Option<String>,
    // Batch edit
    #[serde(default)]
    edits: Option<Vec<SingleEdit>>,
}
```

Update the parameter schema to document both modes. Update execute to:
1. If `edits` is present, use it
2. Else if `old_str`/`new_str` present, wrap into `edits = vec![{old_str, new_str}]`
3. Else error

For batch edits: validate ALL old_str are unique and non-overlapping in the original file BEFORE applying any. Apply all edits to the original content. Generate a combined diff.

- [ ] **Step 2: Run tests and commit**

```bash
git add crates/alva-agent-tools/src/file_edit.rs
git commit -m "feat: file_edit supports batch edits via edits[] array

Single old_str/new_str still works (backward compatible).
New edits[] parameter allows multiple replacements in one call.
All edits validated against the original content before any are applied."
```

---

### Task 9: Final Verification

- [ ] **Step 1: Full build**

Run: `cargo build --workspace`
Expected: PASS

- [ ] **Step 2: Full tests**

Run: `cargo test --workspace`
Expected: PASS

- [ ] **Step 3: Commit any cleanup**

```bash
git add -A && git commit -m "chore: final cleanup after tool quality enhancements"
```
