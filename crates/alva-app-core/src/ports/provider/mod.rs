// INPUT:  crate::ports::provider::provider_registry
// OUTPUT: pub mod provider_registry; pub Provider, ProviderRegistry
// POS:    Aggregates and re-exports provider port sub-modules for a unified provider API.
pub mod provider_registry;

#[allow(unused_imports)]
pub use provider_registry::{Provider, ProviderRegistry};
