// INPUT:  adapter, factory, context_retrieval, mcp_journal, mapping, session_repository modules
// OUTPUT: AlvaAdapter, root-bound SessionPluginFactory/Plugin/Middleware contracts, ContextRetrievalPlugin, catalog/MCP DTOs and repository Inspector
// POS:    Canonical app-layer Alva engine adapter that builds complete BaseAgent sessions for Hub.

mod adapter;
mod context_retrieval;
mod factory;
mod mapping;
mod mcp_journal;
#[cfg(all(test, unix))]
mod mcp_spawn_tests;
mod session_repository;

pub use adapter::AlvaAdapter;
pub use context_retrieval::{visible_context_tool_text, ContextRetrievalPlugin};
pub use alva_app_core::extension::{LateContext, Plugin, Registrar};
pub use alva_kernel_core::{
    middleware::{LlmCallFn, Middleware, MiddlewareError, MiddlewarePriority},
    AgentState,
};
pub use adapter::{AGENT_STOPPED_PRODUCING_EVENTS, SESSION_REMOVED_WHILE_STARTING};
pub use alva_app_core::session_projection::{build_run_record, RunRecord};
pub use alva_app_core::components::{ComponentToggles, COMPONENTS};
pub use alva_app_core::extension::skills::skill_domain::mcp::{McpServerConfig, McpTransportConfig};
pub use factory::{
    BaseAgentFactory, BuiltAgent, CatalogBaseAgentFactory, DefaultBaseAgentFactory,
    DefaultBaseAgentFactoryConfig, SessionPluginFactory,
};
pub use session_repository::{RepositoryInspector, SessionRepository, SqliteAgentSession};
