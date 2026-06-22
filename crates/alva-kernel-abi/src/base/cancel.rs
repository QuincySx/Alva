// INPUT:  std::sync::Arc, tokio::sync::watch
// OUTPUT: pub struct CancellationToken
// POS:    Cooperative cancellation primitive backed by a tokio watch channel.
use std::sync::Arc;
use tokio::sync::watch;

#[derive(Clone)]
pub struct CancellationToken {
    sender: Arc<watch::Sender<bool>>,
    receiver: watch::Receiver<bool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(false);
        Self {
            sender: Arc::new(sender),
            receiver,
        }
    }

    pub fn cancel(&self) {
        let _ = self.sender.send(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    pub async fn cancelled(&mut self) {
        while !*self.receiver.borrow_and_update() {
            if self.receiver.changed().await.is_err() {
                break;
            }
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    //! Tests for CancellationToken.
    //!
    //! The load-bearing invariant is that `Clone` shares state with
    //! the original — cancelling on any handle must be visible from
    //! every other handle, including handles created BEFORE the
    //! cancel and handles awaiting `cancelled()` on another task.
    //! Without that, cancellation requests get silently swallowed
    //! and dependent tasks run forever.
    use super::*;
    use std::time::Duration;

    #[test]
    fn new_token_is_not_cancelled() {
        let t = CancellationToken::new();
        assert!(!t.is_cancelled());
    }

    #[test]
    fn default_token_is_not_cancelled() {
        let t = CancellationToken::default();
        assert!(!t.is_cancelled());
    }

    #[test]
    fn cancel_sets_is_cancelled_to_true() {
        let t = CancellationToken::new();
        t.cancel();
        assert!(t.is_cancelled());
    }

    #[test]
    fn multiple_cancel_calls_are_idempotent() {
        // The watch::Sender::send is best-effort (we explicitly
        // `let _ =`), so a second `cancel` must NOT panic — pin so
        // a refactor adding `.unwrap()` doesn't break long-running
        // callers that may cancel multiple times defensively.
        let t = CancellationToken::new();
        t.cancel();
        t.cancel();
        t.cancel();
        assert!(t.is_cancelled());
    }

    #[test]
    fn clone_shares_state_cancel_on_original_visible_from_clone() {
        // Core contract: cloning produces a handle that observes
        // cancellation on the SAME underlying channel. Without this,
        // `.clone()` to pass into a spawned task would receive a
        // fresh token that never fires.
        let original = CancellationToken::new();
        let cloned = original.clone();
        original.cancel();
        assert!(
            cloned.is_cancelled(),
            "clone must observe original's cancel"
        );
    }

    #[test]
    fn clone_shares_state_cancel_on_clone_visible_from_original() {
        // Symmetric pin: cancellation propagates BACK to the original
        // when triggered from a clone. Without this, a spawned task
        // calling `.cancel()` on its handle wouldn't unblock the
        // parent task waiting on the original handle.
        let original = CancellationToken::new();
        let cloned = original.clone();
        cloned.cancel();
        assert!(original.is_cancelled());
    }

    #[test]
    fn cancel_visible_across_multiple_clones() {
        // 3-way clone graph: cancel on one is visible from all
        // sibling clones (not just the parent/child immediate pair).
        let a = CancellationToken::new();
        let b = a.clone();
        let c = a.clone();
        b.cancel();
        assert!(a.is_cancelled());
        assert!(c.is_cancelled());
    }

    #[tokio::test]
    async fn cancelled_future_returns_immediately_after_pre_cancel() {
        // If cancel happened before the await, cancelled() must
        // return immediately rather than block. Without this, a
        // task that races (cancel happens before it polls cancelled)
        // would never wake up.
        let mut t = CancellationToken::new();
        t.cancel();
        // Wrap in a timeout to fail loudly if it actually blocks.
        let result = tokio::time::timeout(Duration::from_millis(100), t.cancelled()).await;
        assert!(
            result.is_ok(),
            "cancelled() must complete fast when already cancelled"
        );
    }

    #[tokio::test]
    async fn cancelled_future_wakes_when_other_handle_cancels() {
        // Concurrent path: spawn a task waiting on cancelled();
        // cancel from a sibling handle; the waiter must wake.
        let mut waiter = CancellationToken::new();
        let trigger = waiter.clone();

        let handle = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(2), waiter.cancelled())
                .await
                .expect("cancelled() did not fire after sibling cancel")
        });

        // Tiny yield so the spawned task gets a chance to poll.
        tokio::task::yield_now().await;
        trigger.cancel();
        handle.await.expect("waiter task panicked");
    }
}
