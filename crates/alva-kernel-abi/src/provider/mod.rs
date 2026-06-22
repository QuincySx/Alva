// POS: Sub-module grouping provider abstractions and conformance test helpers.
pub mod credential;
pub mod tests;
mod types;
pub use credential::{CredentialSource, StaticCredential};
pub use types::*;
