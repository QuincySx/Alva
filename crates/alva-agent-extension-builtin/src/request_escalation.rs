// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json, optional crate::LocalToolFs
// OUTPUT: EscalationRequest, EscalationExecutor, RequestEscalationTool, NativeEscalationExecutor (native)
// POS:    Permission-gated request for replaceable host-side command execution outside a worker sandbox.

use std::sync::Arc;

use alva_kernel_abi::{
    AgentError, ExecutionMode, ProgressEvent, Tool, ToolContent, ToolExecutionContext,
    ToolFsExecResult, ToolOutput, ToolPermissionResult,
};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 600_000;

fn default_timeout_ms() -> u64 {
    DEFAULT_TIMEOUT_MS
}

/// A worker's declaration of command work that must be executed by its host.
///
/// `cwd` is deliberately required: crossing an isolation boundary without an
/// explicit working-directory declaration makes the request ambiguous and is
/// especially error-prone once guest paths need translating to host paths.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct EscalationRequest {
    /// Shell command the host should execute.
    pub command: String,
    /// Declared working directory. Relative paths are rooted at the agent workspace.
    pub cwd: String,
    /// Execution timeout in milliseconds (default 120000, maximum 600000).
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

/// Replaceable execution half of [`RequestEscalationTool`].
///
/// Native agents use [`NativeEscalationExecutor`]. Ticket 08 can supply an
/// implementation backed by a guest-to-host import without changing the tool,
/// its schema, or its permission classification.
#[async_trait]
pub trait EscalationExecutor: Send + Sync {
    async fn execute(
        &self,
        request: &EscalationRequest,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolFsExecResult, AgentError>;
}

/// Native execution adapter. It delegates to `ToolFs::exec`, so tests and
/// alternate native hosts can replace process execution through the existing
/// execution-context seam.
#[cfg(not(target_family = "wasm"))]
pub struct NativeEscalationExecutor;

#[cfg(not(target_family = "wasm"))]
#[async_trait]
impl EscalationExecutor for NativeEscalationExecutor {
    async fn execute(
        &self,
        request: &EscalationRequest,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolFsExecResult, AgentError> {
        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "request_escalation".into(),
            message: "workspace context required".into(),
        })?;
        let fallback = crate::LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);
        fs.exec(
            &request.command,
            Some(&request.cwd),
            request.timeout_ms.min(MAX_TIMEOUT_MS),
        )
        .await
        .map_err(|error| AgentError::ToolError {
            tool_name: "request_escalation".into(),
            message: error.to_string(),
        })
    }
}

/// Requests that a host execute a command outside the worker sandbox.
///
/// The tool itself owns no process primitive. Its executor is injected so the
/// same tool contract can be backed by local `ToolFs` today and a WASI host
/// import in Ticket 08.
pub struct RequestEscalationTool {
    executor: Arc<dyn EscalationExecutor>,
}

impl RequestEscalationTool {
    pub fn new(executor: Arc<dyn EscalationExecutor>) -> Self {
        Self { executor }
    }
}

#[cfg(not(target_family = "wasm"))]
impl Default for RequestEscalationTool {
    fn default() -> Self {
        Self::new(Arc::new(NativeEscalationExecutor))
    }
}

#[async_trait]
impl Tool for RequestEscalationTool {
    fn name(&self) -> &str {
        "request_escalation"
    }

    fn description(&self) -> &str {
        "Request host-side execution of a shell command outside the worker sandbox. The request \
         enters the normal PermissionMode approval flow. Always declare both command and cwd."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        let mut schema = serde_json::to_value(schemars::schema_for!(EscalationRequest))
            .expect("schemars::schema_for always produces valid JSON");
        alva_kernel_abi::tool::schema::normalize_llm_tool_schema(&mut schema);
        schema
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::SerialGlobal
    }

    fn check_permissions(
        &self,
        input: &serde_json::Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> ToolPermissionResult {
        let command = input
            .get("command")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("<missing command>");
        let cwd = input
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("<missing cwd>");
        ToolPermissionResult::Ask(format!(
            "Allow host-side command execution in '{cwd}': {command}"
        ))
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let mut request: EscalationRequest =
            serde_json::from_value(input).map_err(|error| AgentError::ToolError {
                tool_name: self.name().into(),
                message: format!("invalid input: {error}"),
            })?;
        if request.command.trim().is_empty() {
            return Ok(ToolOutput::error("command must not be empty"));
        }
        if request.cwd.trim().is_empty() {
            return Ok(ToolOutput::error("cwd must not be empty"));
        }
        request.timeout_ms = request.timeout_ms.min(MAX_TIMEOUT_MS);

        ctx.report_progress(ProgressEvent::Status {
            message: format!(
                "Requesting host execution in {}: {}",
                request.cwd, request.command
            ),
        });
        let result = self.executor.execute(&request, ctx).await?;
        for line in result.stdout.lines() {
            ctx.report_progress(ProgressEvent::StdoutLine {
                line: line.to_string(),
            });
        }
        for line in result.stderr.lines() {
            ctx.report_progress(ProgressEvent::StderrLine {
                line: line.to_string(),
            });
        }

        // Do not truncate here: the worker needs the complete failure output
        // to decide what to repair on its next model turn.
        let model_text = format!(
            "stdout:\n{}\nstderr:\n{}\nexit_code: {}",
            result.stdout, result.stderr, result.exit_code
        );
        Ok(ToolOutput {
            content: vec![ToolContent::text(model_text)],
            is_error: result.exit_code != 0,
            details: Some(json!({
                "stdout": result.stdout,
                "stderr": result.stderr,
                "exit_code": result.exit_code,
                "command": request.command,
                "cwd": request.cwd,
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::Path;
    use std::sync::Mutex;

    use alva_kernel_abi::CancellationToken;

    use super::*;

    struct TestContext {
        cancel: CancellationToken,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }

        fn session_id(&self) -> &str {
            "request-escalation-test"
        }

        fn workspace(&self) -> Option<&Path> {
            Some(Path::new("/workspace"))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// `request_escalation` is not on SecurityGuard's dangerous-tool name
    /// list — this declaration is the *entire* reason a host command cannot run
    /// unreviewed. The permission-mode goldens cover it through a full agent
    /// turn, which means a regression here surfaces as several distant tests
    /// going green for the wrong reason; pin the contract at its source.
    #[test]
    fn escalation_always_declares_itself_ask_never_allow() {
        let tool = RequestEscalationTool::new(Arc::new(NativeEscalationExecutor));
        let ctx = TestContext {
            cancel: CancellationToken::new(),
        };
        let decision = tool.check_permissions(
            &serde_json::json!({"command": "cargo test", "cwd": "/tmp"}),
            &ctx,
        );
        match decision {
            ToolPermissionResult::Ask(question) => {
                assert!(question.contains("cargo test"), "{question}");
                assert!(question.contains("/tmp"), "{question}");
            }
            other => panic!("escalation must always ask, got {other:?}"),
        }
    }

    struct RecordingExecutor {
        requests: Mutex<Vec<EscalationRequest>>,
        result: ToolFsExecResult,
    }

    #[async_trait]
    impl EscalationExecutor for RecordingExecutor {
        async fn execute(
            &self,
            request: &EscalationRequest,
            _ctx: &dyn ToolExecutionContext,
        ) -> Result<ToolFsExecResult, AgentError> {
            self.requests.lock().unwrap().push(request.clone());
            Ok(self.result.clone())
        }
    }

    #[tokio::test]
    async fn returns_complete_stdout_stderr_and_exit_code_without_a_process() {
        let executor = Arc::new(RecordingExecutor {
            requests: Mutex::new(Vec::new()),
            result: ToolFsExecResult {
                stdout: "full stdout\nlast stdout line".into(),
                stderr: "full stderr\nlast stderr line".into(),
                exit_code: 17,
            },
        });
        let tool = RequestEscalationTool::new(executor.clone());
        let ctx = TestContext {
            cancel: CancellationToken::new(),
        };

        let output = tool
            .execute(json!({"command": "cargo test", "cwd": "project"}), &ctx)
            .await
            .expect("executor result");

        assert!(output.is_error);
        assert_eq!(
            output.model_text(),
            "stdout:\nfull stdout\nlast stdout line\nstderr:\nfull stderr\nlast stderr line\nexit_code: 17"
        );
        let requests = executor.requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].command, "cargo test");
        assert_eq!(requests[0].cwd, "project");
        assert_eq!(requests[0].timeout_ms, DEFAULT_TIMEOUT_MS);
    }

    #[test]
    fn permission_contract_requests_the_existing_gate() {
        let tool = RequestEscalationTool::new(Arc::new(RecordingExecutor {
            requests: Mutex::new(Vec::new()),
            result: ToolFsExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
            },
        }));
        let ctx = TestContext {
            cancel: CancellationToken::new(),
        };
        assert!(matches!(
            tool.check_permissions(&json!({"command": "cargo test", "cwd": "."}), &ctx),
            ToolPermissionResult::Ask(_)
        ));
    }
}
