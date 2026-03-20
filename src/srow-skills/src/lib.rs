//! Srow Skills -- Skill system and MCP integration for Srow Agent
//!
//! Implements three-level progressive Skill loading, injection strategies,
//! MCP Server lifecycle management, and Agent template instantiation.

pub mod domain;
pub mod ports;
pub mod application;
pub mod adapters;
pub mod error;

// Convenience re-exports
pub use domain::skill::{Skill, SkillBody, SkillKind, SkillMeta};
pub use domain::skill_config::{InjectionPolicy, SkillRef};
pub use domain::mcp::{McpServerConfig, McpServerState, McpToolInfo, McpTransportConfig};
pub use domain::agent_template::{AgentTemplate, GlobalSkillConfig, McpSet, SkillSet};
pub use application::skill_store::SkillStore;
pub use application::skill_loader::SkillLoader;
pub use application::skill_injector::SkillInjector;
pub use application::mcp_manager::McpManager;
pub use application::agent_template_service::{AgentTemplateService, AgentTemplateInstance};
pub use adapters::skill_fs::FsSkillRepository;
pub use adapters::mcp_tool_adapter::build_mcp_tools;
pub use error::SkillError;
pub use ports::skill_repository::{SkillInstallSource, SkillRepository};
pub use ports::mcp_transport::McpTransport;
