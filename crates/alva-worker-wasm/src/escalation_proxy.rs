// INPUT:  alva_agent_extension_builtin escalation contract, alva_kernel_abi, alva_sandbox_abi escalation DTOs/limits, crate::take_host_response, host import
// OUTPUT: HostImportEscalationExecutor
// POS:    WASIp1 guest executor that forwards request_escalation to host policy without owning approval or process execution.

use std::path::{Path, PathBuf};

use alva_agent_extension_builtin::request_escalation::{EscalationExecutor, EscalationRequest};
use alva_kernel_abi::{AgentError, ToolExecutionContext, ToolFsExecResult};
use alva_sandbox_abi::{
    EscalationProxyRequest, EscalationProxyResult, ESCALATION_PROXY_ABI_VERSION,
    ESCALATION_REJECTED_EXIT_CODE, MAX_ESCALATION_PROXY_REQUEST_BYTES,
    MAX_ESCALATION_PROXY_RESPONSE_BYTES,
};
use async_trait::async_trait;

#[link(wasm_import_module = "alva:host/escalation")]
extern "C" {
    #[link_name = "execute"]
    fn host_execute(req_ptr: i32, req_len: i32) -> i64;
}

pub(crate) struct HostImportEscalationExecutor;

#[async_trait]
impl EscalationExecutor for HostImportEscalationExecutor {
    async fn execute(
        &self,
        request: &EscalationRequest,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolFsExecResult, AgentError> {
        let cwd = guest_absolute_cwd(ctx.workspace(), &request.cwd)?;
        let request = EscalationProxyRequest::new(&request.command, cwd, request.timeout_ms);
        let encoded = serde_json::to_vec(&request).map_err(|error| AgentError::ToolError {
            tool_name: "request_escalation".into(),
            message: format!("serialize host escalation request: {error}"),
        })?;
        if encoded.len() > MAX_ESCALATION_PROXY_REQUEST_BYTES {
            return Err(AgentError::ToolError {
                tool_name: "request_escalation".into(),
                message: format!(
                    "host escalation request is {} bytes; limit is {MAX_ESCALATION_PROXY_REQUEST_BYTES} bytes",
                    encoded.len()
                ),
            });
        }
        let req_len = i32::try_from(encoded.len()).map_err(|_| AgentError::ToolError {
            tool_name: "request_escalation".into(),
            message: "host escalation request exceeds ptr/len ABI limit".into(),
        })?;
        let packed = unsafe { host_execute(encoded.as_ptr() as i32, req_len) } as u64;
        let encoded = crate::take_host_response(
            packed,
            "escalation proxy",
            MAX_ESCALATION_PROXY_RESPONSE_BYTES,
        )?;
        let result: EscalationProxyResult =
            serde_json::from_slice(&encoded).map_err(|error| AgentError::ToolError {
                tool_name: "request_escalation".into(),
                message: format!("decode host escalation result: {error}"),
            })?;
        if !result.has_supported_version() {
            return Err(AgentError::ToolError {
                tool_name: "request_escalation".into(),
                message: format!(
                    "unsupported escalation result version {}; guest supports {}",
                    result.version, ESCALATION_PROXY_ABI_VERSION
                ),
            });
        }
        match (result.response, result.error) {
            (Some(response), None) if response.has_supported_version() => Ok(ToolFsExecResult {
                stdout: response.stdout,
                stderr: response.stderr,
                exit_code: response.exit_code,
            }),
            (Some(response), None) => Err(AgentError::ToolError {
                tool_name: "request_escalation".into(),
                message: format!(
                    "unsupported escalation response version {}; guest supports {}",
                    response.version, ESCALATION_PROXY_ABI_VERSION
                ),
            }),
            (None, Some(error)) => Ok(ToolFsExecResult {
                stdout: String::new(),
                stderr: error,
                exit_code: ESCALATION_REJECTED_EXIT_CODE,
            }),
            _ => Err(AgentError::ToolError {
                tool_name: "request_escalation".into(),
                message: "host returned an invalid escalation result envelope".into(),
            }),
        }
    }
}

fn guest_absolute_cwd(workspace: Option<&Path>, cwd: &str) -> Result<String, AgentError> {
    let path = Path::new(cwd);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let workspace = workspace.ok_or_else(|| AgentError::ToolError {
            tool_name: "request_escalation".into(),
            message: "guest workspace context required to resolve relative cwd".into(),
        })?;
        workspace.join(path)
    };
    path_to_guest_string(absolute)
}

fn path_to_guest_string(path: PathBuf) -> Result<String, AgentError> {
    path.into_os_string()
        .into_string()
        .map_err(|_| AgentError::ToolError {
            tool_name: "request_escalation".into(),
            message: "guest cwd is not valid UTF-8".into(),
        })
}
