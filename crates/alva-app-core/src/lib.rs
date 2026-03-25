// INPUT:  alva_types, alva_agent_core, alva_agent_tools, alva_agent_security, alva_agent_memory, alva_agent_runtime, agent/mcp/skills/gateway/base/system/environment/types/domain/ports/adapters/error
// OUTPUT: Facade re-exports (Agent, AgentHooks, AgentEvent, AgentMessage, Tool, Message, SecurityGuard, MemoryService, etc.) + kept module public APIs (skills, mcp, environment, domain, ports)
// POS:    Crate root — thin facade that re-exports from extracted crates and declares remaining kept modules.
//! Alva App Core — thin facade over extracted agent crates, plus skill system, MCP, and environment management
//!
//! Extracted crates (re-exported here for backward compat):
//!   alva-types         — shared type definitions (Tool, Message, LanguageModel, etc.)
//!   alva-agent-core    — Agent engine (Agent, AgentEvent, AgentMessage, middleware)
//!   alva-agent-tools   — Built-in tool implementations (shell, fs, browser, etc.)
//!   alva-agent-security — Security subsystem (sandbox, permissions, path filtering)
//!   alva-agent-memory  — Memory/embedding subsystem
//!   alva-agent-runtime — Batteries-included runtime builder
//!
//! Kept modules:
//!   agent/        — ACP client, session, persistence
//!   mcp/          — MCP protocol layer
//!   skills/       — Skill system (loader, store, injector)
//!   gateway/      — API gateway (placeholder)
//!   base/         — Infrastructure (process manager)
//!   system/       — System capabilities (placeholder)
//!   types/        — Shared type definitions (legacy, prefer alva-types)
//!   domain/       — Domain models (DDD)
//!   ports/        — Port/interface definitions (DDD)
//!   environment/  — Embedded runtime management (Bun, Node, Python, Chromium, etc.)
//!   adapters/     — Adapter implementations (DDD)

// ── Facade re-exports from extracted crates ──────────────────────────

// Re-export alva-types as a module for qualified access (e.g. alva_app_core::alva_types::Message)
pub use alva_types;

// Re-export alva-agent-core types for UI layer consumption
pub use alva_agent_core::{Agent, AgentEvent, AgentMessage};
pub use alva_agent_core::{AgentHooks, AgentContext};

// Re-export alva-agent-tools (tool registration + browser automation)
pub use alva_agent_tools::{register_all_tools, register_builtin_tools};
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

// ── Kept modules ─────────────────────────────────────────────────────

pub mod agent;
pub mod mcp;
pub mod skills;
pub mod gateway;
pub(crate) mod base;
pub mod system;
pub mod environment;
pub(crate) mod types;
pub(crate) mod domain;
pub(crate) mod ports;
pub(crate) mod adapters;
pub mod base_agent;
pub mod error;

// ── Convenience re-exports — BaseAgent ──────────────────────────────

pub use base_agent::{BaseAgent, BaseAgentBuilder};
pub use error::EngineError;
