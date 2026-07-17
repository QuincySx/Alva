// INPUT:  kernel/agent capability crates, host-native, protocol crates, app-core modules
// OUTPUT: BaseAgent facade plus re-exported kernel, security (including per-run sandbox enforcement), memory, and protocol APIs
// POS:    App-layer crate root and public facade for the preset native harness.
//! Alva App Core — BaseAgent + Plugin system.
//!
//! This crate provides the thin orchestration layer that composes extracted
//! agent crates (types, core, tools, security, memory, runtime) into a
//! ready-to-use `BaseAgent` with a plugin architecture.
//!
//! Modules:
//!   base_agent/  — BaseAgent, BaseAgentBuilder, PermissionMode
//!   extension/   — Plugin trait, Registrar, and all built-in
//!                  plugins (skills, mcp, hooks, agent_spawn, evaluation)
//!   settings/    — Settings + HooksSettings types
//!   paths/       — Workspace/global path resolution
//!   utils/       — Small shared helpers (token formatting, cost estimate)
//!   error/       — EngineError

// ── Facade re-exports from extracted crates ──────────────────────────

pub use alva_kernel_abi;

pub use alva_kernel_core::run_agent;
pub use alva_kernel_core::{AgentConfig, AgentEvent, AgentMessage, AgentState};
pub use alva_kernel_core::{Extensions, MiddlewareStack};

pub use alva_agent_extension_builtin::skill_tool::SkillRegistryError;
pub use alva_agent_extension_builtin::tool_presets;

pub use alva_agent_security::{
    AuthorizedRoots, PermissionDecision, PermissionManager, SandboxConfig, SandboxEnforcement,
    SandboxMode, SecurityDecision, SecurityGuard, SensitivePathFilter,
};

pub use alva_agent_memory::{EmbeddingProvider, NoopEmbeddingProvider};
pub use alva_agent_memory::{
    MemoryChunk, MemoryEntry, MemoryError, MemoryFile, MemoryService, SyncReport,
};

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
pub mod checkpoint;
pub mod components;
pub mod config;
pub mod error;
pub mod extension;
pub mod outcome;
pub mod paths;
pub mod resource;
pub mod roster;
pub mod session_projection;
pub mod session_registry;
pub mod settings;
pub mod utils;

// ── Convenience re-exports ───────────────────────────────────────────

pub use base_agent::{BaseAgent, BaseAgentBuilder, PermissionMode};
pub use error::EngineError;
pub use extension::hooks::{
    HookEvent, HookExecutor, HookInput, HookOutcome, HookOutput, HookResult,
};
pub use paths::AlvaPaths;
pub use settings::{load_settings, settings_file_paths, Settings, SettingsCache};
pub use utils::{estimate_cost_usd, format_token_count};

// Plugin runtime API
pub use crate::extension::PluginHost;

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
