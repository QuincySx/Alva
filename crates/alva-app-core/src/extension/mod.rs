//! Plugin system — trait + machinery are re-exported from `alva-agent-core`.
//!
//! Only the built-in Plugin implementations (skills, mcp, hooks, evaluation,
//! agent_spawn, and the flat wrappers for core/shell/task/team/etc) still live
//! in this crate. The Plugin trait + Registrar live in `alva-agent-core`.

pub use alva_agent_core::extension::{
    LateContext, Plugin, PluginHost, RegisteredCommand, Registrar,
};

// Folder-based built-in plugins
pub mod agent_spawn;
pub mod agent_templates;
pub mod evaluation;
pub mod hooks;
pub mod lsp;
pub mod mcp;
pub mod skills;

// Flat built-in plugins (one plugin per file)
mod analytics;
mod approval;
mod blackboard_comm;
mod pending;
mod permission;
pub mod provider_registry;
pub mod spawn_comm_registry;
pub mod tool_lock_registry;

// Re-export built-in plugin types
pub use agent_spawn::SubAgentPlugin;
pub use evaluation::EvaluationPlugin;
pub use hooks::HooksPlugin;
pub use lsp::{
    LspDiagnostic, LspDiagnosticsTool, LspManager, LspPlugin, LspServerConfig, LspSeverity,
    StubLspManager,
};
pub use mcp::McpPlugin;
pub use skills::SkillsPlugin;

// Thin Plugin wrappers now live in `alva-agent-extension-builtin::wrappers`.
pub use alva_agent_extension_builtin::wrappers::{
    CorePlugin, InteractionPlugin, PlanningPlugin, ShellPlugin, TaskPlugin, TeamPlugin,
    UtilityPlugin, WebPlugin,
};
// BrowserPlugin comes from the app-layer browser crate itself: the SDK tool
// crate must not depend on alva-app-* under any feature (dependency
// firewall Rule 16/17).
pub use alva_app_extension_browser::BrowserPlugin;
pub use analytics::{AnalyticsMiddleware, AnalyticsPlugin};
pub use approval::ApprovalPlugin;
pub use blackboard_comm::BlackboardCommPlugin;
pub use pending::{PendingMessage, PendingPlugin, PendingService, PendingServiceImpl};
pub use permission::PermissionPlugin;
pub use provider_registry::ProviderRegistryPlugin;
pub use spawn_comm_registry::SpawnCommRegistryPlugin;
pub use tool_lock_registry::ToolLockRegistryPlugin;
