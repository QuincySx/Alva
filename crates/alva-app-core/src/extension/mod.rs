//! Extension system — trait + machinery are re-exported from `alva-agent-core`.
//!
//! Only the built-in Extension implementations (skills, mcp, hooks, evaluation,
//! agent_spawn, and the flat wrappers for core/shell/task/team/etc) still live
//! in this crate. The trait + HostAPI + event dispatch live in `alva-agent-core`.

pub use alva_agent_core::extension::{
    Extension, ExtensionBridgeMiddleware, ExtensionContext, ExtensionEvent, ExtensionHost,
    EventResult, FinalizeContext, HostAPI, RegisteredCommand,
};

// Folder-based built-in extensions
pub mod skills;
pub mod mcp;
pub mod hooks;
pub mod evaluation;
pub mod agent_spawn;

// Flat built-in extensions (one plugin per file)
mod core;
mod shell;
mod interaction;
mod task;
mod team;
mod planning;
mod utility;
mod web;
mod browser;
mod loop_detection;
mod dangling_tool_call;
mod tool_timeout;
mod compaction;
mod checkpoint;
mod plan_mode;
mod analytics;
mod auth;
mod lsp;

// Re-export built-in extension types
pub use skills::SkillsExtension;
pub use mcp::McpExtension;
pub use hooks::HooksExtension;
pub use evaluation::EvaluationExtension;
pub use agent_spawn::{ChildRunRecording, SubAgentExtension};

pub use core::CoreExtension;
pub use shell::ShellExtension;
pub use interaction::InteractionExtension;
pub use task::TaskExtension;
pub use team::TeamExtension;
pub use planning::PlanningExtension;
pub use utility::UtilityExtension;
pub use web::WebExtension;
pub use browser::BrowserExtension;
pub use loop_detection::LoopDetectionExtension;
pub use dangling_tool_call::DanglingToolCallExtension;
pub use tool_timeout::ToolTimeoutExtension;
pub use compaction::CompactionExtension;
pub use checkpoint::CheckpointExtension;
pub use plan_mode::PlanModeExtension;
pub use analytics::AnalyticsExtension;
pub use auth::AuthExtension;
pub use lsp::LspExtension;
