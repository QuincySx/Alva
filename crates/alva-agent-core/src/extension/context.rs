//! ExtensionContext — what extensions can see after activation.

use std::path::PathBuf;
use std::sync::Arc;
use alva_kernel_abi::{BusHandle, BusWriter, LanguageModel};
use alva_kernel_abi::tool::Tool;

/// Context provided to extensions during configure phase.
pub struct ExtensionContext {
    pub bus: BusHandle,
    pub bus_writer: BusWriter,
    pub workspace: PathBuf,
    pub tool_names: Vec<String>,
}

/// Context for the finalize phase — has everything including model and final tools.
pub struct FinalizeContext {
    pub bus: BusHandle,
    pub bus_writer: BusWriter,
    pub workspace: PathBuf,
    pub model: Arc<dyn LanguageModel>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub max_iterations: u32,
}
