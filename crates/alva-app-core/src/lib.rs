//! Alva App Core — BaseAgent + Extension system.
//!
//! This crate provides the thin orchestration layer that composes extracted
//! agent crates (types, core, tools, security, memory, runtime) into a
//! ready-to-use `BaseAgent` with a plugin architecture.
//!
//! Modules:
//!   base_agent/  — BaseAgent, BaseAgentBuilder, PermissionMode
//!   extension/   — Extension trait, HostAPI, and all built-in
//!                  extensions (skills, mcp, hooks, agent_spawn, evaluation)
//!   settings/    — Settings + HooksSettings types
//!   paths/       — Workspace/global path resolution
//!   utils/       — Small shared helpers (token formatting, cost estimate)
//!   error/       — EngineError

// ── Facade re-exports from extracted crates ──────────────────────────

pub use alva_kernel_abi;

pub use alva_kernel_core::{AgentState, AgentConfig, AgentEvent, AgentMessage};
pub use alva_kernel_core::{MiddlewareStack, Extensions};
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

pub mod analytics;
pub mod base_agent;
pub mod config;
pub mod extension;
pub mod outcome;
pub mod resource;
pub mod roster;
pub mod session_projection;
pub mod session_registry;
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
pub use crate::extension::{ExtensionHost, HostAPI};

// Managed Agents parity surface — see docs/plans/2026-05-11-managed-agents-parity.md.
pub use outcome::{
    render_outcomes_for_session, InMemoryOutcomeRegistry, Outcome, OutcomeError, OutcomeFilter,
    OutcomeParams, OutcomePatch, OutcomeRegistry, OutcomeScore, OutcomeStatus, Rubric,
};
pub use resource::{
    render_resource_instructions, InMemoryResourceRegistry, RepoCheckout, ResourceAccess,
    ResourceError, ResourceFilter, ResourceKind, ResourceParams, ResourcePatch, ResourceRegistry,
    SessionResource,
};
pub use roster::{
    MultiagentRoster, MultiagentRosterCap, RosterEntry, RosterEntryKind, RosterError,
    ROSTER_MAX_ENTRIES, ROSTER_MIN_ENTRIES,
};
pub use session_registry::{
    primary_thread_for, thread_tree, thread_view, InMemorySessionRegistry, SessionFilter,
    SessionMetadata, SessionMetadataPatch, SessionOrder, SessionPage, SessionRegistry,
    SessionStatus, ThreadStats, ThreadUsage,
};
