// INPUT:  alva_sandbox_abi::{AuditEvent, log ABI version/limit}, serde_json, wasmtime guest memory
// OUTPUT: register_job_log_proxy
// POS:    Versioned, bounded guest-to-host audit-event import with caller-owned persistence policy.

use crate::SandboxStoreData;
use alva_sandbox_abi::{AuditEvent, LOG_PROXY_ABI_VERSION, MAX_LOG_PROXY_REQUEST_BYTES};
use wasmtime::{Caller, Extern, Linker};

pub fn register_job_log_proxy<F>(
    linker: &mut Linker<SandboxStoreData>,
    append: F,
) -> Result<(), wasmtime::Error>
where
    F: Fn(AuditEvent) -> Result<(), String> + Send + Sync + 'static,
{
    linker.func_wrap(
        "alva:host/log",
        "append",
        move |mut caller: Caller<'_, SandboxStoreData>, req_ptr: i32, req_len: i32| {
            let req_start = usize::try_from(req_ptr)
                .map_err(|_| wasmtime::Error::msg("negative log request pointer"))?;
            let req_len = usize::try_from(req_len)
                .map_err(|_| wasmtime::Error::msg("negative log request length"))?;
            if req_len > MAX_LOG_PROXY_REQUEST_BYTES {
                return Err(wasmtime::Error::msg(format!(
                    "log request is {req_len} bytes; limit is {MAX_LOG_PROXY_REQUEST_BYTES} bytes"
                )));
            }
            let req_end = req_start
                .checked_add(req_len)
                .ok_or_else(|| wasmtime::Error::msg("log request range overflow"))?;
            let memory = caller
                .get_export("memory")
                .and_then(Extern::into_memory)
                .ok_or_else(|| wasmtime::Error::msg("guest did not export memory"))?;
            let encoded = memory
                .data(&caller)
                .get(req_start..req_end)
                .ok_or_else(|| wasmtime::Error::msg("log request is outside guest memory"))?;
            let event: AuditEvent = serde_json::from_slice(encoded)
                .map_err(|error| wasmtime::Error::msg(format!("decode log request: {error}")))?;
            if !event.has_supported_version() {
                return Err(wasmtime::Error::msg(format!(
                    "unsupported log request version {}; host supports {}",
                    event.version, LOG_PROXY_ABI_VERSION
                )));
            }
            append(event).map_err(wasmtime::Error::msg)
        },
    )?;
    Ok(())
}
