// INPUT:  agent_types, agent_core, agent_tools, agent_security, agent_memory, agent_runtime, agent/mcp/skills/gateway/base/system/environment/types/domain/ports/adapters/error
// OUTPUT: Facade re-exports (Agent, AgentHooks, AgentEvent, AgentMessage, Tool, Message, SecurityGuard, MemoryService, etc.) + kept module public APIs (skills, mcp, environment, domain, ports)
// POS:    Crate root — thin facade that re-exports from extracted crates and declares remaining kept modules.
//! Srow Core — thin facade over extracted agent crates, plus skill system, MCP, and environment management
//!
//! Extracted crates (re-exported here for backward compat):
//!   agent-types    — shared type definitions (Tool, Message, LanguageModel, etc.)
//!   agent-core     — Agent engine (Agent, AgentEvent, AgentMessage, middleware)
//!   agent-tools    — Built-in tool implementations (shell, fs, browser, etc.)
//!   agent-security — Security subsystem (sandbox, permissions, path filtering)
//!   agent-memory   — Memory/embedding subsystem
//!   agent-runtime  — Batteries-included runtime builder
//!
//! Kept modules:
//!   agent/        — ACP client, session, persistence
//!   mcp/          — MCP protocol layer
//!   skills/       — Skill system (loader, store, injector)
//!   gateway/      — API gateway (placeholder)
//!   base/         — Infrastructure (process manager)
//!   system/       — System capabilities (placeholder)
//!   types/        — Shared type definitions (legacy, prefer agent-types)
//!   domain/       — Domain models (DDD)
//!   ports/        — Port/interface definitions (DDD)
//!   environment/  — Embedded runtime management (Bun, Node, Python, Chromium, etc.)
//!   adapters/     — Adapter implementations (DDD)

// ── Facade re-exports from extracted crates ──────────────────────────

// Re-export agent-types as a module for qualified access (e.g. srow_core::agent_types::Message)
pub use agent_types;

// Re-export agent-core types for UI layer consumption
pub use agent_core::{Agent, AgentEvent, AgentMessage};
pub use agent_core::{AgentHooks, AgentContext};

// Re-export agent-tools (tool registration + browser automation)
pub use agent_tools::{register_all_tools, register_builtin_tools};
pub use agent_tools::browser::BrowserManager;
pub use agent_tools::browser::browser_manager::{SharedBrowserManager, shared_browser_manager};

// Re-export agent-security
pub use agent_security::{
    SecurityGuard, SecurityDecision,
    PermissionManager, PermissionDecision,
    SensitivePathFilter, AuthorizedRoots,
    SandboxConfig, SandboxMode,
};

// Re-export agent-memory
pub use agent_memory::{MemoryService, MemoryEntry, MemoryChunk, MemoryFile, SyncReport, MemoryError};
pub use agent_memory::{EmbeddingProvider, NoopEmbeddingProvider, MemorySqlite};

// Re-export agent-runtime (builder + init)
pub use agent_runtime::{AgentRuntime, AgentRuntimeBuilder};
pub use agent_runtime::model;

// ── Kept modules ─────────────────────────────────────────────────────

pub mod agent;
pub mod mcp;
pub mod skills;
pub mod gateway;
pub mod base;
pub mod system;
pub mod environment;
pub mod types;
pub mod domain;
pub mod ports;
pub mod adapters;
pub mod base_agent;
pub mod error;

// ── Convenience re-exports — BaseAgent ──────────────────────────────

pub use base_agent::{BaseAgent, BaseAgentBuilder};

// ── Convenience re-exports — domain ─────────────────────────────────

pub use domain::agent::{AgentConfig, LLMConfig, LLMProviderKind};
pub use domain::session::{Session, SessionStatus};
pub use domain::tool::{ToolCall, ToolDefinition, ToolResult};
pub use error::EngineError;
pub use ports::tool::{LocalToolContext, SrowToolContext, Tool, ToolContext, ToolRegistry};
pub use ports::storage::SessionStorage;
pub use ports::provider::provider_registry::{Provider, ProviderRegistry};

// ── Convenience re-exports — skills ─────────────────────────────────

pub use skills::skill_domain::skill::{Skill, SkillBody, SkillKind, SkillMeta};
pub use skills::skill_domain::skill_config::{InjectionPolicy, SkillRef};
pub use skills::skill_domain::mcp::{McpServerConfig, McpServerState, McpToolInfo, McpTransportConfig};
pub use skills::skill_domain::agent_template::{AgentTemplate, GlobalSkillConfig, McpSet, SkillSet};
pub use skills::store::SkillStore;
pub use skills::loader::SkillLoader;
pub use skills::injector::SkillInjector;
pub use mcp::runtime::McpManager;
pub use mcp::tool_adapter::build_mcp_tools;
pub use mcp::config::{McpConfig, McpServerEntry, McpTransportEntry};
pub use mcp::tools::McpRuntimeTool;
pub use skills::skill_fs::FsSkillRepository;
pub use error::SkillError;
pub use skills::skill_ports::skill_repository::{SkillInstallSource, SkillRepository};
pub use skills::skill_ports::mcp_transport::McpTransport;
pub use skills::agent_template_service::{AgentTemplateService, AgentTemplateInstance};
pub use skills::tools::{SearchSkillsTool, UseSkillTool};
pub use skills::middleware::{SkillInjectionMiddleware, SkillInjectionConfig};

// ── Convenience re-exports — environment runtime management ─────────

pub use environment::EnvironmentManager;
pub use environment::config::EnvironmentConfig;
pub use environment::manifest::{ResourceManifest, ComponentVersion, ArtifactConfig, ArchiveFormat};
pub use environment::versions::{InstalledVersions, VersionStatus};
pub use environment::resolver::RuntimeResolver;
