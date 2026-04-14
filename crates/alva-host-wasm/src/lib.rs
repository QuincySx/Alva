// INPUT:  alva_kernel_abi::Sleeper, alva_kernel_core::run_agent, gloo-timers + wasm-bindgen-futures (wasm only), tokio::sync::oneshot
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

// wasm-bindgen entry points — only compiled on wasm32. The module itself
// is private; anything exposed to JS lives inside it and is marked
// #[wasm_bindgen].
#[cfg(target_family = "wasm")]
mod entry;

// Consumer-facing facade: a minimal `WasmAgent` struct that bundles
// AgentState + AgentConfig + run_agent into one type. Compiles on every
// target (native + wasm) so apps and tests share the same API.
mod agent;
pub use agent::WasmAgent;

// Stateless stub LanguageModel shared by tests, the smoke probe, and
// wasm-bindgen demos. Compiles on every target.
mod stub;
pub use stub::StubLanguageModel;

// Compile-time probe — type-checks the full kernel API surface against
// wasm32 to catch regressions. The probe function is dead code; its purpose
// is to force `cargo check --target wasm32` to exercise `run_agent` from a
// downstream host's perspective. Compiled on every target so native check
// also benefits from the type-level coverage.
mod smoke;
