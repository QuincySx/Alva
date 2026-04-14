// INPUT:  wasm_bindgen::prelude (wasm only), std::sync::Arc, crate::{WasmAgent, StubLanguageModel}
// OUTPUT: version(), run_stub_agent(prompt) — JS-callable
// POS:    wasm-bindgen entry module — every JS-facing function lives here. Only compiled on wasm32.

//! wasm-bindgen entry points.
//!
//! This module is only compiled on wasm32 targets. It contains every
//! function that wasm-bindgen exports to JavaScript. Keeping the
//! bindings in one module makes it easy to audit the JS-facing surface.
//!
//! Current surface:
//! - `version()` — smoke-test target returning the crate version
//! - `run_stub_agent(prompt)` — runs a `WasmAgent` backed by
//!   `StubLanguageModel` and returns the response text. Proves the
//!   entire async + wasm-bindgen + WasmAgent chain works end-to-end.
//!
//! Real LLM provider entries will land once a wasm-compatible HTTP
//! client adapter exists (gloo-net / web_sys::fetch).

use std::sync::Arc;

use wasm_bindgen::prelude::*;

use crate::{StubLanguageModel, WasmAgent};

/// Returns the `alva-host-wasm` crate version. Present mainly as a
/// smoke-test target for the JS side — if this function is callable
/// from JS and returns the expected string, the wasm-bindgen glue is
/// healthy and the rest of the API surface (WasmAgent, WasmSleeper)
/// can be trusted to link.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Runs a minimal `WasmAgent` backed by `StubLanguageModel` against
/// the given prompt and returns the response text. Every component of
/// the wasm host stack gets exercised:
///
///   JS async call
///     → wasm-bindgen-futures Promise bridge
///     → WasmAgent::run_simple
///     → run_agent inner loop (kernel-core)
///     → ToolTimeoutMiddleware with WasmSleeper
///     → StubLanguageModel.stream
///     → apply_injections / assemble passthrough
///     → collected MessageEnd event → returned String
///
/// This is a stub — the StubLanguageModel always replies
/// `"stub-response"` regardless of input. Real wasm apps should
/// provide their own `LanguageModel` impl built on `gloo-net::http`
/// or `web_sys::fetch`, then construct a `WasmAgent` directly instead
/// of calling this entry.
#[wasm_bindgen]
pub async fn run_stub_agent(prompt: String) -> Result<String, JsValue> {
    let mut agent = WasmAgent::new(
        Arc::new(StubLanguageModel::default()),
        Vec::new(),
        "",
    );
    agent
        .run_simple(prompt)
        .await
        .map_err(|e| JsValue::from_str(&e.to_string()))
}
