//! Browser automation tools — control Chrome via CDP (Chrome DevTools Protocol)
//!
//! Tools:
//!   browser_start      — launch a Chrome instance
//!   browser_stop       — shut down a Chrome instance
//!   browser_navigate   — navigate to a URL
//!   browser_action     — page interaction (click/type/press/scroll)
//!   browser_snapshot   — extract page content (text / HTML / readability)
//!   browser_screenshot — capture page screenshot
//!   browser_status     — query browser state (running?, URL, tabs)

pub mod browser_manager;
pub mod browser_start;
pub mod browser_stop;
pub mod browser_navigate;
pub mod browser_action;
pub mod browser_snapshot;
pub mod browser_screenshot;
pub mod browser_status;

pub use browser_manager::BrowserManager;
pub use browser_start::BrowserStartTool;
pub use browser_stop::BrowserStopTool;
pub use browser_navigate::BrowserNavigateTool;
pub use browser_action::BrowserActionTool;
pub use browser_snapshot::BrowserSnapshotTool;
pub use browser_screenshot::BrowserScreenshotTool;
pub use browser_status::BrowserStatusTool;
