// INPUT:  alva_llm_wire proxy DTOs/limits, serde_json, std::io, wasmtime::{Caller, Extern, Linker}, WasiP1Ctx
// OUTPUT: register_llm_proxy
// POS:    Production ptr/len memory bridge for a caller-supplied synchronous LLM completion policy.

use alva_llm_wire::{
    LlmProxyRequest, LlmProxyResponse, LLM_PROXY_ABI_VERSION, MAX_LLM_PROXY_REQUEST_BYTES,
    MAX_LLM_PROXY_RESPONSE_BYTES,
};
use std::io;
use wasmtime::{Caller, Extern, Linker};
use wasmtime_wasi::p1::WasiP1Ctx;

struct BoundedJsonBuffer {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

impl BoundedJsonBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            exceeded: false,
        }
    }
}

impl io::Write for BoundedJsonBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.bytes.len().saturating_add(bytes.len()) > self.limit {
            self.exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "LLM proxy JSON exceeds byte limit",
            ));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Register the versioned blocking `alva:host/llm::llm_complete` import.
///
/// This function owns only the ABI mechanics: guest-memory bounds checks,
/// JSON decoding/encoding, version validation, response allocation and size
/// limits. The callback owns policy (which model/provider to call and which
/// host-only credentials it uses), keeping the generic runner provider-free.
pub fn register_llm_proxy<F>(
    linker: &mut Linker<WasiP1Ctx>,
    complete: F,
) -> Result<(), wasmtime::Error>
where
    F: Fn(LlmProxyRequest) -> Result<LlmProxyResponse, String> + Send + Sync + 'static,
{
    linker.func_wrap(
        "alva:host/llm",
        "llm_complete",
        move |mut caller: Caller<'_, WasiP1Ctx>, req_ptr: i32, req_len: i32| {
            let req_start = usize::try_from(req_ptr)
                .map_err(|_| wasmtime::Error::msg("negative LLM proxy request pointer"))?;
            let req_len = usize::try_from(req_len)
                .map_err(|_| wasmtime::Error::msg("negative LLM proxy request length"))?;
            if req_len > MAX_LLM_PROXY_REQUEST_BYTES {
                return Err(wasmtime::Error::msg(format!(
                    "LLM proxy request is {req_len} bytes; limit is {MAX_LLM_PROXY_REQUEST_BYTES} bytes"
                )));
            }
            let req_end = req_start
                .checked_add(req_len)
                .ok_or_else(|| wasmtime::Error::msg("LLM proxy request range overflow"))?;
            let memory = caller
                .get_export("memory")
                .and_then(Extern::into_memory)
                .ok_or_else(|| wasmtime::Error::msg("guest did not export memory"))?;
            let request = memory
                .data(&caller)
                .get(req_start..req_end)
                .ok_or_else(|| {
                    wasmtime::Error::msg("LLM proxy request range is outside guest memory")
                })?;
            let request: LlmProxyRequest = serde_json::from_slice(request)
                .map_err(|error| wasmtime::Error::msg(format!("decode LLM proxy request: {error}")))?;
            if !request.has_supported_version() {
                return Err(wasmtime::Error::msg(format!(
                    "unsupported LLM proxy request version {}; host supports {}",
                    request.version, LLM_PROXY_ABI_VERSION
                )));
            }

            let response = complete(request).map_err(wasmtime::Error::msg)?;
            if !response.has_supported_version() {
                return Err(wasmtime::Error::msg(format!(
                    "LLM proxy callback returned version {}; host supports {}",
                    response.version, LLM_PROXY_ABI_VERSION
                )));
            }
            let mut encoded = BoundedJsonBuffer::new(MAX_LLM_PROXY_RESPONSE_BYTES);
            if let Err(error) = serde_json::to_writer(&mut encoded, &response) {
                return Err(wasmtime::Error::msg(if encoded.exceeded {
                    format!(
                        "LLM proxy response exceeds the {MAX_LLM_PROXY_RESPONSE_BYTES}-byte limit"
                    )
                } else {
                    format!("encode LLM proxy response: {error}")
                }));
            }
            let response = encoded.bytes;
            let resp_len = i32::try_from(response.len()).map_err(|_| {
                wasmtime::Error::msg("LLM proxy response exceeds ptr/len ABI limit")
            })?;
            let alloc = caller
                .get_export("alloc")
                .and_then(Extern::into_func)
                .ok_or_else(|| wasmtime::Error::msg("guest did not export alloc"))?
                .typed::<i32, i32>(&caller)?;
            let resp_ptr = alloc.call(&mut caller, resp_len)?;
            let resp_start = usize::try_from(resp_ptr)
                .map_err(|_| wasmtime::Error::msg("guest alloc returned a negative pointer"))?;
            memory.write(&mut caller, resp_start, &response)?;

            let packed = (u64::from(resp_ptr as u32) << 32) | u64::from(resp_len as u32);
            Ok(packed as i64)
        },
    )?;
    Ok(())
}
