// INPUT:  alva_kernel_abi::context
// OUTPUT: re-exports ContextError, ContextHooks
// POS:    Re-exports the ContextHooks trait and ContextError from alva_kernel_abi::context.
//! ContextHooks trait — re-exported from `alva_kernel_abi::context`.

pub use alva_kernel_abi::context::{ContextError, ContextHooks};
