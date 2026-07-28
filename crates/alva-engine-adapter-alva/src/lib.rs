// INPUT:  adapter, factory, mapping (internal modules)
// OUTPUT: AlvaAdapter, BaseAgentFactory, DefaultBaseAgentFactory, DefaultBaseAgentFactoryConfig
// POS:    Canonical app-layer Alva engine adapter that builds complete BaseAgent sessions for Hub.

mod adapter;
mod factory;
mod mapping;

pub use adapter::AlvaAdapter;
pub use factory::{BaseAgentFactory, DefaultBaseAgentFactory, DefaultBaseAgentFactoryConfig};
