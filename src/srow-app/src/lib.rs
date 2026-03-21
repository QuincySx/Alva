// INPUT:  (re-exports submodules: types, models, views, chat, error, theme)
// OUTPUT: pub mod types, pub mod models, pub mod views, pub mod chat, pub mod error, pub mod theme
// POS:    Crate root library; re-exports all top-level modules of the srow-app GUI application.
pub mod types;
pub mod models;
pub mod views;
pub mod chat;
pub mod error;
pub mod theme;
