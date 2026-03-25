// INPUT:  (re-exports submodules: types, models, views, chat, error, theme)
// OUTPUT: pub mod types, pub mod models, pub mod views, pub mod chat, pub mod error, pub mod theme
// POS:    Crate root library; re-exports all top-level modules of the alva-app GUI application.
pub mod types;
pub mod models;
pub mod views;
pub mod chat;
pub mod error;
pub mod theme;

/// GPUI Global wrapping the debug ViewRegistry so views can register themselves
/// for runtime inspection via the debug server.
#[cfg(debug_assertions)]
pub struct DebugViewRegistry(pub std::sync::Arc<alva_app_debug::gpui::ViewRegistry>);

#[cfg(debug_assertions)]
impl gpui::Global for DebugViewRegistry {}

/// GPUI Global wrapping the debug ActionRegistry so components can register
/// type-erased action/state closures for the debug HTTP API.
#[cfg(debug_assertions)]
pub struct DebugActionRegistry(pub std::sync::Arc<alva_app_debug::ActionRegistry>);

#[cfg(debug_assertions)]
impl gpui::Global for DebugActionRegistry {}
