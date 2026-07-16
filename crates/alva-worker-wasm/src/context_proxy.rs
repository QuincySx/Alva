// INPUT:  alva_sandbox_abi context DTO/version/limit, crate::take_host_response, host context import
// OUTPUT: load_wasm_environment_skill() -> Result<String, AgentError>
// POS:    WASIp1 guest half of the bounded host-to-guest environment skill bridge.

use alva_kernel_abi::AgentError;
use alva_sandbox_abi::{
    WasmEnvironmentContext, MAX_WASM_ENV_CONTEXT_RESPONSE_BYTES, WASM_ENV_CONTEXT_ABI_VERSION,
};

#[link(wasm_import_module = "alva:host/context")]
extern "C" {
    fn wasm_environment() -> i64;
}

pub(crate) fn load_wasm_environment_skill() -> Result<String, AgentError> {
    let packed = unsafe { wasm_environment() } as u64;
    let encoded = crate::take_host_response(
        packed,
        "wasm environment context",
        MAX_WASM_ENV_CONTEXT_RESPONSE_BYTES,
    )?;
    let context: WasmEnvironmentContext = serde_json::from_slice(&encoded)
        .map_err(|error| AgentError::Other(format!("decode wasm environment context: {error}")))?;
    if !context.has_supported_version() {
        return Err(AgentError::Other(format!(
            "unsupported wasm environment context version {}; guest supports {}",
            context.version, WASM_ENV_CONTEXT_ABI_VERSION
        )));
    }
    if context.system_prompt.trim().is_empty() {
        return Err(AgentError::Other(
            "host did not provide the required wasm environment skill".into(),
        ));
    }
    Ok(context.system_prompt)
}
