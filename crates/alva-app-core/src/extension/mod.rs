//! Extension system — the primary extensibility point for agents.
//!
//! Contains the `Extension` trait + `HostAPI` infrastructure **and** every
//! built-in extension (one file or folder per plugin):
//!
//! Folder-based plugins (Extension impl lives inside the folder):
//! - `skills/`       — SkillsExtension (skill discovery, loading, injection)
//! - `mcp/`          — McpExtension (MCP server integration)
//! - `hooks/`        — HooksExtension (lifecycle hook execution)
//! - `evaluation/`   — EvaluationExtension (SprintContract + grading)
//!
//! Flat plugins (one file each):
//! - `agent_spawn`   — SubAgentExtension + AgentSpawnTool
//! - `core`, `shell`, `interaction`, `task`, `team`, `planning`, `utility`,
//!   `web`, `browser` — tool-preset wrappers
//! - `loop_detection`, `dangling_tool_call`, `tool_timeout`, `compaction`,
//!   `checkpoint`, `plan_mode` — middleware wrappers
//! - `analytics`, `auth`, `lsp` — stub/hook-point extensions

mod context;
mod events;
mod host;
mod bridge;

pub mod skills;
pub mod mcp;
pub mod hooks;
pub mod evaluation;
pub mod agent_spawn;

// Flat built-in extensions (one plugin per file).
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

pub use context::{ExtensionContext, FinalizeContext};
pub use events::{ExtensionEvent, EventResult};
pub use host::{ExtensionHost, HostAPI, RegisteredCommand};
pub use bridge::ExtensionBridgeMiddleware;

// Re-export every built-in Extension type at `alva_app_core::extension::*`,
// preserving the public API.
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

use std::sync::Arc;
use async_trait::async_trait;
use alva_kernel_abi::tool::Tool;

/// A capability package that participates in agent construction and runtime.
///
/// Lifecycle:
///   1. `tools()`     — build phase: provide tools
///   2. `activate()`  — build phase: register middleware, event handlers, commands via HostAPI
///   3. `configure()`  — build phase: setup with bus/workspace context
///   4. `finalize()`  — build phase: add tools that depend on the final tool list
///   5. runtime       — event handlers fire, steer/follow_up/shutdown available
///
/// This is the **only** public extensibility point for BaseAgent users.
#[async_trait]
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str { "" }
    /// Provide tools during build phase.
    async fn tools(&self) -> Vec<Box<dyn Tool>> { vec![] }
    /// Register middleware, event handlers, commands via the HostAPI.
    /// Called after tools are collected but before middleware stack is built.
    fn activate(&self, _api: &HostAPI) {}
    /// Called after all extensions are collected and bus/workspace are ready.
    async fn configure(&self, _ctx: &ExtensionContext) {}
    /// Called after ALL tools/middleware from ALL extensions are collected.
    /// Can return additional tools that depend on the final tool list.
    async fn finalize(&self, _ctx: &FinalizeContext) -> Vec<Arc<dyn Tool>> { vec![] }
}
