//! Srow Engine — Agent core engine
//!
//! Core loop: prompt -> LLM -> tool_call -> execute -> loop
//! Based on rig-core for LLM provider integration.

pub mod domain;
pub mod ports;
pub mod application;
pub mod adapters;
pub mod error;

// Convenience re-exports
pub use application::engine::{AgentEngine, EngineEvent};
pub use application::session_service::SessionService;
pub use domain::agent::{AgentConfig, LLMConfig, LLMProviderKind};
pub use domain::message::{LLMContent, LLMMessage, Role};
pub use domain::session::{Session, SessionStatus};
pub use domain::tool::{ToolCall, ToolDefinition, ToolResult};
pub use error::EngineError;
pub use ports::llm_provider::LLMProvider;
pub use ports::tool::{Tool, ToolContext, ToolRegistry};
pub use ports::storage::SessionStorage;
