//! Rate limiting for LLM provider requests.
//!
//! Tracks request counts within rolling windows and overage state
//! based on API response headers. Modeled after Claude Code's
//! `claudeAiLimits.ts` pattern.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

/// Rate limit state tracking (matching Claude Code's claudeAiLimits.ts)
#[derive(Debug)]
pub struct RateLimitState {
    /// Requests made in the current 5-hour window
    pub requests_in_window: AtomicU64,
    /// Window start time (unix seconds)
    pub window_start: AtomicU64,
    /// Total requests in 7-day period
    pub requests_7day: AtomicU64,
    /// Whether we're in overage state
    pub is_overage: AtomicBool,
}

impl RateLimitState {
    pub fn new() -> Self {
        Self {
            requests_in_window: AtomicU64::new(0),
            window_start: AtomicU64::new(now_secs()),
            requests_7day: AtomicU64::new(0),
            is_overage: AtomicBool::new(false),
        }
    }

    /// Record a request and check if rate limited.
    pub fn record_request(&self) -> RateLimitCheck {
        let now = now_secs();
        let window = self.window_start.load(Ordering::Relaxed);

        // Reset window if 5 hours passed
        if now - window > 5 * 3600 {
            self.window_start.store(now, Ordering::Relaxed);
            self.requests_in_window.store(0, Ordering::Relaxed);
        }

        let count = self.requests_in_window.fetch_add(1, Ordering::Relaxed) + 1;
        self.requests_7day.fetch_add(1, Ordering::Relaxed);

        RateLimitCheck {
            requests_in_window: count,
            window_remaining_secs: 5 * 3600 - (now - self.window_start.load(Ordering::Relaxed)),
            is_overage: self.is_overage.load(Ordering::Relaxed),
        }
    }

    /// Update state from API response headers.
    pub fn update_from_headers(&self, headers: &[(String, String)]) {
        for (key, value) in headers {
            match key.to_lowercase().as_str() {
                "x-ratelimit-remaining" => {
                    // If remaining is 0, we're rate limited
                    if value == "0" {
                        self.is_overage.store(true, Ordering::Relaxed);
                    }
                }
                "retry-after" => {
                    // Rate limited, mark overage
                    self.is_overage.store(true, Ordering::Relaxed);
                }
                _ => {}
            }
        }
    }

    /// Reset overage state.
    pub fn clear_overage(&self) {
        self.is_overage.store(false, Ordering::Relaxed);
    }
}

impl Default for RateLimitState {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a rate limit check after recording a request.
#[derive(Debug, Clone)]
pub struct RateLimitCheck {
    pub requests_in_window: u64,
    pub window_remaining_secs: u64,
    pub is_overage: bool,
}

impl RateLimitCheck {
    /// Whether the user should be warned about approaching limits.
    pub fn is_approaching_limit(&self, max_requests: u64) -> bool {
        let threshold = (max_requests as f64 * 0.8) as u64;
        self.requests_in_window >= threshold
    }
}

/// Rate limit type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateLimitType {
    /// 5-hour rolling window
    FiveHour,
    /// 7-day rolling window
    SevenDay,
    /// Overage (exceeded plan limits)
    Overage,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_starts_at_zero() {
        let state = RateLimitState::new();
        assert_eq!(state.requests_in_window.load(Ordering::Relaxed), 0);
        assert_eq!(state.requests_7day.load(Ordering::Relaxed), 0);
        assert!(!state.is_overage.load(Ordering::Relaxed));
    }

    #[test]
    fn record_request_increments_counts() {
        let state = RateLimitState::new();
        let check = state.record_request();
        assert_eq!(check.requests_in_window, 1);
        assert!(!check.is_overage);

        let check2 = state.record_request();
        assert_eq!(check2.requests_in_window, 2);
    }

    #[test]
    fn is_approaching_limit_threshold() {
        let check = RateLimitCheck {
            requests_in_window: 79,
            window_remaining_secs: 3600,
            is_overage: false,
        };
        assert!(!check.is_approaching_limit(100));

        let check_at_80 = RateLimitCheck {
            requests_in_window: 80,
            window_remaining_secs: 3600,
            is_overage: false,
        };
        assert!(check_at_80.is_approaching_limit(100));
    }

    #[test]
    fn update_from_headers_sets_overage_on_zero_remaining() {
        let state = RateLimitState::new();
        state.update_from_headers(&[
            ("x-ratelimit-remaining".to_string(), "0".to_string()),
        ]);
        assert!(state.is_overage.load(Ordering::Relaxed));
    }

    #[test]
    fn update_from_headers_sets_overage_on_retry_after() {
        let state = RateLimitState::new();
        state.update_from_headers(&[
            ("retry-after".to_string(), "60".to_string()),
        ]);
        assert!(state.is_overage.load(Ordering::Relaxed));
    }

    #[test]
    fn update_from_headers_ignores_nonzero_remaining() {
        let state = RateLimitState::new();
        state.update_from_headers(&[
            ("x-ratelimit-remaining".to_string(), "42".to_string()),
        ]);
        assert!(!state.is_overage.load(Ordering::Relaxed));
    }

    #[test]
    fn clear_overage_resets_flag() {
        let state = RateLimitState::new();
        state.is_overage.store(true, Ordering::Relaxed);
        state.clear_overage();
        assert!(!state.is_overage.load(Ordering::Relaxed));
    }

    #[test]
    fn rate_limit_type_serde_roundtrip() {
        let types = [RateLimitType::FiveHour, RateLimitType::SevenDay, RateLimitType::Overage];
        for t in &types {
            let json = serde_json::to_string(t).unwrap();
            let parsed: RateLimitType = serde_json::from_str(&json).unwrap();
            assert_eq!(*t, parsed);
        }
    }

    #[test]
    fn default_impl_works() {
        let state = RateLimitState::default();
        assert_eq!(state.requests_in_window.load(Ordering::Relaxed), 0);
    }
}
