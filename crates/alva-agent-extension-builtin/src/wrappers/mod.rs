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

pub use self::core::CorePlugin;
pub use interaction::InteractionPlugin;
pub use memory::MemoryPlugin;
pub use planning::PlanningPlugin;
pub use security::SecurityPlugin;
pub use shell::ShellPlugin;
#[cfg(not(target_family = "wasm"))]
pub use system_context::SystemContextPlugin;
#[cfg(feature = "task")]
pub use task::TaskPlugin;
#[cfg(feature = "team")]
pub use team::TeamPlugin;
pub use utility::UtilityPlugin;
pub use web::WebPlugin;
#[cfg(feature = "browser")]
pub use browser::BrowserPlugin;
