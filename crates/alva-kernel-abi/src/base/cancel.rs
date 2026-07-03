// INPUT:  std::sync::Arc, tokio::sync::watch
// OUTPUT: pub struct CancellationToken
// POS:    Cooperative cancellation primitive backed by a tokio watch channel.
use std::sync::Arc;
use tokio::sync::watch;

#[derive(Clone)]
pub struct CancellationToken {
    sender: Arc<watch::Sender<bool>>,
    receiver: watch::Receiver<bool>,
    /// Read-only views of every ancestor channel (root first) for
    /// hierarchical tokens (see [`Self::child`]): a child also observes its
    /// ancestors' cancellation, but cancelling the child sends only on its
    /// own channel — nothing propagates upward.
    ancestors: Vec<watch::Receiver<bool>>,
    /// Keeps the paired ancestor senders alive so the channels in
    /// `ancestors` can never close (and `changed()` never errors) while a
    /// descendant exists.
    ancestor_senders: Vec<Arc<watch::Sender<bool>>>,
}

impl CancellationToken {
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(false);
        Self {
            sender: Arc::new(sender),
            receiver,
            ancestors: Vec::new(),
            ancestor_senders: Vec::new(),
        }
    }

    /// Derive a child token: cancelling `self` (or any ancestor) cancels the
    /// child, but cancelling the child does NOT reach `self`. Use this for
    /// sub-scopes with their own kill switch (e.g. a sub-agent whose
    /// wall-clock budget fires) that must not take the parent run down.
    ///
    /// `Clone`, by contrast, is a peer handle on the SAME token: cancel on
    /// either side is visible from the other.
    pub fn child(&self) -> Self {
        let (sender, receiver) = watch::channel(false);
        let mut ancestors = self.ancestors.clone();
        ancestors.push(self.receiver.clone());
        let mut ancestor_senders = self.ancestor_senders.clone();
        ancestor_senders.push(self.sender.clone());
        Self {
            sender: Arc::new(sender),
            receiver,
            ancestors,
            ancestor_senders,
        }
    }

    pub fn cancel(&self) {
        let _ = self.sender.send(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow() || self.ancestors.iter().any(|r| *r.borrow())
    }

    pub async fn cancelled(&mut self) {
        loop {
            if *self.receiver.borrow_and_update() {
                return;
            }
            if self.ancestors.iter_mut().any(|r| *r.borrow_and_update()) {
                return;
            }
            // Wait for a change on the own channel or any ancestor channel.
            // All their senders are held alive (self.sender /
            // ancestor_senders), so Err from `changed()` is unreachable; it
            // is still treated as "stop waiting" to preserve the previous
            // closed-channel contract.
            let Self {
                receiver,
                ancestors,
                ..
            } = self;
            type ChangedFut<'a> = std::pin::Pin<
                Box<
                    dyn core::future::Future<Output = Result<(), watch::error::RecvError>>
                        + Send
                        + 'a,
                >,
            >;
            let mut futs: Vec<ChangedFut<'_>> = Vec::with_capacity(1 + ancestors.len());
            futs.push(Box::pin(receiver.changed()));
            for r in ancestors.iter_mut() {
                futs.push(Box::pin(r.changed()));
            }
            let (res, _idx, _rest) = futures_util::future::select_all(futs).await;
            if res.is_err() {
                return;
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

    #[test]
    fn child_observes_parent_cancel() {
        let parent = CancellationToken::new();
        let child = parent.child();
        parent.cancel();
        assert!(child.is_cancelled(), "child must observe parent's cancel");
    }

    #[test]
    fn child_cancel_does_not_reach_parent() {
        // The load-bearing asymmetry: a child's cancel (e.g. its wall-clock
        // budget firing in run_child) must stop the child subtree ONLY.
        // With a plain clone the shared channel would take the whole parent
        // run down with it.
        let parent = CancellationToken::new();
        let child = parent.child();
        child.cancel();
        assert!(child.is_cancelled());
        assert!(
            !parent.is_cancelled(),
            "child cancel must not propagate upward"
        );
    }

    #[test]
    fn grandchild_observes_root_cancel() {
        let root = CancellationToken::new();
        let child = root.child();
        let grandchild = child.child();
        root.cancel();
        assert!(
            grandchild.is_cancelled(),
            "cancel must reach all descendants"
        );
    }

    #[test]
    fn grandchild_cancel_leaves_ancestors_untouched() {
        let root = CancellationToken::new();
        let child = root.child();
        let grandchild = child.child();
        grandchild.cancel();
        assert!(!child.is_cancelled());
        assert!(!root.is_cancelled());
    }

    #[test]
    fn child_created_after_parent_cancel_is_already_cancelled() {
        // Latch: a cancel that races ahead of the child spawning must not
        // be lost.
        let parent = CancellationToken::new();
        parent.cancel();
        let child = parent.child();
        assert!(child.is_cancelled());
    }

    #[test]
    fn sibling_children_are_independent() {
        let parent = CancellationToken::new();
        let a = parent.child();
        let b = parent.child();
        a.cancel();
        assert!(!b.is_cancelled(), "siblings must not share cancel state");
        assert!(!parent.is_cancelled());
    }

    #[test]
    fn clone_of_child_shares_child_state_not_parent() {
        // A clone of a child is a handle on the SAME child token: cancel on
        // the clone is visible from the child, and still does not reach the
        // parent.
        let parent = CancellationToken::new();
        let child = parent.child();
        let handle = child.clone();
        handle.cancel();
        assert!(child.is_cancelled());
        assert!(!parent.is_cancelled());
    }

    #[tokio::test]
    async fn cancelled_future_wakes_on_parent_cancel() {
        // The async path must observe the parent chain, not just the own
        // channel — a child blocked in `cancelled()` wakes when the PARENT
        // cancels.
        let parent = CancellationToken::new();
        let mut child = parent.child();

        let handle = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(2), child.cancelled())
                .await
                .expect("child cancelled() did not fire after parent cancel")
        });

        tokio::task::yield_now().await;
        parent.cancel();
        handle.await.expect("waiter task panicked");
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
