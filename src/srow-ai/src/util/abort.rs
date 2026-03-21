use tokio::sync::watch;

/// Cancel controller — the "sender" side. Call abort() to signal cancellation.
pub struct AbortController {
    cancel_tx: watch::Sender<bool>,
}

/// Cancel handle — the "receiver" side. Clone-able, pass to consumers.
#[derive(Clone)]
pub struct AbortHandle {
    cancel_rx: watch::Receiver<bool>,
}

impl AbortController {
    pub fn new() -> (Self, AbortHandle) {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        (Self { cancel_tx }, AbortHandle { cancel_rx })
    }

    pub fn abort(&self) {
        let _ = self.cancel_tx.send(true);
    }
}

impl AbortHandle {
    pub fn is_aborted(&self) -> bool {
        *self.cancel_rx.borrow()
    }

    pub async fn cancelled(&mut self) {
        while !*self.cancel_rx.borrow() {
            if self.cancel_rx.changed().await.is_err() {
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_abort_signal_received() {
        let (controller, handle) = AbortController::new();
        assert!(!handle.is_aborted());
        controller.abort();
        assert!(handle.is_aborted());
    }

    #[tokio::test]
    async fn test_cancelled_resolves_on_abort() {
        let (controller, mut handle) = AbortController::new();
        let task = tokio::spawn(async move {
            handle.cancelled().await;
            true
        });
        tokio::task::yield_now().await;
        controller.abort();
        assert!(task.await.unwrap());
    }

    #[tokio::test]
    async fn test_clone_handle() {
        let (controller, handle) = AbortController::new();
        let handle2 = handle.clone();
        controller.abort();
        assert!(handle.is_aborted());
        assert!(handle2.is_aborted());
    }
}
