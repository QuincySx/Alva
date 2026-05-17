//! Built-in Extension wrappers (formerly in `alva-app-core/src/extension/`).

pub mod core;
pub mod interaction;
pub mod memory;
pub mod planning;
pub mod security;
pub mod shell;
#[cfg(not(target_family = "wasm"))]
pub mod system_context;
#[cfg(feature = "task")]
pub mod task;
#[cfg(feature = "team")]
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
#[cfg(not(target_family = "wasm"))]
pub use system_context::SystemContextExtension;
#[cfg(feature = "task")]
pub use task::TaskExtension;
#[cfg(feature = "team")]
pub use team::TeamExtension;
pub use utility::UtilityExtension;
pub use web::WebExtension;
#[cfg(feature = "browser")]
pub use browser::BrowserExtension;
