// INPUT:  error, runtime (sub-modules)
// OUTPUT: HubRuntime, HubRequest, HubEvent, EngineId, ProductSessionId, HubSessionSnapshot, HubError
// POS:    Product-level multi-engine session supervisor that depends only on the engine contract.

mod error;
mod runtime;

pub use error::HubError;
pub use runtime::{
    EngineId, HubEvent, HubEventStream, HubRequest, HubRuntime, HubSessionSnapshot,
    HubSessionStatus, ProductSessionId,
};
