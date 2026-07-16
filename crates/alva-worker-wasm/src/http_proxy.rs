// INPUT:  alva_sandbox_abi fetch DTOs/limits, crate::take_host_response, serde_json, host alva:host/http import
// OUTPUT: fetch(FetchRequest) -> Result<FetchResponse, String>
// POS:    WASIp1 guest half of the blocking fetch ptr/len ABI; host owns all network policy and execution.

use alva_sandbox_abi::{
    FetchProxyResult, FetchRequest, FetchResponse, FETCH_PROXY_ABI_VERSION,
    MAX_FETCH_PROXY_REQUEST_BYTES, MAX_FETCH_PROXY_RESPONSE_BYTES, MAX_FETCH_REQUEST_BODY_BYTES,
};

#[link(wasm_import_module = "alva:host/http")]
extern "C" {
    #[link_name = "fetch"]
    fn host_fetch(req_ptr: i32, req_len: i32) -> i64;
}

pub(crate) fn fetch(request: FetchRequest) -> Result<FetchResponse, String> {
    if request.body.len() > MAX_FETCH_REQUEST_BODY_BYTES {
        return Err(format!(
            "fetch request body is {} bytes; limit is {MAX_FETCH_REQUEST_BODY_BYTES} bytes",
            request.body.len()
        ));
    }
    let encoded = serde_json::to_vec(&request)
        .map_err(|error| format!("serialize fetch request: {error}"))?;
    if encoded.len() > MAX_FETCH_PROXY_REQUEST_BYTES {
        return Err(format!(
            "fetch request is {} bytes; limit is {MAX_FETCH_PROXY_REQUEST_BYTES} bytes",
            encoded.len()
        ));
    }
    let req_len = i32::try_from(encoded.len())
        .map_err(|_| "fetch request exceeds ptr/len ABI limit".to_string())?;
    let packed = unsafe { host_fetch(encoded.as_ptr() as i32, req_len) } as u64;
    let encoded = crate::take_host_response(packed, "fetch proxy", MAX_FETCH_PROXY_RESPONSE_BYTES)
        .map_err(|error| error.to_string())?;
    let result: FetchProxyResult = serde_json::from_slice(&encoded)
        .map_err(|error| format!("decode fetch response: {error}"))?;
    if !result.has_supported_version() {
        return Err(format!(
            "unsupported fetch result version {}; guest supports {}",
            result.version, FETCH_PROXY_ABI_VERSION
        ));
    }
    match (result.response, result.error) {
        (Some(response), None) if response.has_supported_version() => Ok(response),
        (Some(response), None) => Err(format!(
            "unsupported fetch response version {}; guest supports {}",
            response.version, FETCH_PROXY_ABI_VERSION
        )),
        (None, Some(error)) => Err(error),
        _ => Err("host returned an invalid fetch result envelope".to_string()),
    }
}
