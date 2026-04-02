// INPUT:  error, event, request, runtime (sub-modules)
// OUTPUT: RuntimeError, RuntimeEvent, RuntimeCapabilities, RuntimeUsage, PermissionDecision, RuntimeOptions, RuntimeRequest, EngineRuntime
// POS:    Crate root for alva-engine-runtime — re-exports error types, events, requests, and the EngineRuntime trait
pub mod error;
pub mod event;
pub mod request;
pub mod runtime;

pub use error::RuntimeError;
pub use event::{PermissionDecision, RuntimeCapabilities, RuntimeEvent, RuntimeUsage};
pub use request::{RuntimeOptions, RuntimeRequest};
pub use runtime::EngineRuntime;

#[cfg(test)]
mod tests;
