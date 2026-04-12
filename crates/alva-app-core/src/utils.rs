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
