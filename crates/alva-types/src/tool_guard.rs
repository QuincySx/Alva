// INPUT:  std::sync::atomic, std::sync::Arc
// OUTPUT: ToolGuard, GuardToken, GuardError
// POS:    Reusable execution limiter for tools — depth control, concurrency control, cooldown.

//! Tool execution guards — reusable limiters that any Tool can embed.
//!
//! # Examples
//!
//! ```rust,ignore
//! use alva_types::tool_guard::ToolGuard;
//!
//! // Max depth = 2 (one level of nesting allowed)
//! let guard = ToolGuard::max_depth(2);
//!
//! // In Tool::execute():
//! let _token = guard.try_acquire("team")
//!     .map_err(|e| /* return error to LLM */)?;
//! // ... do work ...
//! // token drops → depth decremented automatically
//! ```

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Reusable execution guard for tools.
///
/// Tracks active invocations and enforces limits. Thread-safe via atomics.
#[derive(Clone)]
pub struct ToolGuard {
    active: Arc<AtomicU32>,
    max: u32,
    kind: GuardKind,
}

#[derive(Clone, Debug)]
enum GuardKind {
    /// Limits nesting depth (recursive calls).
    Depth,
    /// Limits concurrent invocations.
    Concurrency,
}

/// RAII token — decrements the counter on drop.
#[derive(Debug)]
pub struct GuardToken {
    active: Arc<AtomicU32>,
}

impl Drop for GuardToken {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Error when guard refuses execution.
#[derive(Debug, Clone)]
pub struct GuardError {
    pub tool_name: String,
    pub current: u32,
    pub max: u32,
    pub message: String,
}

impl std::fmt::Display for GuardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl ToolGuard {
    /// Create a depth guard: limits how many nested calls can be active.
    ///
    /// `max_depth=1` means no nesting (only one call at a time).
    /// `max_depth=2` means one level of nesting.
    pub fn max_depth(max: u32) -> Self {
        Self {
            active: Arc::new(AtomicU32::new(0)),
            max,
            kind: GuardKind::Depth,
        }
    }

    /// Create a concurrency guard: limits how many parallel calls can run.
    ///
    /// `max_concurrent=3` means at most 3 simultaneous invocations.
    pub fn max_concurrent(max: u32) -> Self {
        Self {
            active: Arc::new(AtomicU32::new(0)),
            max,
            kind: GuardKind::Concurrency,
        }
    }

    /// Try to acquire a token. Returns `Err` if the limit is reached.
    ///
    /// The returned `GuardToken` decrements the counter on drop,
    /// so hold it for the duration of the tool execution.
    pub fn try_acquire(&self, tool_name: &str) -> Result<GuardToken, GuardError> {
        let current = self.active.load(Ordering::SeqCst);
        if current >= self.max {
            let reason = match self.kind {
                GuardKind::Depth => format!(
                    "Cannot nest {} — already at depth {}/{}. \
                     Handle the task directly instead of delegating further.",
                    tool_name, current, self.max,
                ),
                GuardKind::Concurrency => format!(
                    "Cannot run {} — {}/{} concurrent invocations already active. \
                     Wait for one to complete before starting another.",
                    tool_name, current, self.max,
                ),
            };
            return Err(GuardError {
                tool_name: tool_name.to_string(),
                current,
                max: self.max,
                message: reason,
            });
        }

        self.active.fetch_add(1, Ordering::SeqCst);
        Ok(GuardToken {
            active: self.active.clone(),
        })
    }

    /// Current active count.
    pub fn active_count(&self) -> u32 {
        self.active.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_guard_allows_within_limit() {
        let guard = ToolGuard::max_depth(2);
        let t1 = guard.try_acquire("test").unwrap();
        let t2 = guard.try_acquire("test").unwrap();
        assert_eq!(guard.active_count(), 2);

        let result = guard.try_acquire("test");
        assert!(result.is_err());

        drop(t1);
        assert_eq!(guard.active_count(), 1);

        let t3 = guard.try_acquire("test").unwrap();
        assert_eq!(guard.active_count(), 2);

        drop(t2);
        drop(t3);
        assert_eq!(guard.active_count(), 0);
    }

    #[test]
    fn depth_guard_refuses_at_limit() {
        let guard = ToolGuard::max_depth(1);
        let _t1 = guard.try_acquire("team").unwrap();

        let err = guard.try_acquire("team").unwrap_err();
        assert!(err.message.contains("Cannot nest"));
        assert_eq!(err.current, 1);
        assert_eq!(err.max, 1);
    }

    #[test]
    fn concurrency_guard_message() {
        let guard = ToolGuard::max_concurrent(1);
        let _t1 = guard.try_acquire("shell").unwrap();

        let err = guard.try_acquire("shell").unwrap_err();
        assert!(err.message.contains("concurrent"));
    }

    #[test]
    fn clone_shares_state() {
        let g1 = ToolGuard::max_depth(2);
        let g2 = g1.clone();

        let _t1 = g1.try_acquire("x").unwrap();
        assert_eq!(g2.active_count(), 1);

        let _t2 = g2.try_acquire("x").unwrap();
        assert_eq!(g1.active_count(), 2);
    }

    #[test]
    fn drop_decrements_even_without_explicit_drop() {
        let guard = ToolGuard::max_depth(1);
        {
            let _t = guard.try_acquire("x").unwrap();
            assert_eq!(guard.active_count(), 1);
        }
        // _t dropped here
        assert_eq!(guard.active_count(), 0);
    }
}
