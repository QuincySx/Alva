// INPUT:  (none — pure logic)
// OUTPUT: TruncateResult, truncate_head, truncate_tail, truncate_line, MAX_LINES, MAX_BYTES, MAX_LINE_LENGTH
// POS:    Shared output truncation to prevent context overflow across all tool implementations.
//! Shared truncation utilities for tool output.
//!
//! Two strategies:
//! - `truncate_head` — keep the beginning (for read, grep, find, ls)
//! - `truncate_tail` — keep the end (for bash — errors are usually at the bottom)
//!
//! Plus `truncate_line` for single-line length capping.

/// Maximum lines returned in a single tool output.
pub const MAX_LINES: usize = 2000;
/// Maximum bytes returned in a single tool output (50 KB).
pub const MAX_BYTES: usize = 50 * 1024;
/// Maximum length of a single line before truncation.
pub const MAX_LINE_LENGTH: usize = 500;

/// Result of a truncation operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TruncateResult {
    /// The (possibly truncated) text.
    pub text: String,
    /// Total number of lines in the original input.
    pub total_lines: usize,
    /// Number of lines included in the output.
    pub shown_lines: usize,
    /// Whether truncation occurred.
    pub truncated: bool,
}

/// Keep the first N lines within the byte and line limits.
///
/// Iterates lines from the start, accumulating bytes (including newlines).
/// Stops when either `max_lines` or `max_bytes` is reached.
/// Never returns partial lines.
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

    let mut byte_count: usize = 0;
    let mut kept = 0;

    for (i, line) in lines.iter().enumerate() {
        if i >= max_lines {
            break;
        }
        let line_bytes = line.len() + 1; // +1 for newline
        if byte_count + line_bytes > max_bytes && i > 0 {
            break;
        }
        byte_count += line_bytes;
        kept = i + 1;
    }

    let truncated = kept < total_lines;
    let result_text = lines[..kept].join("\n");

    TruncateResult {
        text: result_text,
        total_lines,
        shown_lines: kept,
        truncated,
    }
}

/// Keep the last N lines within the byte and line limits.
///
/// Iterates lines from the end, accumulating bytes backwards.
/// Stops when either `max_lines` or `max_bytes` is reached.
/// Never returns partial lines.
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

    let mut byte_count: usize = 0;
    let mut kept = 0;

    for (i, line) in lines.iter().rev().enumerate() {
        if i >= max_lines {
            break;
        }
        let line_bytes = line.len() + 1; // +1 for newline
        if byte_count + line_bytes > max_bytes && i > 0 {
            break;
        }
        byte_count += line_bytes;
        kept = i + 1;
    }

    let truncated = kept < total_lines;
    let start = total_lines - kept;
    let result_text = lines[start..].join("\n");

    TruncateResult {
        text: result_text,
        total_lines,
        shown_lines: kept,
        truncated,
    }
}

/// Truncate a single line if it exceeds `max_len` characters.
///
/// Appends `"... [truncated]"` to indicate the line was shortened.
pub fn truncate_line(line: &str, max_len: usize) -> String {
    if line.len() <= max_len {
        line.to_string()
    } else {
        let mut result = line[..max_len].to_string();
        result.push_str("... [truncated]");
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── truncate_head ──────────────────────────────────────────

    #[test]
    fn head_no_truncation_when_within_limits() {
        let text = "line1\nline2\nline3";
        let result = truncate_head(text, 10, 1024);
        assert_eq!(result.total_lines, 3);
        assert_eq!(result.shown_lines, 3);
        assert!(!result.truncated);
        assert_eq!(result.text, "line1\nline2\nline3");
    }

    #[test]
    fn head_truncates_by_line_limit() {
        let text = "a\nb\nc\nd\ne";
        let result = truncate_head(text, 3, 1024);
        assert_eq!(result.total_lines, 5);
        assert_eq!(result.shown_lines, 3);
        assert!(result.truncated);
        assert_eq!(result.text, "a\nb\nc");
    }

    #[test]
    fn head_truncates_by_byte_limit() {
        // Each line "aaaa" = 4 chars + 1 newline = 5 bytes
        let text = "aaaa\nbbbb\ncccc\ndddd";
        // Allow 12 bytes: should fit "aaaa\n" (5) + "bbbb\n" (5) = 10, then "cccc\n" (5) would be 15 > 12
        let result = truncate_head(text, 100, 12);
        assert_eq!(result.total_lines, 4);
        assert_eq!(result.shown_lines, 2);
        assert!(result.truncated);
        assert_eq!(result.text, "aaaa\nbbbb");
    }

    #[test]
    fn head_always_includes_at_least_one_line() {
        // Even if the first line exceeds byte limit, we still include it
        let text = "this is a long line\nshort";
        let result = truncate_head(text, 100, 5);
        assert_eq!(result.shown_lines, 1);
        assert!(result.truncated);
        assert_eq!(result.text, "this is a long line");
    }

    #[test]
    fn head_empty_input() {
        let result = truncate_head("", 100, 1024);
        assert_eq!(result.total_lines, 0);
        assert_eq!(result.shown_lines, 0);
        assert!(!result.truncated);
        assert_eq!(result.text, "");
    }

    #[test]
    fn head_single_line() {
        let result = truncate_head("hello", 100, 1024);
        assert_eq!(result.total_lines, 1);
        assert_eq!(result.shown_lines, 1);
        assert!(!result.truncated);
        assert_eq!(result.text, "hello");
    }

    // ── truncate_tail ──────────────────────────────────────────

    #[test]
    fn tail_no_truncation_when_within_limits() {
        let text = "line1\nline2\nline3";
        let result = truncate_tail(text, 10, 1024);
        assert_eq!(result.total_lines, 3);
        assert_eq!(result.shown_lines, 3);
        assert!(!result.truncated);
        assert_eq!(result.text, "line1\nline2\nline3");
    }

    #[test]
    fn tail_truncates_by_line_limit() {
        let text = "a\nb\nc\nd\ne";
        let result = truncate_tail(text, 3, 1024);
        assert_eq!(result.total_lines, 5);
        assert_eq!(result.shown_lines, 3);
        assert!(result.truncated);
        assert_eq!(result.text, "c\nd\ne");
    }

    #[test]
    fn tail_truncates_by_byte_limit() {
        // Each line "aaaa" = 4 chars + 1 newline = 5 bytes
        let text = "aaaa\nbbbb\ncccc\ndddd";
        // Allow 12 bytes: should fit "dddd\n" (5) + "cccc\n" (5) = 10, then "bbbb\n" (5) would be 15 > 12
        let result = truncate_tail(text, 100, 12);
        assert_eq!(result.total_lines, 4);
        assert_eq!(result.shown_lines, 2);
        assert!(result.truncated);
        assert_eq!(result.text, "cccc\ndddd");
    }

    #[test]
    fn tail_always_includes_at_least_one_line() {
        let text = "short\nthis is a long line";
        let result = truncate_tail(text, 100, 5);
        assert_eq!(result.shown_lines, 1);
        assert!(result.truncated);
        assert_eq!(result.text, "this is a long line");
    }

    #[test]
    fn tail_empty_input() {
        let result = truncate_tail("", 100, 1024);
        assert_eq!(result.total_lines, 0);
        assert_eq!(result.shown_lines, 0);
        assert!(!result.truncated);
        assert_eq!(result.text, "");
    }

    #[test]
    fn tail_single_line() {
        let result = truncate_tail("hello", 100, 1024);
        assert_eq!(result.total_lines, 1);
        assert_eq!(result.shown_lines, 1);
        assert!(!result.truncated);
        assert_eq!(result.text, "hello");
    }

    // ── truncate_line ──────────────────────────────────────────

    #[test]
    fn line_no_truncation_when_within_limit() {
        let line = "short line";
        assert_eq!(truncate_line(line, 500), "short line");
    }

    #[test]
    fn line_truncates_long_line() {
        let line = "a".repeat(600);
        let result = truncate_line(&line, 500);
        assert!(result.starts_with(&"a".repeat(500)));
        assert!(result.ends_with("... [truncated]"));
        assert_eq!(result.len(), 500 + "... [truncated]".len());
    }

    #[test]
    fn line_empty_input() {
        assert_eq!(truncate_line("", 500), "");
    }

    #[test]
    fn line_exact_boundary() {
        let line = "a".repeat(500);
        assert_eq!(truncate_line(&line, 500), line);
    }

    #[test]
    fn line_one_over_boundary() {
        let line = "a".repeat(501);
        let result = truncate_line(&line, 500);
        assert!(result.ends_with("... [truncated]"));
    }

    // ── constants ──────────────────────────────────────────────

    #[test]
    fn constants_have_expected_values() {
        assert_eq!(MAX_LINES, 2000);
        assert_eq!(MAX_BYTES, 50 * 1024);
        assert_eq!(MAX_LINE_LENGTH, 500);
    }
}
