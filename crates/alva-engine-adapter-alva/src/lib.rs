// INPUT:  adapter, factory, context_retrieval, mapping, session_repository modules
// OUTPUT: AlvaAdapter, BaseAgentFactory, ContextRetrievalPlugin, catalog/MCP DTOs, repository Inspector and run projection
// POS:    Canonical app-layer Alva engine adapter that builds complete BaseAgent sessions for Hub.

mod adapter;
mod context_retrieval;
mod factory;
mod mapping;
mod session_repository;

pub use adapter::AlvaAdapter;
pub use context_retrieval::ContextRetrievalPlugin;
pub use adapter::{AGENT_STOPPED_PRODUCING_EVENTS, SESSION_REMOVED_WHILE_STARTING};
pub use alva_app_core::session_projection::{build_run_record, RunRecord};
pub use alva_app_core::components::{ComponentToggles, COMPONENTS};
pub use alva_app_core::extension::skills::skill_domain::mcp::{McpServerConfig, McpTransportConfig};
pub use factory::{
    BaseAgentFactory, BuiltAgent, CatalogBaseAgentFactory, DefaultBaseAgentFactory,
    DefaultBaseAgentFactoryConfig,
};
pub use session_repository::{RepositoryInspector, SessionRepository, SqliteAgentSession};
