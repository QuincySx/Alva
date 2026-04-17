// INPUT:  (phase 1) serde, serde_json, thiserror
// OUTPUT: (phase 1) AEP protocol types, plugin manifest types
// POS:    Crate root — dynamic loader for subprocess-based extensions.

//! `alva-app-extension-loader` — dynamic loader for third-party
//! extensions written in languages other than Rust.
//!
//! This crate implements the host side of **AEP** (Alva Extension
//! Protocol), a JSON-RPC 2.0 based protocol for running plugins as
//! subprocesses. Plugin authors write Python / JavaScript files
//! against the `alva-sdk` libraries; this crate loads them at runtime
//! and registers each one with `ExtensionHost` as a normal
//! [`Extension`](alva_agent_core::extension::Extension) via a
//! `RemoteExtensionProxy` that forwards events over stdio.
//!
//! Plugin authors never see Rust, never touch `Cargo.toml`, and never
//! learn about the `Extension` trait — they write a single `.py` or
//! `.js` file against their language's SDK and drop it in
//! `~/.alva/extensions/<name>/`.
//!
//! ## Phase status (v0.1 draft)
//!
//! Work is staged so each phase compiles and has a smoke test.
//!
//! - [x] **Phase 1** — protocol types + manifest types
//! - [x] **Phase 2** — subprocess runtime + JSON-RPC dispatcher
//! - [x] **Phase 3** — `SubprocessLoaderExtension` + `RemoteExtensionProxy`
//! - [ ] **Phase 4** — Python SDK (`alva_sdk` package, separate repo)
//! - [ ] **Phase 5** — JS SDK (`@alva/sdk` package, separate repo)
//! - [ ] **Phase 6** — `host/memory.*` integration + first real E2E demo
//!
//! See [`docs/aep.md`](../docs/aep.md) for the complete wire
//! protocol specification.

#[cfg(not(target_family = "wasm"))]
pub mod protocol;

#[cfg(not(target_family = "wasm"))]
pub mod manifest;

#[cfg(not(target_family = "wasm"))]
pub mod subprocess;

#[cfg(not(target_family = "wasm"))]
pub mod dispatcher;

#[cfg(not(target_family = "wasm"))]
pub mod proxy;

#[cfg(not(target_family = "wasm"))]
pub mod loader;

#[cfg(not(target_family = "wasm"))]
pub mod host_api;
