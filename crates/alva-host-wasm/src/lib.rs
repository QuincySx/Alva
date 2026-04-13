// INPUT:  (none yet)
// OUTPUT: (none yet — placeholder skeleton)
// POS:    Crate root — wasm32 host assembly for alva-kernel. Empty placeholder; impls land in follow-up commits.

//! `alva-host-wasm` — wasm32 host装配 for the alva agent kernel.
//!
//! This is the wasm-side counterpart of `alva-host-native`. The crate is
//! intentionally empty in this commit — it just establishes the workspace
//! slot, the package name, and the dependency edge to `alva-kernel-abi`.
//! Concrete pieces land in follow-up commits, each individually verifiable:
//!
//! 1. `WasmSleeper` — `Sleeper` impl bridging non-Send `gloo-timers` futures
//!    via `spawn_local` + `oneshot::Receiver` so the outer future is Send
//! 2. Tools cfg-gating helper for wasm32
//! 3. `wasm-bindgen` entry that wraps `run_agent`
//! 4. LLM provider adapter (HTTP via `gloo-net` or `web_sys::fetch`)
//!
//! On native targets this crate compiles successfully but exposes nothing.
//! It only earns its keep when built for `target_family = "wasm"`.
