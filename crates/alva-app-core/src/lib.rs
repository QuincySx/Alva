// INPUT:  alva_types, alva_agent_core, alva_agent_tools, alva_agent_security, alva_agent_memory, alva_agent_runtime, agent/mcp/skills/gateway/base/system/environment/types/domain/ports/adapters/error
// OUTPUT: Facade re-exports (AgentState, AgentConfig, AgentEvent, AgentMessage, Tool, Message, SecurityGuard, MemoryService, etc.) + kept module public APIs (skills, mcp, environment, domain, ports)
// POS:    Crate root — thin facade that re-exports from extracted crates and declares remaining kept modules.
//! Alva App Core — thin facade over extracted agent crates, plus skill system, MCP, and environment management
//!
//! Extracted crates (re-exported here for backward compat):
//!   alva-types         — shared type definitions (Tool, Message, LanguageModel, etc.)
//!   alva-agent-core    — V2 Agent engine (AgentState, AgentConfig, run_agent, middleware)
//!   alva-agent-tools   — Built-in tool implementations (shell, fs, browser, etc.)
//!   alva-agent-security — Security subsystem (sandbox, permissions, path filtering)
//!   alva-agent-memory  — Memory/embedding subsystem
//!   alva-agent-runtime — Batteries-included runtime builder
//!
//! Kept modules:
//!   agent/        — ACP client, session, persistence
//!   mcp/          — MCP protocol layer
//!   skills/       — Skill system (loader, store, injector)
//!   base/         — Infrastructure (process manager)
//!   types/        — Shared type definitions (legacy, prefer alva-types)
//!   domain/       — Domain models (DDD)
//!   ports/        — Port/interface definitions (DDD)
//!   environment/  — Embedded runtime management (extracted to alva-sandbox, re-exported here)
//!   adapters/     — Adapter implementations (DDD)

// ── Facade re-exports from extracted crates ──────────────────────────

// Re-export alva-types as a module for qualified access (e.g. alva_app_core::alva_types::Message)
pub use alva_types;

// Re-export alva-agent-core V2 types for UI layer consumption
pub use alva_agent_core::{AgentState, AgentConfig, AgentEvent, AgentMessage, PendingMessageQueue};
pub use alva_agent_core::{Middleware, MiddlewareStack, MiddlewareError, MiddlewarePriority, Extensions};
pub use alva_agent_core::run_agent;

// Re-export alva-agent-tools (tool registration + presets + browser automation)
pub use alva_agent_tools::{register_all_tools, register_builtin_tools, tool_presets};
pub use alva_agent_tools::browser::BrowserManager;
pub use alva_agent_tools::browser::browser_manager::{SharedBrowserManager, shared_browser_manager};

// Re-export alva-agent-security
pub use alva_agent_security::{
    SecurityGuard, SecurityDecision,
    PermissionManager, PermissionDecision,
    SensitivePathFilter, AuthorizedRoots,
    SandboxConfig, SandboxMode,
};

// Re-export alva-agent-memory
pub use alva_agent_memory::{MemoryService, MemoryEntry, MemoryChunk, MemoryFile, SyncReport, MemoryError};
pub use alva_agent_memory::{EmbeddingProvider, NoopEmbeddingProvider, MemorySqlite};

// Re-export alva-agent-runtime (builder + init)
pub use alva_agent_runtime::{AgentRuntime, AgentRuntimeBuilder};
pub use alva_agent_runtime::model;

// Re-export protocol crates as modules for qualified access
pub use alva_protocol_acp;
pub use alva_protocol_mcp;
pub use alva_protocol_skill;

// Re-export alva-sandbox (environment management)
pub use alva_environment;

// ── Kept modules ─────────────────────────────────────────────────────

pub mod extension;
pub mod agent;
pub mod mcp;
pub mod skills;
pub(crate) mod base;
pub use alva_environment::environment;
pub(crate) mod types;
pub(crate) mod domain;
pub(crate) mod ports;
pub(crate) mod adapters;
pub mod auth;
pub mod base_agent;
pub mod error;
pub mod lsp;
pub mod paths;
pub mod plugins;
pub mod analytics;
pub mod hooks;
pub mod settings;
pub mod state;

// Re-export alva-agent-scope (blackboard + scope infrastructure)
pub use alva_agent_scope;
pub use alva_agent_scope::scope_impl as scope;

// ── Convenience re-exports — BaseAgent ──────────────────────────────

pub use base_agent::{BaseAgent, BaseAgentBuilder, PermissionMode};
pub use error::EngineError;
pub use paths::AlvaPaths;
pub use hooks::{HookEvent, HookExecutor, HookInput, HookOutcome, HookOutput, HookResult};
pub use settings::{Settings, SettingsCache, load_settings, settings_file_paths};
pub use state::{AppState, AppStateStore, Selectors};

// Re-export Extension V2 runtime API types
pub use crate::extension::{ExtensionEvent, EventResult, ExtensionHost, HostAPI};
