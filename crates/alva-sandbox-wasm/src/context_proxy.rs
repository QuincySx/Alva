// INPUT:  alva_sandbox_abi context DTO/limit, serde_json, wasmtime guest memory
// OUTPUT: register_wasm_environment_context_proxy
// POS:    Bounded host-to-guest bridge for a host-parsed wasm environment skill injection.

use crate::SandboxStoreData;
use alva_sandbox_abi::{WasmEnvironmentContext, MAX_WASM_ENV_CONTEXT_RESPONSE_BYTES};
use wasmtime::{Caller, Extern, Linker};

/// Register `alva:host/context::wasm_environment`, which returns one
/// versioned prompt block without exposing the host skill directory to WASI.
pub fn register_wasm_environment_context_proxy<F>(
    linker: &mut Linker<SandboxStoreData>,
    load: F,
) -> Result<(), wasmtime::Error>
where
    F: Fn() -> Result<WasmEnvironmentContext, String> + Send + Sync + 'static,
{
    linker.func_wrap(
        "alva:host/context",
        "wasm_environment",
        move |mut caller: Caller<'_, SandboxStoreData>| {
            let context = load().map_err(wasmtime::Error::msg)?;
            if !context.has_supported_version() {
                return Err(wasmtime::Error::msg(format!(
                    "unsupported wasm environment context version {}",
                    context.version
                )));
            }
            let encoded = serde_json::to_vec(&context).map_err(|error| {
                wasmtime::Error::msg(format!("encode wasm environment context: {error}"))
            })?;
            if encoded.len() > MAX_WASM_ENV_CONTEXT_RESPONSE_BYTES {
                return Err(wasmtime::Error::msg(format!(
                    "wasm environment context is {} bytes; limit is {MAX_WASM_ENV_CONTEXT_RESPONSE_BYTES} bytes",
                    encoded.len()
                )));
            }
            let response_len = i32::try_from(encoded.len()).map_err(|_| {
                wasmtime::Error::msg("wasm environment context exceeds ptr/len ABI limit")
            })?;
            let memory = caller
                .get_export("memory")
                .and_then(Extern::into_memory)
                .ok_or_else(|| wasmtime::Error::msg("guest did not export memory"))?;
            let alloc = caller
                .get_export("alloc")
                .and_then(Extern::into_func)
                .ok_or_else(|| wasmtime::Error::msg("guest did not export alloc"))?
                .typed::<i32, i32>(&caller)?;
            let response_ptr = alloc.call(&mut caller, response_len)?;
            let response_start = usize::try_from(response_ptr).map_err(|_| {
                wasmtime::Error::msg("guest alloc returned a negative pointer")
            })?;
            memory.write(&mut caller, response_start, &encoded)?;

            let packed =
                (u64::from(response_ptr as u32) << 32) | u64::from(response_len as u32);
            Ok(packed as i64)
        },
    )?;
    Ok(())
}
