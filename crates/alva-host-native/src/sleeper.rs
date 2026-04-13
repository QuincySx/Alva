// INPUT:  std::time::Duration, async_trait, alva_kernel_abi::Sleeper
// OUTPUT: TokioSleeper
// POS:    Native Sleeper impl backed by tokio::time::sleep — the production driver for kernel timeouts.

//! `TokioSleeper` — the native `Sleeper` implementation.
//!
//! `alva-host-native` provides this so the kernel's `Sleeper` trait
//! (defined in `alva-kernel-abi`) gets a real driver when running under
//! tokio. Other host crates (`alva-host-wasm` etc.) will provide their
//! own `Sleeper` impls without touching the kernel.

use std::time::Duration;

use alva_kernel_abi::Sleeper;
use async_trait::async_trait;

/// Native sleeper backed by `tokio::time::sleep`. Stateless, cheap to
/// `Arc::new`, safe to share across the entire host runtime.
#[derive(Debug, Default, Clone, Copy)]
pub struct TokioSleeper;

#[async_trait]
impl Sleeper for TokioSleeper {
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::timeout;
    use std::sync::Arc;

    #[tokio::test]
    async fn tokio_sleeper_actually_times_out() {
        let sleeper: Arc<dyn Sleeper> = Arc::new(TokioSleeper);
        // Future that never completes vs a 10ms timeout — the timeout wins.
        let result = timeout(
            sleeper.as_ref(),
            Duration::from_millis(10),
            std::future::pending::<()>(),
        )
        .await;
        assert!(result.is_err(), "timeout should have fired");
    }

    #[tokio::test]
    async fn tokio_sleeper_lets_fast_future_win() {
        let sleeper: Arc<dyn Sleeper> = Arc::new(TokioSleeper);
        // Future that completes immediately vs a long timeout — the future wins.
        let result =
            timeout(sleeper.as_ref(), Duration::from_secs(60), async { 42 }).await;
        assert_eq!(result, Ok(42));
    }
}
