// INPUT:  alva_sandbox_abi::{AuditEvent, log request limit}, kernel middleware tool hooks, serde_json, host log import
// OUTPUT: AuditLogMiddleware, append_tool_call, next_tool_call_id
// POS:    WASIp1 guest audit reporter for completed Tool calls and nested host-policy fetch operations.

use std::sync::atomic::{AtomicU64, Ordering};

use alva_kernel_abi::{ToolCall, ToolContent, ToolOutput};
use alva_kernel_core::{AgentState, Middleware, MiddlewareError, MiddlewarePriority};
use alva_sandbox_abi::{AuditEvent, MAX_LOG_PROXY_REQUEST_BYTES};
use async_trait::async_trait;

const MAX_RESULT_SUMMARY_CHARS: usize = 512;

#[link(wasm_import_module = "alva:host/log")]
extern "C" {
    #[link_name = "append"]
    fn host_append(req_ptr: i32, req_len: i32);
}

pub(crate) fn next_tool_call_id(prefix: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("{prefix}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

pub(crate) fn append_tool_call(
    tool_call_id: impl Into<String>,
    tool_name: impl Into<String>,
    is_error: bool,
    result_summary: impl Into<String>,
) -> Result<(), String> {
    let event = AuditEvent::tool_call(
        tool_call_id,
        tool_name,
        is_error,
        truncate_summary(&result_summary.into()),
    );
    let encoded =
        serde_json::to_vec(&event).map_err(|error| format!("serialize audit event: {error}"))?;
    if encoded.len() > MAX_LOG_PROXY_REQUEST_BYTES {
        return Err(format!(
            "audit event is {} bytes; limit is {MAX_LOG_PROXY_REQUEST_BYTES} bytes",
            encoded.len()
        ));
    }
    let req_len = i32::try_from(encoded.len())
        .map_err(|_| "audit event exceeds ptr/len ABI limit".to_string())?;
    unsafe { host_append(encoded.as_ptr() as i32, req_len) };
    Ok(())
}

fn summarize(content: &[ToolContent]) -> String {
    let raw = content
        .iter()
        .map(ToolContent::to_model_string)
        .collect::<Vec<_>>()
        .join("\n");
    truncate_summary(&raw)
}

fn truncate_summary(raw: &str) -> String {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = collapsed.chars();
    let summary = chars
        .by_ref()
        .take(MAX_RESULT_SUMMARY_CHARS)
        .collect::<String>();
    if chars.next().is_some() {
        format!("{summary}…")
    } else {
        summary
    }
}

pub(crate) struct AuditLogMiddleware;

#[async_trait]
impl Middleware for AuditLogMiddleware {
    async fn after_tool_call(
        &self,
        _state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        let _ = append_tool_call(
            tool_call.id.clone(),
            tool_call.name.clone(),
            result.is_error,
            summarize(&result.content),
        );
        Ok(())
    }

    fn priority(&self) -> i32 {
        MiddlewarePriority::OBSERVATION
    }

    fn name(&self) -> &str {
        "wasm-audit-log"
    }
}
