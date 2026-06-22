//! Terminal output formatting for alva-cli.

use crossterm::style::Stylize;
use std::io::{self, Write};
use std::path::Path;

pub fn print_banner(model: &str, workspace: &str) {
    eprintln!(
        "{} {} {}",
        "╭".dark_grey(),
        "Alva Agent".bold().cyan(),
        format!("v{}", env!("CARGO_PKG_VERSION")).dark_grey(),
    );
    eprintln!("{}  Model: {}", "│".dark_grey(), model.yellow());
    eprintln!("{}  Workspace: {}", "│".dark_grey(), workspace.white());
}

pub fn print_banner_end() {
    eprintln!("{}", "╰───────────────────────────────────────".dark_grey());
}

pub fn print_git_status(workspace: &Path) {
    let branch = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(workspace)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    if let Some(branch) = branch {
        eprintln!("{}  Branch: {}", "│".dark_grey(), branch.magenta());

        let status = std::process::Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(workspace)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

        if let Some(status) = status {
            if status.is_empty() {
                eprintln!("{}  Status: {}", "│".dark_grey(), "clean".green());
            } else {
                let count = status.lines().count();
                eprintln!(
                    "{}  Status: {}",
                    "│".dark_grey(),
                    format!("{} changed files", count).yellow()
                );
            }
        }
    }
}

pub fn print_tool_start(name: &str) {
    eprintln!("  {} {} ...", "●".dark_yellow(), name.dark_yellow());
}

pub fn print_tool_end(name: &str, is_error: bool, preview: &str) {
    let preview_clean = preview.replace('\n', " ");
    // Use safe_preview to back off to the previous UTF-8 char boundary —
    // tool output containing emoji / CJK at the truncation point would
    // otherwise panic on `&preview_clean[..100]`.
    let preview_short = safe_preview(&preview_clean, 100);
    if is_error {
        eprintln!(
            "  {} {} {}",
            "✗".red(),
            name.red(),
            preview_short.dark_grey()
        );
    } else {
        eprintln!(
            "  {} {} {}",
            "✓".green(),
            name.green(),
            preview_short.dark_grey()
        );
    }
}

pub fn print_error(msg: &str) {
    eprintln!("{} {}", "Error:".red().bold(), msg);
}

pub fn print_assistant_text(text: &str) {
    print!("{}", text);
    io::stdout().flush().ok();
}

pub fn print_divider() {
    eprintln!("{}", "───────────────────────────────────────".dark_grey());
}

pub fn print_session_resumed(id: &str, count: usize, summary: &str) {
    eprintln!(
        "{}",
        format!(
            "Resuming session {} ({} messages) — \"{}\"",
            id, count, summary
        )
        .dark_grey()
    );
    eprintln!(
        "{}",
        "Type /new for fresh session, /help for commands.".dark_grey()
    );
}

pub fn print_session_new(id: &str) {
    eprintln!("{}", format!("New session: {}", id).dark_grey());
}

pub fn print_usage(input_tokens: u64, output_tokens: u64) {
    let total = input_tokens + output_tokens;
    eprintln!(
        "\x1b[90m  tokens: {} in / {} out / {} total\x1b[0m",
        input_tokens, output_tokens, total
    );
}

pub fn print_approval_prompt(tool_name: &str, args: &serde_json::Value) {
    use crate::ui::permission_dialog::is_dangerous_command;

    eprintln!();

    // Detect dangerous commands
    let is_dangerous = if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
        is_dangerous_command(cmd)
    } else {
        false
    };

    if is_dangerous {
        eprintln!(
            "  {}",
            "╭─ ⚠ DANGEROUS — Permission Required ──────────"
                .red()
                .bold()
        );
    } else {
        eprintln!(
            "  {}",
            "╭─ Permission Required ──────────────────────────".dark_yellow()
        );
    }

    eprintln!(
        "  {}  Tool: {}",
        "│".dark_yellow(),
        tool_name.yellow().bold()
    );

    // Show relevant arguments
    if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
        if is_dangerous {
            eprintln!("  {}  Command: {}", "│".dark_yellow(), cmd.red().bold());
        } else {
            eprintln!("  {}  Command: {}", "│".dark_yellow(), cmd.white());
        }
    }
    if let Some(path) = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .and_then(|v| v.as_str())
    {
        eprintln!("  {}  Path: {}", "│".dark_yellow(), path.white());
    }

    // Show old_str/new_str diff for file_edit
    if let Some(old_str) = args
        .get("old_string")
        .or_else(|| args.get("old_str"))
        .and_then(|v| v.as_str())
    {
        let preview = safe_preview(old_str, 120);
        eprintln!(
            "  {}  {}: {}",
            "│".dark_yellow(),
            "Old".red(),
            preview.red()
        );
    }
    if let Some(new_str) = args
        .get("new_string")
        .or_else(|| args.get("new_str"))
        .and_then(|v| v.as_str())
    {
        let preview = safe_preview(new_str, 120);
        eprintln!(
            "  {}  {}: {}",
            "│".dark_yellow(),
            "New".green(),
            preview.green()
        );
    }

    // Show URL for web fetch
    if let Some(url) = args.get("url").and_then(|v| v.as_str()) {
        eprintln!("  {}  URL: {}", "│".dark_yellow(), url.white());
    }

    if let Some(content) = args.get("content").and_then(|v| v.as_str()) {
        let preview = safe_preview(content, 80);
        eprintln!("  {}  Content: {}", "│".dark_yellow(), preview.dark_grey());
    }

    eprintln!(
        "  {}",
        "╰────────────────────────────────────────────────".dark_yellow()
    );
    eprint!(
        "  Allow? [{}]es / [{}]o / [{}]lways / [{}]eny always: ",
        "y".green().bold(),
        "n".red().bold(),
        "a".cyan().bold(),
        "d".red().bold(),
    );
    io::stderr().flush().ok();
}

/// Truncate a string at a UTF-8 safe boundary for display previews.
fn safe_preview(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

#[cfg(test)]
mod tests {
    //! Tests for `safe_preview`, the UTF-8-aware truncation utility.
    //!
    //! These pin down the boundary-safety behavior — if a future
    //! refactor replaces `safe_preview` with naive `&s[..n]` slicing
    //! (the bug that was just fixed in `print_tool_end`), these
    //! tests panic loudly with byte-index errors instead of silently
    //! shipping a process-crashing regression.
    use super::*;

    #[test]
    fn safe_preview_short_string_returned_unchanged() {
        let s = "hello";
        assert_eq!(safe_preview(s, 100), "hello");
    }

    #[test]
    fn safe_preview_exact_byte_length_returned_unchanged() {
        // s.len() == max_bytes → no truncation (`<= max_bytes` branch).
        let s = "abcdef"; // 6 bytes
        assert_eq!(safe_preview(s, 6), "abcdef");
    }

    #[test]
    fn safe_preview_ascii_truncated_at_exact_byte() {
        // ASCII so every byte is also a char boundary.
        let s = "abcdefghij"; // 10 bytes
        assert_eq!(safe_preview(s, 4), "abcd...");
    }

    #[test]
    fn safe_preview_backs_off_from_mid_emoji_to_char_boundary() {
        // "🦀" is 4 bytes. "a🦀b" = 1 + 4 + 1 = 6 bytes.
        // max_bytes=3 → byte 3 is INSIDE 🦀 (bytes 1..5) → back off
        // to byte 1 (the char boundary before 🦀).
        let s = "a🦀b";
        assert_eq!(s.len(), 6);
        assert!(
            !s.is_char_boundary(3),
            "test premise: byte 3 is not a boundary"
        );
        assert_eq!(safe_preview(s, 3), "a...");
    }

    #[test]
    fn safe_preview_backs_off_from_mid_cjk_to_char_boundary() {
        // CJK char "中" is 3 bytes. "a中b" = 1 + 3 + 1 = 5 bytes.
        // max_bytes=2 → byte 2 lands inside 中 (bytes 1..4) → back off
        // to byte 1.
        let s = "a中b";
        assert_eq!(s.len(), 5);
        assert!(!s.is_char_boundary(2));
        assert_eq!(safe_preview(s, 2), "a...");
    }

    #[test]
    fn safe_preview_handles_max_bytes_zero() {
        // max_bytes=0: long string truncates to "..." (back-off loop
        // exits immediately on `end > 0`); short string (len <= 0 is
        // only the empty string) returns unchanged.
        assert_eq!(safe_preview("anything", 0), "...");
        assert_eq!(safe_preview("", 0), "");
    }

    #[test]
    fn safe_preview_realistic_repl_panic_scenario_no_longer_crashes() {
        // The bug print_tool_end used to have: 30 ASCII chars + 18×
        // 4-byte emoji = 30 + 72 = 102 bytes; max_bytes=100 lands in
        // the middle of the 18th emoji. Naive `&s[..100]` would panic.
        // safe_preview must back off to the boundary BEFORE byte 100.
        let s = format!("{}{}", "x".repeat(30), "🦀".repeat(18));
        assert_eq!(s.len(), 30 + 18 * 4, "test premise: 102 bytes");
        // Must not panic; output ends with "..." after safe truncation.
        let out = safe_preview(&s, 100);
        assert!(out.ends_with("..."));
        // The truncated content (before "...") must be valid UTF-8 — it
        // already is by construction, but assert it's at most 100 bytes
        // and contains only complete chars.
        let kept = out.strip_suffix("...").unwrap();
        assert!(kept.len() <= 100);
        // Verify no half-character: take the chars iterator round-trip.
        assert_eq!(kept.chars().count() > 0, true);
    }

    #[test]
    fn safe_preview_realistic_approval_prompt_content_arg_no_crash() {
        // Regression for L62: `print_approval_prompt` used to do
        // `&content[..80]` for the "content" arg preview. Realistic
        // case where a Write/Edit tool's content arg starts with
        // ~70 ASCII chars + an emoji or CJK char that lands across
        // byte 80 — the old code would panic before the user could
        // even see the approval prompt, taking down the REPL.
        //
        // Construct: 78 ASCII + 1 4-byte emoji + 50 ASCII = 132 bytes.
        // Byte 80 falls inside the emoji (bytes 78..82).
        let s = format!("{}{}{}", "a".repeat(78), "🦀", "b".repeat(50));
        assert_eq!(s.len(), 132);
        assert!(!s.is_char_boundary(80), "test premise: byte 80 mid-emoji");
        // Must not panic.
        let out = safe_preview(&s, 80);
        assert!(out.ends_with("..."));
        let kept = out.strip_suffix("...").unwrap();
        assert!(kept.len() <= 80);
        assert!(kept.is_char_boundary(kept.len()));
    }
}
