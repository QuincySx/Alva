//! Unified application state — single source of truth for session, model, tokens,
//! tasks, plan mode, and tool/MCP inventory.
//!
//! `AppStateStore` wraps `AppState` in `Arc<RwLock<_>>` and provides
//! a subscription mechanism so the TUI / CLI layers can react to changes.

mod app_state;
mod selectors;

pub use app_state::{AppState, AppStateStore, StateSubscriber};
pub use selectors::{Selectors, estimate_cost_usd, format_token_count};
