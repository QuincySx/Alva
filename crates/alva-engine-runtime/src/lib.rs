// INPUT:  error, event, request, runtime, stream_contract (sub-modules)
// OUTPUT: RuntimeError, RuntimeEvent, RuntimeCapabilities, RuntimeUsage, PermissionDecision, RuntimeOptions, RuntimeRequest, EngineRuntime, StreamContract*
// POS:    Crate root for alva-engine-runtime — re-exports error types, events, requests, the EngineRuntime trait, and stream-contract helpers
pub mod error;
pub mod event;
pub mod request;
pub mod runtime;
pub mod stream_contract;

pub use error::RuntimeError;
pub use event::{PermissionDecision, RuntimeCapabilities, RuntimeEvent, RuntimeUsage};
pub use request::{RuntimeOptions, RuntimeRequest};
pub use runtime::EngineRuntime;
pub use stream_contract::{
    is_consumer_visible_delta, is_message_content_block, validate_event_stream, StreamContract,
    StreamContractError, StreamContractViolation,
};

#[cfg(test)]
mod tests;
