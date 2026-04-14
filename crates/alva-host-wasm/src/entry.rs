// INPUT:  wasm_bindgen::prelude (wasm only)
// OUTPUT: version() — JS-callable function exposing the crate version
// POS:    wasm-bindgen entry module — minimum viable proof that this crate can expose functions to JS via #[wasm_bindgen].

//! wasm-bindgen entry points.
//!
//! This module is only compiled on wasm32 targets. It contains every
//! function that wasm-bindgen exports to JavaScript. Keeping the
//! bindings in one module makes it easy to audit the JS-facing surface.
//!
//! Current surface is intentionally minimal — a single `version()` call
//! to verify the wasm-bindgen integration compiles + links end-to-end.
//! Real agent-running entries (e.g., `run_with_provider(...)`) will land
//! in follow-up commits once a wasm-compatible LLM provider exists.

use wasm_bindgen::prelude::*;

/// Returns the `alva-host-wasm` crate version. Present mainly as a
/// smoke-test target for the JS side — if this function is callable
/// from JS and returns the expected string, the wasm-bindgen glue is
/// healthy and the rest of the API surface (WasmAgent, WasmSleeper)
/// can be trusted to link.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}
