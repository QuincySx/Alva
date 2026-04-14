// INPUT:  browser_manager, browser_start, browser_stop, browser_navigate, browser_action, browser_snapshot, browser_screenshot, browser_status (all native-only)
// OUTPUT: BrowserManager, 7 BrowserXxxTool types, browser_tools() preset
// POS:    Crate root — Chrome CDP automation tools, native-only by construction. Empty on wasm32.

//! `alva-agent-browser-tools` — browser automation tools (Chrome via CDP).
//!
//! Extracted from the former `alva-agent-tools` crate so the core
//! tools crate stays wasm32-clean. Browser tools depend on `chromiumoxide`, which pulls
//! `tokio/net` → `mio`, which doesn't compile for wasm32.
//!
//! On native targets this crate exports 7 tools (start / stop /
//! navigate / action / snapshot / screenshot / status) plus a shared
//! `BrowserManager` and a convenience `browser_tools()` preset that
//! returns `Vec<Box<dyn Tool>>`.
//!
//! On wasm32 targets the entire crate compiles to an empty library —
//! it can stay in the workspace without breaking wasm builds. Any
//! browser interaction in a wasm context should use `web_sys::*`
//! directly inside the wasm app, not this crate.

#[cfg(not(target_family = "wasm"))]
pub mod browser_manager;
#[cfg(not(target_family = "wasm"))]
pub mod browser_start;
#[cfg(not(target_family = "wasm"))]
pub mod browser_stop;
#[cfg(not(target_family = "wasm"))]
pub mod browser_navigate;
#[cfg(not(target_family = "wasm"))]
pub mod browser_action;
#[cfg(not(target_family = "wasm"))]
pub mod browser_snapshot;
#[cfg(not(target_family = "wasm"))]
pub mod browser_screenshot;
#[cfg(not(target_family = "wasm"))]
pub mod browser_status;

#[cfg(not(target_family = "wasm"))]
pub use browser_action::BrowserActionTool;
#[cfg(not(target_family = "wasm"))]
pub use browser_manager::BrowserManager;
#[cfg(not(target_family = "wasm"))]
pub use browser_navigate::BrowserNavigateTool;
#[cfg(not(target_family = "wasm"))]
pub use browser_screenshot::BrowserScreenshotTool;
#[cfg(not(target_family = "wasm"))]
pub use browser_snapshot::BrowserSnapshotTool;
#[cfg(not(target_family = "wasm"))]
pub use browser_start::BrowserStartTool;
#[cfg(not(target_family = "wasm"))]
pub use browser_status::BrowserStatusTool;
#[cfg(not(target_family = "wasm"))]
pub use browser_stop::BrowserStopTool;

/// Pre-built browser tool set — returns `Vec<Box<dyn Tool>>` so it can
/// be passed straight to a `ToolRegistry`. Native: 7 tools. wasm32: empty.
#[cfg(not(target_family = "wasm"))]
pub fn browser_tools() -> Vec<Box<dyn alva_kernel_abi::tool::Tool>> {
    let manager = browser_manager::shared_browser_manager();
    vec![
        Box::new(BrowserStartTool { manager: manager.clone() }),
        Box::new(BrowserStopTool { manager: manager.clone() }),
        Box::new(BrowserNavigateTool { manager: manager.clone() }),
        Box::new(BrowserActionTool { manager: manager.clone() }),
        Box::new(BrowserSnapshotTool { manager: manager.clone() }),
        Box::new(BrowserScreenshotTool { manager: manager.clone() }),
        Box::new(BrowserStatusTool { manager }),
    ]
}

#[cfg(target_family = "wasm")]
pub fn browser_tools() -> Vec<Box<dyn alva_kernel_abi::tool::Tool>> {
    Vec::new()
}
