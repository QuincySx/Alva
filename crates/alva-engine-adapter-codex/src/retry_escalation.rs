// INPUT:  std::time::Instant
// OUTPUT: RecoverableRetryTracker, escalate thresholds / message builder
// POS:    cindy-style cap on recoverable/network retry loops so a turn cannot spin forever.

//! Cap unbounded recoverable-error loops (cindy `retry-escalation`).
//!
//! Background: Codex daemon / stream paths can emit `willRetry`-style network
//! errors indefinitely (`Reconnecting... N/M`). Without a host-side ceiling the
//! UI stays "running" forever. This tracker is independent of the exec vs
//! app-server transport — adapters call [`RecoverableRetryTracker::track`] on
//! each recoverable `RuntimeEvent::Error` and escalate to a fatal close.

use std::time::Instant;

/// Same turn retry count before host-side escalation (~1/s ≈ 30s).
pub const RETRY_ESCALATION_MAX_COUNT: u32 = 30;

/// Wall time from first recoverable error before escalation (2 minutes).
pub const RETRY_ESCALATION_MAX_ELAPSED_MS: u64 = 120_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryEscalationDecision {
    pub escalate: bool,
    pub retry_count: u32,
    pub elapsed_ms: u64,
}

/// Counts recoverable errors within a single execute() stream.
#[derive(Debug, Default)]
pub struct RecoverableRetryTracker {
    count: u32,
    first_at: Option<Instant>,
}

impl RecoverableRetryTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record one recoverable error. Returns whether the host should escalate.
    pub fn track(&mut self, now: Instant) -> RetryEscalationDecision {
        if self.first_at.is_none() {
            self.first_at = Some(now);
        }
        self.count = self.count.saturating_add(1);
        let elapsed_ms = self
            .first_at
            .map(|t| now.saturating_duration_since(t).as_millis() as u64)
            .unwrap_or(0);
        RetryEscalationDecision {
            escalate: self.count >= RETRY_ESCALATION_MAX_COUNT
                || elapsed_ms >= RETRY_ESCALATION_MAX_ELAPSED_MS,
            retry_count: self.count,
            elapsed_ms,
        }
    }

    pub fn reset(&mut self) {
        self.count = 0;
        self.first_at = None;
    }
}

/// User-facing fatal message after escalation (cindy `buildBackendUnreachableMessage` lite).
pub fn build_escalation_message(decision: &RetryEscalationDecision, last_error: &str) -> String {
    let last = last_error.trim().replace('\n', " ");
    let last = if last.chars().count() > 300 {
        format!("{}…", last.chars().take(300).collect::<String>())
    } else {
        last
    };
    let elapsed_s = decision.elapsed_ms / 1000;
    format!(
        "Codex backend unreachable — host escalated after {} recoverable errors over {}s \
         (last error: \"{last}\"). Check network/proxy/VPN and retry.",
        decision.retry_count, elapsed_s
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn escalates_on_count_threshold() {
        let mut t = RecoverableRetryTracker::new();
        let start = Instant::now();
        for i in 1..=RETRY_ESCALATION_MAX_COUNT {
            let d = t.track(start);
            if i < RETRY_ESCALATION_MAX_COUNT {
                assert!(!d.escalate, "count {i} should not escalate");
            } else {
                assert!(d.escalate);
                assert_eq!(d.retry_count, RETRY_ESCALATION_MAX_COUNT);
            }
        }
    }

    #[test]
    fn escalates_on_elapsed_threshold() {
        let mut t = RecoverableRetryTracker::new();
        let start = Instant::now();
        let early = t.track(start);
        assert!(!early.escalate);
        let late = t.track(start + Duration::from_millis(RETRY_ESCALATION_MAX_ELAPSED_MS));
        assert!(late.escalate);
        assert_eq!(late.retry_count, 2);
    }

    #[test]
    fn escalation_message_keeps_last_error_anchor() {
        let decision = RetryEscalationDecision {
            escalate: true,
            retry_count: 30,
            elapsed_ms: 35_000,
        };
        let msg = build_escalation_message(&decision, "ECONNREFUSED 127.0.0.1:1");
        assert!(msg.contains("30 recoverable"));
        assert!(msg.contains("ECONNREFUSED"));
    }
}
