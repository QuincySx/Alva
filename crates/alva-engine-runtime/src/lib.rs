// INPUT:  error, event, request, runtime, stream_contract (sub-modules)
// OUTPUT: RuntimeError, RuntimeEvent, RuntimeCapabilities, RuntimeUsage, PermissionDecision, RuntimeOptions, RuntimeRequest, EngineRuntime, StreamContract*
// POS:    Crate root for alva-engine-runtime — re-exports error types, events, requests, the EngineRuntime trait, and stream-contract helpers
pub mod descriptor;
pub mod error;
pub mod event;
pub mod inspector;
pub mod request;
pub mod runtime;
pub mod stream_contract;
#[cfg(feature = "test-support")]
pub mod conformance;

#[cfg(feature = "test-support")]
pub use conformance::{
    assert_conforms, assert_conforms_events, collect_stream, soft_warnings_for, Scenario,
};

pub use descriptor::{
    ConfigSurface, EngineDescriptor, InstallMetadata, PermissionPlanDecl, PermissionRelation,
    ProviderProtocol,
};
pub use error::RuntimeError;
pub use event::{PermissionDecision, RuntimeCapabilities, RuntimeEvent, RuntimeUsage};
pub use alva_kernel_abi::ToolOutput;
pub use inspector::{
    encode_jsonl_bundle, ChildSessionLink, EngineInspector, InspectionCapabilities,
    InspectionLevel, InspectionSnapshot, InspectionStream, InspectionUpdate,
    InspectorBundleManifest,
};
pub use request::{RuntimeOptions, RuntimeRequest};
pub use runtime::EngineRuntime;
pub use stream_contract::{
    is_consumer_visible_delta, is_fatal_error, is_message_content_block, is_terminal_runtime_event,
    validate_event_stream, SoftStreamWarning, StreamContract, StreamContractError,
    StreamContractViolation,
};

#[cfg(test)]
mod tests;
