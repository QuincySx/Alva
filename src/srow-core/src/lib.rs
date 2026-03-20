//! Srow Core — unified agent engine, skill system, and MCP integration
//!
//! Module layout aligned with Wukong:
//!   agent/        — Agent core (engine, ACP client, session, memory, persistence)
//!   mcp/          — MCP protocol layer
//!   skills/       — Skill system (loader, store, injector)
//!   gateway/      — API gateway (placeholder)
//!   base/         — Infrastructure (process manager)
//!   system/       — System capabilities (placeholder)
//!   types/        — Shared type definitions
//!   domain/       — Domain models (DDD)
//!   ports/        — Port/interface definitions (DDD)
//!   environment/  — Embedded runtime management (Bun, Node, Python, Chromium, etc.)
//!   adapters/     — Adapter implementations (DDD)

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
pub mod error;

// Convenience re-exports — engine
pub use agent::runtime::engine::engine::{AgentEngine, EngineEvent};
pub use agent::runtime::engine::session_service::SessionService;
pub use domain::agent::{AgentConfig, LLMConfig, LLMProviderKind};
pub use domain::message::{LLMContent, LLMMessage, Role};
pub use domain::session::{Session, SessionStatus};
pub use domain::tool::{ToolCall, ToolDefinition, ToolResult};
pub use error::EngineError;
pub use ports::llm_provider::LLMProvider;
pub use ports::tool::{Tool, ToolContext, ToolRegistry};
pub use ports::storage::SessionStorage;

// Convenience re-exports — skills
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

// Convenience re-exports — browser automation
pub use agent::runtime::tools::browser::BrowserManager;
pub use agent::runtime::tools::browser::browser_manager::{SharedBrowserManager, shared_browser_manager};
pub use agent::runtime::tools::register_all_tools;

// Convenience re-exports — security
pub use agent::runtime::security::guard::{SecurityGuard, SecurityDecision};
pub use agent::runtime::security::permission::{PermissionManager, PermissionDecision};
pub use agent::runtime::security::sensitive_paths::SensitivePathFilter;
pub use agent::runtime::security::authorized_roots::AuthorizedRoots;
pub use agent::runtime::security::sandbox::{SandboxConfig, SandboxMode};

// Convenience re-exports — environment runtime management
pub use environment::EnvironmentManager;
pub use environment::config::EnvironmentConfig;
pub use environment::manifest::{ResourceManifest, ComponentVersion, ArtifactConfig, ArchiveFormat};
pub use environment::versions::{InstalledVersions, VersionStatus};
pub use environment::resolver::RuntimeResolver;

// Convenience re-exports — orchestrator
pub use agent::orchestrator::orchestrator::{Orchestrator, OrchestratorHandle};
pub use agent::orchestrator::instance::{AgentInstance, AgentInstanceStatus};
pub use agent::orchestrator::template::OrchestratorAgentTemplate;
pub use agent::orchestrator::communication::{AgentMessage, MessageBus};
pub use agent::orchestrator::tools::register_orchestration_tools;
