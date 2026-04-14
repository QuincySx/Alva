//! Alva App Core — BaseAgent + Extension system.
//!
//! This crate provides the thin orchestration layer that composes extracted
//! agent crates (types, core, tools, security, memory, runtime) into a
//! ready-to-use `BaseAgent` with a plugin architecture.
//!
//! Modules:
//!   base_agent/  — BaseAgent, BaseAgentBuilder, PermissionMode
//!   extension/   — Extension trait, HostAPI, event system, and all built-in
//!                  extensions (skills, mcp, hooks, agent_spawn, evaluation)
//!   settings/    — Settings + HooksSettings types
//!   paths/       — Workspace/global path resolution
//!   utils/       — Small shared helpers (token formatting, cost estimate)
//!   error/       — EngineError

// ── Facade re-exports from extracted crates ──────────────────────────

pub use alva_kernel_abi;

pub use alva_kernel_core::{AgentState, AgentConfig, AgentEvent, AgentMessage, PendingMessageQueue};
pub use alva_kernel_core::{Middleware, MiddlewareStack, MiddlewareError, MiddlewarePriority, Extensions};
pub use alva_kernel_core::run_agent;

pub use alva_agent_extension_builtin::tool_presets;

pub use alva_agent_security::{
    SecurityGuard, SecurityDecision,
    PermissionManager, PermissionDecision,
    SensitivePathFilter, AuthorizedRoots,
    SandboxConfig, SandboxMode,
};

pub use alva_agent_memory::{MemoryService, MemoryEntry, MemoryChunk, MemoryFile, SyncReport, MemoryError};
pub use alva_agent_memory::{EmbeddingProvider, NoopEmbeddingProvider};

pub use alva_host_native::{AgentRuntime, AgentRuntimeBuilder};
pub use alva_host_native::model;

pub use alva_protocol_acp;
pub use alva_protocol_mcp;
pub use alva_protocol_skill;

pub use alva_environment;
pub use alva_environment::environment;

pub use alva_agent_context::scope as alva_agent_scope;
pub use alva_agent_context::scope::scope_impl as scope;

// ── Crate modules ────────────────────────────────────────────────────

pub mod base_agent;
pub mod extension;
pub mod settings;
pub mod paths;
pub mod utils;
pub mod error;

// ── Convenience re-exports ───────────────────────────────────────────

pub use base_agent::{BaseAgent, BaseAgentBuilder, PermissionMode};
pub use error::EngineError;
pub use paths::AlvaPaths;
pub use extension::hooks::{HookEvent, HookExecutor, HookInput, HookOutcome, HookOutput, HookResult};
pub use settings::{Settings, SettingsCache, load_settings, settings_file_paths};
pub use utils::{estimate_cost_usd, format_token_count};

// Extension runtime API
pub use crate::extension::{ExtensionEvent, EventResult, ExtensionHost, HostAPI};
