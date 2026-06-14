//! Extension system — trait + machinery are re-exported from `alva-agent-core`.
//!
//! Only the built-in Extension implementations (skills, mcp, hooks, evaluation,
//! agent_spawn, and the flat wrappers for core/shell/task/team/etc) still live
//! in this crate. The trait + HostAPI + event dispatch live in `alva-agent-core`.

pub use alva_agent_core::extension::{
    Extension, ExtensionContext, ExtensionHost, FinalizeContext, HostAPI, LateContext, Plugin,
    Registrar, RegisteredCommand,
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
mod loop_detection;
mod dangling_tool_call;
mod tool_timeout;
mod compaction;
mod checkpoint;
mod permission;
mod pending;
pub mod provider_registry;
pub mod spawn_comm_registry;
pub mod tool_lock_registry;

// Re-export built-in extension types
pub use skills::SkillsExtension;
pub use mcp::McpExtension;
pub use hooks::HooksExtension;
pub use evaluation::EvaluationExtension;
pub use agent_spawn::SubAgentExtension;
pub use lsp::{
    LspDiagnostic, LspDiagnosticsTool, LspExtension, LspManager, LspServerConfig, LspSeverity,
    StubLspManager,
};

// Thin Extension wrappers now live in `alva-agent-extension-builtin::wrappers`.
pub use alva_agent_extension_builtin::wrappers::{
    BrowserExtension, CoreExtension, InteractionExtension, PlanningExtension, ShellExtension,
    TaskExtension, TeamExtension, UtilityExtension, WebExtension,
};
pub use approval::ApprovalExtension;
pub use blackboard_comm::BlackboardCommExtension;
pub use loop_detection::LoopDetectionExtension;
pub use dangling_tool_call::DanglingToolCallExtension;
pub use tool_timeout::ToolTimeoutExtension;
pub use analytics::{AnalyticsExtension, AnalyticsMiddleware};
pub use compaction::CompactionExtension;
pub use checkpoint::CheckpointExtension;
pub use permission::PermissionExtension;
pub use pending::{PendingExtension, PendingMessage, PendingService, PendingServiceImpl};
pub use provider_registry::ProviderRegistryExtension;
pub use spawn_comm_registry::SpawnCommRegistryExtension;
pub use tool_lock_registry::ToolLockRegistryExtension;
