// INPUT:  std::time::Duration, async_trait, alva_kernel_abi::Sleeper, tokio::sync::oneshot, gloo_timers::future::sleep, wasm_bindgen_futures::spawn_local
// OUTPUT: WasmSleeper
// POS:    wasm32 Sleeper impl — bridges non-Send gloo-timers futures via spawn_local + oneshot so the outer future stays Send.

//! `WasmSleeper` — wasm32 `Sleeper` impl backed by `gloo_timers::future::sleep`.
//!
//! ## Why the bridge
//!
//! `alva_kernel_abi::Sleeper` is `Send + Sync`, and `async_trait` requires
//! the futures returned by its methods to be `Send + 'a`. But on wasm32 the
//! natural sleep primitive — `gloo_timers::future::sleep` — returns a
//! future that holds a `Closure<dyn FnMut()>`, which is **not** `Send`.
//!
//! Naively calling `gloo_timers::future::sleep(duration).await` inside an
//! `impl Sleeper for WasmSleeper` body fails to compile because the outer
//! async fn captures the non-Send timer future across an `.await`.
//!
//! The fix is to never let the non-Send future cross an `.await` in the
//! outer function:
//!
//! 1. Create a `tokio::sync::oneshot` channel — the `Receiver<()>` is `Send`.
//! 2. Hand the gloo-timer + sender into `wasm_bindgen_futures::spawn_local`.
//!    `spawn_local`'s task is single-threaded and has no `Send` requirement,
//!    so it's happy to hold the non-Send timer future.
//! 3. Await the receiver in the outer function. The outer future captures
//!    only the receiver, which is `Send`, satisfying `async_trait`.
//!
//! On wasm32 single-threaded execution this is just two future polls in
//! sequence; there's no real overhead beyond an extra channel hop.

use std::time::Duration;

use alva_kernel_abi::Sleeper;
use async_trait::async_trait;

/// Wasm32 sleeper. Stateless and trivially clonable.
#[derive(Debug, Default, Clone, Copy)]
pub struct WasmSleeper;

#[async_trait]
impl Sleeper for WasmSleeper {
    async fn sleep(&self, duration: Duration) {
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        wasm_bindgen_futures::spawn_local(async move {
            gloo_timers::future::sleep(duration).await;
            let _ = tx.send(());
        });
        let _ = rx.await;
    }
}
