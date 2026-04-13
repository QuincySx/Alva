// INPUT:  alva_kernel_abi::Sleeper, gloo-timers + wasm-bindgen-futures (wasm only), tokio::sync::oneshot
// OUTPUT: WasmSleeper (wasm only)
// POS:    Crate root — wasm32 host assembly for alva-kernel.

//! `alva-host-wasm` — wasm32 host装配 for the alva agent kernel.
//!
//! The wasm-side counterpart of `alva-host-native`. Currently provides:
//!
//! - [`WasmSleeper`] — `Sleeper` impl backed by `gloo_timers::future::sleep`,
//!   bridged through `spawn_local + oneshot` so the outer future is Send.
//!
//! Planned (follow-up commits):
//! - Tools cfg-gating helper for wasm32
//! - `wasm-bindgen` entry that wraps `run_agent`
//! - LLM provider adapter (HTTP via `gloo-net` or `web_sys::fetch`)
//!
//! On native targets the wasm-only modules are cfg-gated out, so the crate
//! still compiles successfully — it just exposes nothing useful.

#[cfg(target_family = "wasm")]
mod sleeper;

#[cfg(target_family = "wasm")]
pub use sleeper::WasmSleeper;
