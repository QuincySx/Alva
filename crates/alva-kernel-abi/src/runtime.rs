// INPUT:  std::future::Future, std::time::Duration, async_trait, futures_util::future::{select, Either}
// OUTPUT: Sleeper, TimeoutError, timeout
// POS:    Runtime-agnostic sleep + timeout primitives — kernel does not depend on tokio::time.

//! Runtime-agnostic timing primitives.
//!
//! The kernel layers (`alva-kernel-{bus,abi,core}`) must not depend on any
//! specific async runtime, so they can compile and run on native (tokio),
//! wasm32 (`wasm-bindgen-futures`), or any future driver. This module defines
//! the minimum trait the kernel needs from a runtime — a way to wait for a
//! duration — and provides a runtime-agnostic `timeout` helper built on top.
//!
//! Concrete implementations live outside the kernel:
//! - `alva-host-native::TokioSleeper` — production native impl using `tokio::time::sleep`
//! - (future) `alva-host-wasm::WasmSleeper` — wasm impl using `gloo-timers`
//!
//! The kernel never constructs a `Sleeper` itself; the host装配层 passes one in.

use std::future::Future;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::future::{select, Either};

/// A runtime-agnostic primitive for waiting a fixed duration.
///
/// Implementations should never block the OS thread; they should suspend the
/// current task until the duration elapses.
#[async_trait]
pub trait Sleeper: Send + Sync {
    async fn sleep(&self, duration: Duration);
}

/// Returned by [`timeout`] when the future does not complete in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutError;

impl std::fmt::Display for TimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "operation timed out")
    }
}

impl std::error::Error for TimeoutError {}

/// Race a future against a timeout. Returns `Ok(value)` if `fut` completes
/// before `duration` elapses, or `Err(TimeoutError)` otherwise.
///
/// This is a runtime-agnostic equivalent of `tokio::time::timeout`. It uses
/// `futures::future::select` under the hood, which has no runtime dependency
/// — the actual time tracking happens inside the `Sleeper` impl.
pub async fn timeout<T, F>(
    sleeper: &dyn Sleeper,
    duration: Duration,
    fut: F,
) -> Result<T, TimeoutError>
where
    F: Future<Output = T>,
{
    let user_fut = Box::pin(fut);
    let sleep_fut = Box::pin(sleeper.sleep(duration));
    match select(user_fut, sleep_fut).await {
        Either::Left((value, _)) => Ok(value),
        Either::Right(_) => Err(TimeoutError),
    }
}

/// A `Sleeper` that never wakes up. Selecting against it is equivalent to
/// "no timeout enforcement" — the user future always wins. Use this when a
/// caller has no real runtime but still needs to satisfy a `&dyn Sleeper`
/// parameter (e.g., unit tests, or middleware constructed without an
/// explicit sleeper). Production code should always pass a real impl.
pub struct NoopSleeper;

#[async_trait]
impl Sleeper for NoopSleeper {
    async fn sleep(&self, _duration: Duration) {
        std::future::pending::<()>().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_sleeper_never_resolves_so_user_fut_always_wins() {
        let sleeper = NoopSleeper;
        // Ask for an absurdly short timeout — but with NoopSleeper the
        // sleep future never completes, so the user future wins.
        let result = timeout(&sleeper, Duration::from_nanos(1), async { 42 }).await;
        assert_eq!(result, Ok(42));
    }
}
