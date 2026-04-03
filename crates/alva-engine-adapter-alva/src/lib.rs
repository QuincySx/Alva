// INPUT:  config, mapping, adapter (internal modules)
// OUTPUT: pub AlvaAdapterConfig, pub AlvaAdapter
// POS:    Crate root that re-exports the Alva native engine adapter and its configuration.

mod adapter;
mod config;
mod mapping;

pub use adapter::AlvaAdapter;
pub use config::AlvaAdapterConfig;
