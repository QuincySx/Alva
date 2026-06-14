//! Extension system — trait + machinery are re-exported from `alva-agent-core`.
//!
//! Only the built-in Extension implementations (skills, mcp, hooks, evaluation,
//! agent_spawn, and the flat wrappers for core/shell/task/team/etc) still live
//! in this crate. The trait + HostAPI live in `alva-agent-core`.

pub use alva_agent_core::extension::{
    ExtensionHost, HostAPI, LateContext, Plugin, Registrar, RegisteredCommand,
};

// Folder-based built-in extensions
pub mod skills;
pub mod mcp;
pub mod hooks;
pub mod evaluation;
pub mod agent_spawn;
pub mod lsp;

// Flat built-in extensions (one plugin per file)
mod analytics;
mod approval;
mod blackboard_comm;
mod permission;
mod pending;
pub mod provider_registry;
pub mod spawn_comm_registry;
pub mod tool_lock_registry;

// Re-export built-in extension types
pub use skills::SkillsPlugin;
pub use mcp::McpPlugin;
pub use hooks::HooksPlugin;
pub use evaluation::EvaluationPlugin;
pub use agent_spawn::SubAgentPlugin;
pub use lsp::{
    LspDiagnostic, LspDiagnosticsTool, LspPlugin, LspManager, LspServerConfig, LspSeverity,
    StubLspManager,
};

// Thin Extension wrappers now live in `alva-agent-extension-builtin::wrappers`.
pub use alva_agent_extension_builtin::wrappers::{
    BrowserPlugin, CorePlugin, InteractionPlugin, PlanningPlugin, ShellPlugin,
    TaskPlugin, TeamPlugin, UtilityPlugin, WebPlugin,
};
pub use approval::ApprovalPlugin;
pub use blackboard_comm::BlackboardCommPlugin;
pub use analytics::{AnalyticsPlugin, AnalyticsMiddleware};
pub use permission::PermissionPlugin;
pub use pending::{PendingPlugin, PendingMessage, PendingService, PendingServiceImpl};
pub use provider_registry::ProviderRegistryPlugin;
pub use spawn_comm_registry::SpawnCommRegistryPlugin;
pub use tool_lock_registry::ToolLockRegistryPlugin;
