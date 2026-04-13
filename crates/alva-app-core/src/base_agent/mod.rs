// INPUT:  alva_kernel_core, alva_kernel_abi::{Bus, BusHandle, BusWriter, BusPlugin, PluginRegistrar, TokenCounter},
//         alva_agent_tools, alva_agent_security, alva_agent_memory, alva_host_native
// OUTPUT: BaseAgent, BaseAgentBuilder, PermissionMode
// POS:    Pre-wired batteries-included agent — owns Bus lifecycle, registers plugins, exposes bus_writer/bus for post-init capability wiring.

mod permission;
mod agent;
pub mod builder;

pub use permission::PermissionMode;
pub use agent::BaseAgent;
pub use builder::BaseAgentBuilder;
