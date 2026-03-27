// INPUT:  crate::ports::provider::{types, errors, provider_registry}
// OUTPUT: pub mod types, errors, provider_registry; pub Provider, ProviderRegistry
// POS:    Aggregates and re-exports provider port sub-modules for a unified provider API.
pub mod types;
pub mod errors;
pub mod provider_registry;

#[allow(unused_imports)]
pub use errors::*;
#[allow(unused_imports)]
pub use provider_registry::{Provider, ProviderRegistry};
