//! Small utility helpers shared across layers.

/// Claude-family heuristic cost estimate: $3/M input, $15/M output.
pub fn estimate_cost_usd(input_tokens: u64, output_tokens: u64) -> f64 {
    (input_tokens as f64 * 3.0 + output_tokens as f64 * 15.0) / 1_000_000.0
}

/// Compact number formatter. e.g. 1500 → "1.5K", 2_500_000 → "2.5M".
pub fn format_token_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    //! Tests for shared cost + token-count helpers. These power the
    //! "session cost / tokens used" line in the CLI status bar and
    //! the Tauri UI — silent drift here would mis-bill or mis-display
    //! across every session, hard to catch in manual UI checks.
    use super::*;

    // -- estimate_cost_usd -------------------------------------------------

    #[test]
    fn estimate_cost_zero_is_zero() {
        assert_eq!(estimate_cost_usd(0, 0), 0.0);
    }

    #[test]
    fn estimate_cost_one_million_input_is_three_dollars() {
        // $3 per 1M input tokens is the published Claude-family input rate
        // we're pinning here. Bump if the heuristic ever changes — also
        // forces a deliberate human review at that moment.
        assert_eq!(estimate_cost_usd(1_000_000, 0), 3.0);
    }

    #[test]
    fn estimate_cost_one_million_output_is_fifteen_dollars() {
        // $15 per 1M output tokens — same pin as above for output.
        assert_eq!(estimate_cost_usd(0, 1_000_000), 15.0);
    }

    #[test]
    fn estimate_cost_combined_input_and_output() {
        // 500K input + 200K output = (500_000 * 3 + 200_000 * 15) / 1M
        // = (1_500_000 + 3_000_000) / 1M = 4.5
        assert_eq!(estimate_cost_usd(500_000, 200_000), 4.5);
    }

    #[test]
    fn estimate_cost_sub_micro_token_amount_is_tiny_but_positive() {
        // 100 input tokens → 0.0003 USD (300 µ$). Smoke that
        // small-magnitude estimates don't truncate to zero.
        let cost = estimate_cost_usd(100, 0);
        assert!(cost > 0.0);
        assert!((cost - 0.0003).abs() < 1e-9);
    }

    // -- format_token_count ------------------------------------------------

    #[test]
    fn format_token_count_below_thousand_passes_through_decimal_string() {
        assert_eq!(format_token_count(0), "0");
        assert_eq!(format_token_count(1), "1");
        assert_eq!(format_token_count(999), "999");
    }

    #[test]
    fn format_token_count_exactly_1000_renders_as_1_0_kilo() {
        // Boundary pin: the >= 1_000 branch fires at exactly 1_000.
        assert_eq!(format_token_count(1_000), "1.0K");
    }

    #[test]
    fn format_token_count_1500_renders_as_1_5_kilo() {
        assert_eq!(format_token_count(1_500), "1.5K");
    }

    #[test]
    fn format_token_count_just_under_million_stays_in_kilo() {
        // 999_999 → "1000.0K" (not "1.0M" — the >= 1_000_000 branch
        // hasn't fired yet). Pin to detect a future off-by-one in the
        // boundary.
        assert_eq!(format_token_count(999_999), "1000.0K");
    }

    #[test]
    fn format_token_count_exactly_1_million_renders_as_1_0_mega() {
        // Boundary pin: the >= 1_000_000 branch fires at exactly 1M.
        assert_eq!(format_token_count(1_000_000), "1.0M");
    }

    #[test]
    fn format_token_count_2_5_million_renders_as_2_5_mega() {
        assert_eq!(format_token_count(2_500_000), "2.5M");
    }

    #[test]
    fn format_token_count_large_value_renders_with_one_decimal() {
        // Higher-magnitude pin — confirms the {:.1} format spec persists
        // and doesn't accidentally switch to integer or full precision.
        assert_eq!(format_token_count(12_345_678), "12.3M");
    }
}
