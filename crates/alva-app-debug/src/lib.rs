// INPUT:  builder, inspect, log_layer, log_store, router, server, traced, action_registry, gpui (sub-modules)
// OUTPUT: ActionRegistry, RegisteredView, DebugServer, DebugServerBuilder, DebugServerHandle, InspectNode, Inspectable, LogCaptureLayer, LogHandle, LogQuery, LogRecord
// POS:    Crate root for alva-app-debug — exposes debug server, log capture, and UI inspection infrastructure
mod builder;
mod inspect;
mod log_layer;
mod log_store;
mod router;
mod server;
mod traced;

pub mod action_registry;
pub mod gpui;

pub use action_registry::{ActionRegistry, RegisteredView};
pub use builder::{DebugServer, DebugServerBuilder, DebugServerHandle};
pub use inspect::{Bounds, InspectNode, Inspectable};
#[cfg(debug_assertions)]
pub use inspect::DebugInspect;
pub use log_layer::{LogCaptureLayer, LogHandle};
pub use log_store::{LogQuery, LogQueryResponse, LogRecord};
