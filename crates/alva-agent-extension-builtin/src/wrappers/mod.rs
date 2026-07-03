//! Built-in Extension wrappers (formerly in `alva-app-core/src/extension/`).

// NOTE: no browser wrapper here — it lives in `alva-app-extension-browser`
// (app layer). This SDK crate must not depend on alva-app-* under any
// feature; the dependency firewall (Rule 16/17) enforces it.
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
