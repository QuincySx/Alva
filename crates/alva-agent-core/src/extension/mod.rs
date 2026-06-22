//! Plugin system — the primary extensibility point for agents.
//!
//! Capabilities are expressed as [`Plugin`]s registered via a [`Registrar`].
//! Built-in Plugin implementations (file-io, shell, task, team, web, etc.)
//! live in `alva-agent-extension-builtin`. App-layer protocol plugins
//! (skills, mcp, hooks, evaluation, agent_spawn) live in `alva-app-core`.

mod host;
mod phase;
mod plugin;
mod registrar;

pub use host::{PluginHost, RegisteredCommand};
pub use phase::{PhaseContribution, PhaseHandler, PhaseOrder};
pub use plugin::Plugin;
pub use registrar::{LateContext, Registrar};
