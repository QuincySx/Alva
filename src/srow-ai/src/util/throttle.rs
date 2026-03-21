use std::time::{Duration, Instant};

/// Simple time-based throttle. `should_emit()` returns true at most once per `interval`.
pub struct Throttle {
    interval: Duration,
    last_emit: Option<Instant>,
}

impl Throttle {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_emit: None,
        }
    }

    pub fn should_emit(&mut self) -> bool {
        let now = Instant::now();
        match self.last_emit {
            None => {
                self.last_emit = Some(now);
                true
            }
            Some(last) if now.duration_since(last) >= self.interval => {
                self.last_emit = Some(now);
                true
            }
            _ => false,
        }
    }

    /// Force the next call to should_emit() to return true.
    pub fn reset(&mut self) {
        self.last_emit = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_first_call_passes() {
        let mut t = Throttle::new(Duration::from_millis(100));
        assert!(t.should_emit());
    }

    #[test]
    fn test_rapid_calls_throttled() {
        let mut t = Throttle::new(Duration::from_secs(10));
        assert!(t.should_emit());
        assert!(!t.should_emit());
        assert!(!t.should_emit());
    }

    #[test]
    fn test_reset_allows_next_call() {
        let mut t = Throttle::new(Duration::from_secs(10));
        assert!(t.should_emit());
        assert!(!t.should_emit());
        t.reset();
        assert!(t.should_emit());
    }
}
