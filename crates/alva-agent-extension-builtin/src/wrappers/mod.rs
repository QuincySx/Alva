//! Built-in Extension wrappers (formerly in `alva-app-core/src/extension/`).

pub mod core;
pub mod interaction;
pub mod memory;
pub mod planning;
pub mod security;
pub mod shell;
pub mod task;
pub mod team;
pub mod utility;
pub mod web;
#[cfg(feature = "browser")]
pub mod browser;

pub use self::core::CoreExtension;
pub use interaction::InteractionExtension;
pub use memory::MemoryExtension;
pub use planning::PlanningExtension;
pub use security::SecurityExtension;
pub use shell::ShellExtension;
pub use task::TaskExtension;
pub use team::TeamExtension;
pub use utility::UtilityExtension;
pub use web::WebExtension;
#[cfg(feature = "browser")]
pub use browser::BrowserExtension;
