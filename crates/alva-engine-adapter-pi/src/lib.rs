// INPUT:  adapter, config, mapping, process, protocol
// OUTPUT: PiAdapter, PiAdapterConfig, PiProviderConfig, PiToolPolicy
// POS:    Crate root for the Pi coding-agent EngineRuntime adapter.

mod adapter;
mod config;
mod mapping;
mod process;
mod protocol;

pub use adapter::PiAdapter;
pub use config::{PiAdapterConfig, PiApiProtocol, PiProviderConfig, PiToolPolicy};
