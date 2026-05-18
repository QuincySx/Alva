// INPUT:  std::str
// OUTPUT: pub(crate) fn truncate_for_log
// POS:    Small UTF-8-safe slice helper used by every provider's
//         tracing previews (request body / response body / SSE chunks).

/// UTF-8 safe slice for use in `tracing` macros' body/data/chunk
/// preview fields. Naive `&s[..s.len().min(N)]` panics if byte N
/// lands inside a multi-byte char — realistic for HTTP response
/// bodies and SSE chunks that include emoji or CJK. A panic in a
/// tracing call brings down the request handler.
///
/// Returns `&str` (not `String`) so it can be passed directly to
/// `tracing::debug!` / `tracing::warn!` without an extra allocation.
///
/// History: each provider (anthropic, openai_responses, openai_chat,
/// gemini) carried its own file-private copy of this helper across
/// L68-L71 before being consolidated here in L72.
pub(crate) fn truncate_for_log(s: &str, max_bytes: usize) -> &str {
    let mut end = s.len().min(max_bytes);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    //! Consolidated tests for `truncate_for_log`. These cover the
    //! pure helper itself; each provider file retains its
    //! integration-style regression test that exercises the helper
    //! via that provider's specific tracing call paths and locale
    //! scenario (Anthropic-Chinese / OpenAI-Responses-reasoning /
    //! OpenAI-Chat-Chinese / Gemini-Japanese).
    use super::*;

    #[test]
    fn short_string_returns_whole_string() {
        assert_eq!(truncate_for_log("hello", 500), "hello");
    }

    #[test]
    fn exact_byte_length_returns_whole_string() {
        let s = "a".repeat(500);
        assert_eq!(truncate_for_log(&s, 500), s.as_str());
    }

    #[test]
    fn long_ascii_truncates_at_max_bytes() {
        let s = "a".repeat(800);
        assert_eq!(truncate_for_log(&s, 500).len(), 500);
    }

    #[test]
    fn backs_off_from_mid_emoji_at_boundary() {
        // 498 ASCII + 🦀 (4 bytes) + tail = 602 bytes; byte 500 inside
        // emoji → back off to byte 498.
        let s = format!("{}{}{}", "a".repeat(498), "🦀", "b".repeat(100));
        assert_eq!(s.len(), 602);
        assert!(!s.is_char_boundary(500));
        let out = truncate_for_log(&s, 500);
        assert!(out.len() <= 500);
        assert!(out.is_char_boundary(out.len()));
        assert!(!out.contains("🦀"));
    }

    #[test]
    fn backs_off_from_mid_cjk_at_boundary() {
        // CJK char "中" = 3 bytes. 200 of them = 600 bytes; byte 500
        // is inside the 167th char (bytes 498..501).
        let s = "中".repeat(200);
        assert_eq!(s.len(), 600);
        assert!(!s.is_char_boundary(500));
        let out = truncate_for_log(&s, 500);
        assert!(out.len() <= 500);
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn three_hundred_byte_first_chunk_limit_pattern() {
        // openai_chat's first-chunk preview uses 300 bytes — verify
        // the helper handles this less-common limit correctly.
        let s = format!("{}{}{}", "x".repeat(298), "🦀", "y".repeat(100));
        assert_eq!(s.len(), 402);
        assert!(!s.is_char_boundary(300));
        let out = truncate_for_log(&s, 300);
        assert!(out.len() <= 300);
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn sse_chunk_200_byte_limit_pattern() {
        let s = format!("{}{}{}", "x".repeat(198), "🦀", "y".repeat(50));
        assert_eq!(s.len(), 252);
        assert!(!s.is_char_boundary(200));
        let out = truncate_for_log(&s, 200);
        assert!(out.len() <= 200);
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn zero_max_bytes_returns_empty() {
        assert_eq!(truncate_for_log("anything", 0), "");
    }
}
