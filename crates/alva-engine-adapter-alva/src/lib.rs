// INPUT:  adapter, factory, mapping (internal modules)
// OUTPUT: AlvaAdapter, BaseAgentFactory, repository Inspector, and stable run projection
// POS:    Canonical app-layer Alva engine adapter that builds complete BaseAgent sessions for Hub.

mod adapter;
mod factory;
mod mapping;
mod session_repository;

pub use adapter::AlvaAdapter;
pub use alva_app_core::session_projection::{build_run_record, RunRecord};
pub use factory::{
    BaseAgentFactory, BuiltAgent, CatalogBaseAgentFactory, DefaultBaseAgentFactory,
    DefaultBaseAgentFactoryConfig,
};
pub use session_repository::{RepositoryInspector, SessionRepository, SqliteAgentSession};
