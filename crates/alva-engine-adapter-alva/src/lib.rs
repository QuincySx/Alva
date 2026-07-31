// INPUT:  adapter, factory, mapping (internal modules)
// OUTPUT: AlvaAdapter, BaseAgentFactory, BuiltAgent, Default* / Catalog* factories
// POS:    Canonical app-layer Alva engine adapter that builds complete BaseAgent sessions for Hub.

mod adapter;
mod factory;
mod history;
mod mapping;

pub use adapter::AlvaAdapter;
pub use factory::{
    BaseAgentFactory, BuiltAgent, CatalogBaseAgentFactory, DefaultBaseAgentFactory,
    DefaultBaseAgentFactoryConfig,
};
pub use history::HistoryStore;
