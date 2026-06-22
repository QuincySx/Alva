// INPUT:  std::str
// OUTPUT: pub(crate) fn truncate_for_display
// POS:    Small UTF-8-safe display utilities used across the crate.
//
// Why this lives in its own module rather than inside sdk_impl.rs:
// three callsites — sdk_impl.rs (×2), system_context.rs, compact.rs —
// were each copy-pasting the same 5-line `is_char_boundary` back-off
// pattern. Centralised here as `pub(crate)` so all of them share one
// helper + one test suite. Any new callsite in this crate should
// just `use crate::util::truncate_for_display;`.

/// UTF-8 safe display truncation.
///
/// If `s` is at most `max_bytes` bytes, returns `s` unchanged. Otherwise
/// finds the largest valid char boundary at or below `max_bytes`, slices
/// there, and appends `marker`.
///
/// This avoids `&s[..n]`'s panic when `n` lands inside a multi-byte
/// UTF-8 char (4-byte emoji, 3-byte CJK). Use whenever truncating
/// LLM output, user input, tool output, or git status for display.
pub(crate) fn truncate_for_display(s: &str, max_bytes: usize, marker: &str) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{}", &s[..end], marker)
}

#[cfg(test)]
mod tests {
    //! Consolidated tests for `truncate_for_display`. These cover the
    //! pure helper itself; each callsite (sdk_impl / system_context /
    //! compact) retains its own integration-level regression test that
    //! exercises the helper via its real call path.
    use super::*;

    #[test]
    fn short_string_returned_unchanged() {
        assert_eq!(truncate_for_display("hello", 100, "..."), "hello");
    }

    #[test]
    fn exact_byte_length_returned_unchanged() {
        // s.len() == max_bytes hits the `<= max_bytes` branch.
        assert_eq!(truncate_for_display("abcdef", 6, "..."), "abcdef");
    }

    #[test]
    fn ascii_truncates_at_exact_byte_with_marker() {
        let s = "abcdefghij"; // 10 bytes
        assert_eq!(truncate_for_display(s, 4, "..."), "abcd...");
    }

    #[test]
    fn empty_marker_just_hard_cuts() {
        // Used by the summary-with-hints path (sdk_impl L396): no marker.
        let s = "a".repeat(500);
        let out = truncate_for_display(&s, 100, "");
        assert_eq!(out.len(), 100);
    }

    #[test]
    fn backs_off_from_mid_emoji_to_char_boundary() {
        // "a🦀b" = 1 + 4 + 1 = 6 bytes. max=3 → byte 3 inside 🦀 → back
        // off to byte 1 (the boundary before the emoji).
        let s = "a🦀b";
        assert_eq!(s.len(), 6);
        assert!(!s.is_char_boundary(3), "test premise: byte 3 mid-char");
        assert_eq!(truncate_for_display(s, 3, "..."), "a...");
    }

    #[test]
    fn backs_off_from_mid_cjk_to_char_boundary() {
        // "a中b" = 1 + 3 + 1 = 5. max=2 → byte 2 inside 中 → back to 1.
        let s = "a中b";
        assert!(!s.is_char_boundary(2));
        assert_eq!(
            truncate_for_display(s, 2, "...(truncated)"),
            "a...(truncated)"
        );
    }

    #[test]
    fn handles_max_bytes_zero() {
        // Long string: while loop exits immediately, end=0 → format!
        // gives just the marker. Empty string: len <= 0 → unchanged.
        assert_eq!(truncate_for_display("anything", 0, "..."), "...");
        assert_eq!(truncate_for_display("", 0, "..."), "");
    }

    #[test]
    fn realistic_2000_byte_emoji_at_boundary_does_not_crash() {
        // Regression guard: 1998 ASCII + 1 emoji (4 bytes) + 100 ASCII
        // = 2102 bytes; byte 2000 lands inside the emoji (bytes 1998..2002).
        // Naive `&s[..2000]` would panic; we must back off to byte 1998.
        let s = format!("{}{}{}", "a".repeat(1998), "🦀", "b".repeat(100));
        assert_eq!(s.len(), 2102);
        assert!(!s.is_char_boundary(2000));
        let out = truncate_for_display(&s, 2000, "...(truncated)");
        assert!(out.ends_with("...(truncated)"));
        // Kept portion must be at most 2000 bytes and end on a char
        // boundary (here byte 1998, just before the emoji).
        let kept = out.strip_suffix("...(truncated)").unwrap();
        assert!(kept.len() <= 2000);
        assert!(kept.is_char_boundary(kept.len()));
    }
}
