// INPUT:  configured LanguageModel, PermissionMode/SecurityGuard, canonical host grants, allowed domains, escalation executor, alva_sandbox_wasm proxies, optional host job logger, tokio runtime handle
// OUTPUT: run(model, grants, allowed_domains, task) and resolve_worker_wasm() production sidecar discovery
// POS:    CLI-owned wasm-tier host policy: guest mounts, provider streaming, escalation approval/execution, and audit behind spawn_blocking.

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use alva_agent_extension_builtin::request_escalation::{
    EscalationExecutor, EscalationRequest, NativeEscalationExecutor,
};
use alva_app_core::{
    PermissionDecision, PermissionMode, SandboxMode, SecurityDecision, SecurityGuard,
};
use alva_kernel_abi::{
    AgentError, CancellationToken, LanguageModel, Tool, ToolExecutionContext, ToolOutput,
    ToolPermissionResult,
};
use alva_llm_wire::{LlmProxyRequest, LlmProxyResponse, ToolDefinition};
use alva_sandbox_wasm::{
    register_escalation_proxy, register_job_log_proxy, register_llm_proxy, translate_guest_cwd,
    EscalationProxyRequest, EscalationProxyResult, EscalationResponse, Grant, RunLimits,
    RunRequest, SandboxRunner,
};
use async_trait::async_trait;
use futures::StreamExt;

use crate::job_log::JobToolLogger;

const PRIMARY_GUEST_PATH: &str = "/workspace";

struct HostEscalationContext {
    cancel: CancellationToken,
    workspace: PathBuf,
}

impl ToolExecutionContext for HostEscalationContext {
    fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }

    fn session_id(&self) -> &str {
        "wasm-host-escalation"
    }

    fn workspace(&self) -> Option<&Path> {
        Some(&self.workspace)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct HostEscalationHandler {
    runtime_handle: tokio::runtime::Handle,
    grants: Vec<Grant>,
    workspace: PathBuf,
    guard: Mutex<SecurityGuard>,
    executor: Arc<dyn EscalationExecutor>,
    job_logger: Option<Arc<JobToolLogger>>,
}

impl HostEscalationHandler {
    fn new(
        runtime_handle: tokio::runtime::Handle,
        grants: Vec<Grant>,
        permission_mode: PermissionMode,
        job_logger: Option<Arc<JobToolLogger>>,
    ) -> Result<Self, String> {
        let workspace = grants
            .first()
            .ok_or_else(|| "host escalation requires at least one job grant".to_string())?
            .host
            .canonicalize()
            .map_err(|error| format!("resolve primary host grant: {error}"))?;
        let mut guard = SecurityGuard::new(workspace.clone(), SandboxMode::RestrictiveOpen);
        for grant in grants.iter().skip(1) {
            let root = grant
                .host
                .canonicalize()
                .map_err(|error| format!("resolve host grant {}: {error}", grant.host.display()))?;
            guard.add_authorized_root(root);
        }
        guard.set_permission_mode(permission_mode.to_security_mode());
        Ok(Self {
            runtime_handle,
            grants,
            workspace,
            guard: Mutex::new(guard),
            executor: Arc::new(NativeEscalationExecutor),
            job_logger,
        })
    }

    fn execute(&self, request: EscalationProxyRequest) -> EscalationProxyResult {
        let audit_id = self.job_logger.as_ref().map(|logger| {
            let id = logger.next_escalation_id();
            if let Err(error) = logger.record_escalation_request(
                &id,
                &request.command,
                &request.cwd,
                request.timeout_ms,
            ) {
                tracing::warn!(error = %error, "failed to append escalation request job log");
            }
            id
        });
        let result = self.execute_inner(request);
        if let (Some(logger), Some(id)) = (&self.job_logger, audit_id) {
            if let Err(error) = logger.record_escalation_result(&id, &result) {
                tracing::warn!(error = %error, "failed to append escalation result job log");
            }
        }
        result
    }

    fn execute_inner(&self, request: EscalationProxyRequest) -> EscalationProxyResult {
        let host_cwd = match translate_guest_cwd(&self.grants, &request.cwd) {
            Ok(path) => path,
            Err(reason) => {
                return EscalationProxyResult::failure(format!(
                    "host escalation denied: invalid guest cwd: {reason}"
                ))
            }
        };
        let host_cwd_text = host_cwd.to_string_lossy().into_owned();
        let arguments = serde_json::json!({
            "command": request.command,
            "cwd": host_cwd_text,
            "timeout_ms": request.timeout_ms,
        });
        let context = HostEscalationContext {
            cancel: CancellationToken::new(),
            workspace: self.workspace.clone(),
        };
        let decision = {
            let mut guard = self
                .guard
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let decision = guard.check_tool_call_with_permission(
                "request_escalation",
                &arguments,
                &context,
                ToolPermissionResult::Ask(format!(
                    "Allow host-side command execution in {host_cwd_text:?}: {}",
                    request.command
                )),
            );
            if let SecurityDecision::NeedHumanApproval { request_id } = &decision {
                let _pending = guard.take_pending_receiver(request_id);
                let _ = guard.resolve_permission(
                    request_id,
                    "request_escalation",
                    PermissionDecision::RejectOnce,
                );
            }
            decision
        };
        match decision {
            SecurityDecision::Allow => {}
            SecurityDecision::Deny { reason } => {
                return EscalationProxyResult::failure(format!(
                    "host escalation denied by permission policy: {reason}"
                ))
            }
            SecurityDecision::NeedHumanApproval { .. } => {
                return EscalationProxyResult::failure(
                    "host escalation denied: --sandbox wasm runs only in headless -p mode, so Ask approval is rejected once; re-run with --permission-mode accept-shell for classifier-approved commands or bypass",
                )
            }
        }

        let native_request = EscalationRequest {
            command: arguments["command"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            cwd: host_cwd_text,
            timeout_ms: request.timeout_ms,
        };
        match self
            .runtime_handle
            .block_on(self.executor.execute(&native_request, &context))
        {
            Ok(output) => EscalationProxyResult::success(EscalationResponse::new(
                output.stdout,
                output.stderr,
                output.exit_code,
            )),
            Err(error) => {
                EscalationProxyResult::failure(format!("host escalation execution failed: {error}"))
            }
        }
    }
}

struct DefinitionOnlyTool(ToolDefinition);

#[async_trait]
impl Tool for DefinitionOnlyTool {
    fn name(&self) -> &str {
        &self.0.name
    }

    fn description(&self) -> &str {
        &self.0.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.0.parameters.clone()
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        Err(AgentError::ToolError {
            tool_name: self.name().to_string(),
            message: "definition-only host proxy tool cannot execute".to_string(),
        })
    }
}

/// Run one task through the WASIp1 worker without blocking a Tokio async
/// worker thread. The configured provider object and its key remain captured
/// by the host callback; guest args contain only task text and guest paths.
pub(crate) async fn run(
    model: Arc<dyn LanguageModel>,
    host_grants: Vec<PathBuf>,
    allowed_domains: Vec<String>,
    permission_mode: PermissionMode,
    task: String,
) -> Result<String, String> {
    let runtime_handle = tokio::runtime::Handle::current();
    let job_logger = JobToolLogger::from_env();
    tokio::task::spawn_blocking(move || {
        let module = std::fs::read(resolve_worker_wasm()?)
            .map_err(|error| format!("read alva-worker-wasm.wasm: {error}"))?;
        let guest_paths = host_grants
            .iter()
            .enumerate()
            .map(|(index, _)| {
                if index == 0 {
                    PRIMARY_GUEST_PATH.to_string()
                } else {
                    format!("/grants/{index}")
                }
            })
            .collect::<Vec<_>>();
        let grants = host_grants
            .into_iter()
            .zip(&guest_paths)
            .map(|(host, guest)| Grant::read_write(host, guest.clone()))
            .collect::<Vec<_>>();
        let escalation_handler = Arc::new(HostEscalationHandler::new(
            runtime_handle.clone(),
            grants.clone(),
            permission_mode,
            job_logger.clone(),
        )?);
        // argv[0] is the program name per WASI convention; the guest skips it.
        let mut args = vec![
            "alva-worker-wasm".to_string(),
            "--workspace".to_string(),
            PRIMARY_GUEST_PATH.to_string(),
            "--task".to_string(),
            task,
            "--result".to_string(),
            "-".to_string(),
        ];
        for guest in &guest_paths {
            args.push("--grant".to_string());
            args.push(guest.clone());
        }

        let outcome = SandboxRunner::new()
            .run_with_imports(
                RunRequest {
                    module,
                    grants,
                    args,
                    allowed_domains,
                    limits: RunLimits::default(),
                },
                move |linker| {
                    let job_logger = job_logger.clone();
                    register_job_log_proxy(linker, move |event| {
                        if let Some(logger) = &job_logger {
                            if let Err(error) = logger.record_event(event) {
                                tracing::warn!(error = %error, "failed to append wasm job tool log");
                            }
                        }
                        Ok(())
                    })?;
                    let escalation_handler = Arc::clone(&escalation_handler);
                    register_escalation_proxy(linker, move |request| {
                        Ok(escalation_handler.execute(request))
                    })?;
                    register_llm_proxy(linker, move |request: LlmProxyRequest| {
                        let tools: Vec<Box<dyn Tool>> = request
                            .tools
                            .into_iter()
                            .map(|definition| {
                                Box::new(DefinitionOnlyTool(definition)) as Box<dyn Tool>
                            })
                            .collect();
                        let tool_refs = tools.iter().map(Box::as_ref).collect::<Vec<_>>();
                        let events = runtime_handle.block_on(async {
                            model
                                .stream(&request.messages, &tool_refs, &request.config)
                                .collect()
                                .await
                        });
                        Ok(LlmProxyResponse::new(events))
                    })
                },
            )
            .map_err(|error| format!("wasm sandbox execution failed: {error}"))?;

        if outcome.exit_code != 0 {
            let reason = outcome.stderr.trim();
            return Err(if reason.is_empty() {
                format!("wasm worker exited with code {}", outcome.exit_code)
            } else {
                format!(
                    "wasm worker exited with code {}: {reason}",
                    outcome.exit_code
                )
            });
        }
        Ok(outcome.stdout)
    })
    .await
    .map_err(|error| format!("wasm sandbox blocking task failed: {error}"))?
}

/// Resolve the worker as a production sidecar, with explicit override and a
/// source-tree fallback for `cargo run` development.
pub(crate) fn resolve_worker_wasm() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("ALVA_WORKER_WASM") {
        return validate_worker_path(PathBuf::from(path), "ALVA_WORKER_WASM");
    }

    let mut candidates = Vec::new();
    if let Ok(executable) = std::env::current_exe() {
        if let Some(bin_dir) = executable.parent() {
            candidates.push(bin_dir.join("alva-worker-wasm.wasm"));
            candidates.push(
                bin_dir
                    .join("..")
                    .join("lib")
                    .join("alva")
                    .join("alva-worker-wasm.wasm"),
            );
        }
    }
    let workspace_target = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("wasm32-wasip1");
    let development_profiles = if cfg!(debug_assertions) {
        ["debug", "release"]
    } else {
        ["release", "debug"]
    };
    for profile in development_profiles {
        candidates.push(workspace_target.join(profile).join("alva-worker-wasm.wasm"));
    }

    if let Some(path) = candidates.iter().find(|path| path.is_file()) {
        return path
            .canonicalize()
            .map_err(|error| format!("canonicalize worker wasm {}: {error}", path.display()));
    }

    let searched = candidates
        .iter()
        .map(|path| format!("  {}", path.display()))
        .collect::<Vec<_>>()
        .join("\n");
    Err(format!(
        "alva-worker-wasm.wasm was not found. Build it with \
         `cargo build -p alva-worker-wasm --target wasm32-wasip1`, install the \
         wasm sidecar beside `alva`, or set ALVA_WORKER_WASM. Searched:\n{searched}"
    ))
}

fn validate_worker_path(path: PathBuf, source: &str) -> Result<PathBuf, String> {
    if !path.is_file() {
        return Err(format!(
            "{source} points to {}, which is not a file",
            path.display()
        ));
    }
    path.canonicalize()
        .map_err(|error| format!("canonicalize {} from {source}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_test::fixtures::{make_assistant_message, make_tool_call_message};
    use alva_test::mock_provider::MockLanguageModel;
    use serde_json::json;

    fn tool_result_text(messages: &[alva_kernel_abi::Message], tool_name: &str) -> (String, bool) {
        messages
            .iter()
            .flat_map(|message| &message.content)
            .filter_map(|block| block.as_tool_result())
            .find_map(|(_, content, is_error)| {
                let text = content
                    .iter()
                    .filter_map(|item| item.as_text())
                    .collect::<String>();
                text.contains(tool_name).then_some((text, is_error))
            })
            .or_else(|| {
                messages
                    .iter()
                    .flat_map(|message| &message.content)
                    .filter_map(|block| block.as_tool_result())
                    .next_back()
                    .map(|(_, content, is_error)| {
                        (
                            content
                                .iter()
                                .filter_map(|item| item.as_text())
                                .collect::<String>(),
                            is_error,
                        )
                    })
            })
            .expect("model turn contains a tool result")
    }

    #[test]
    fn explicit_worker_override_must_be_a_file() {
        let temp = tempfile::tempdir().unwrap();
        let error = validate_worker_path(temp.path().to_path_buf(), "test")
            .expect_err("directory is not a worker module");
        assert!(error.contains("not a file"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn production_bridge_runs_read_write_agent_loop_off_runtime_thread() {
        let workspace = tempfile::tempdir().unwrap();
        std::fs::write(workspace.path().join("a.txt"), "hello wasm").unwrap();
        let mock = MockLanguageModel::new()
            .with_response(make_tool_call_message(
                "read_file",
                json!({"path": "a.txt"}),
            ))
            .with_response(make_tool_call_message(
                "create_file",
                json!({"path": "b.txt", "content": "HELLO WASM"}),
            ))
            .with_response(make_assistant_message("done"));
        let recorded = mock.clone();

        let result = run(
            Arc::new(mock),
            vec![workspace.path().canonicalize().unwrap()],
            Vec::new(),
            PermissionMode::Ask,
            "Read a.txt and write its uppercase content to b.txt".into(),
        )
        .await
        .expect("production wasm bridge succeeds");

        assert_eq!(result, "done");
        assert_eq!(
            std::fs::read_to_string(workspace.path().join("b.txt")).unwrap(),
            "HELLO WASM"
        );
        let calls = recorded.calls();
        assert_eq!(calls.len(), 3);
        assert!(calls[1]
            .iter()
            .flat_map(|message| &message.content)
            .filter_map(|block| block.as_tool_result())
            .flat_map(|(_, content, _)| content)
            .filter_map(|content| content.as_text())
            .any(|text| text.contains("hello wasm")));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn accept_shell_wasm_golden_repairs_after_host_test_failure() {
        let workspace = tempfile::tempdir().unwrap();
        let cargo_toml = r#"[package]
name = "wasm-escalation-golden"
version = "0.1.0"
edition = "2021"
"#;
        let failing = r#"pub fn answer() -> u32 { 1 }
#[cfg(test)]
mod tests {
    #[test]
    fn answer_is_42() { assert_eq!(super::answer(), 42); }
}
"#;
        let fixed = failing.replace("{ 1 }", "{ 42 }");
        let mock = MockLanguageModel::new()
            .with_response(make_tool_call_message(
                "create_file",
                json!({"path": "Cargo.toml", "content": cargo_toml}),
            ))
            .with_response(make_tool_call_message(
                "create_file",
                json!({"path": "src/lib.rs", "content": failing}),
            ))
            .with_response(make_tool_call_message(
                "request_escalation",
                json!({"command": "cargo test --offline", "cwd": "/workspace"}),
            ))
            .with_response(make_tool_call_message(
                "create_file",
                json!({"path": "src/lib.rs", "content": fixed}),
            ))
            .with_response(make_tool_call_message(
                "request_escalation",
                json!({"command": "cargo test --offline", "cwd": "/workspace"}),
            ))
            .with_response(make_assistant_message("tests repaired and passing"));
        let recorded = mock.clone();

        let result = run(
            Arc::new(mock),
            vec![workspace.path().canonicalize().unwrap()],
            Vec::new(),
            PermissionMode::AcceptShell,
            "Create the crate, run its tests, repair failures, and verify again".into(),
        )
        .await
        .expect("AcceptShell escalation loop succeeds");

        assert_eq!(result, "tests repaired and passing");
        assert!(std::fs::read_to_string(workspace.path().join("src/lib.rs"))
            .unwrap()
            .contains("{ 42 }"));
        let calls = recorded.calls();
        assert_eq!(calls.len(), 6);
        let (failed_test, failed) = tool_result_text(&calls[3], "request_escalation");
        assert!(
            failed,
            "first cargo test unexpectedly passed: {failed_test}"
        );
        assert!(failed_test.contains("exit_code: 101"), "{failed_test}");
        let (passing_test, failed) = tool_result_text(&calls[5], "request_escalation");
        assert!(!failed, "second cargo test failed: {passing_test}");
        assert!(passing_test.contains("exit_code: 0"), "{passing_test}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ask_headless_rejection_reaches_worker_and_finishes_gracefully() {
        let workspace = tempfile::tempdir().unwrap();
        let mock = MockLanguageModel::new()
            .with_response(make_tool_call_message(
                "request_escalation",
                json!({"command": "cargo test --offline", "cwd": "/workspace"}),
            ))
            .with_response(make_assistant_message(
                "Could not run tests because headless Ask rejected the escalation.",
            ));
        let recorded = mock.clone();

        let result = run(
            Arc::new(mock),
            vec![workspace.path().canonicalize().unwrap()],
            Vec::new(),
            PermissionMode::Ask,
            "Run tests, or report the exact host denial".into(),
        )
        .await
        .expect("Ask rejection is a recoverable tool result");

        assert!(result.contains("headless Ask rejected"));
        let calls = recorded.calls();
        assert_eq!(calls.len(), 2);
        let (denial, is_error) = tool_result_text(&calls[1], "request_escalation");
        assert!(is_error, "Ask rejection must be an error tool result");
        assert!(denial.contains("headless -p mode"), "{denial}");
        assert!(denial.contains("rejected once"), "{denial}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn accept_shell_blocks_destructive_wasm_escalation() {
        let workspace = tempfile::tempdir().unwrap();
        let protected = workspace.path().join("protected");
        std::fs::create_dir(&protected).unwrap();
        std::fs::write(protected.join("marker.txt"), "keep").unwrap();
        let mock = MockLanguageModel::new()
            .with_response(make_tool_call_message(
                "request_escalation",
                json!({"command": "rm -rf protected", "cwd": "/workspace"}),
            ))
            .with_response(make_assistant_message("destructive escalation was blocked"));
        let recorded = mock.clone();

        let result = run(
            Arc::new(mock),
            vec![workspace.path().canonicalize().unwrap()],
            Vec::new(),
            PermissionMode::AcceptShell,
            "Try the destructive command and report policy".into(),
        )
        .await
        .expect("policy denial remains a recoverable tool result");

        assert_eq!(result, "destructive escalation was blocked");
        assert_eq!(
            std::fs::read_to_string(protected.join("marker.txt")).unwrap(),
            "keep"
        );
        let (denial, is_error) = tool_result_text(&recorded.calls()[1], "request_escalation");
        assert!(is_error);
        assert!(denial.contains("destructive command"), "{denial}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_handler_rejects_ungranted_guest_cwd_even_in_bypass() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let marker = outside.path().join("marker.txt");
        std::fs::write(&marker, "keep").unwrap();
        let handler = HostEscalationHandler::new(
            tokio::runtime::Handle::current(),
            vec![Grant::read_write(
                workspace.path().canonicalize().unwrap(),
                "/workspace",
            )],
            PermissionMode::Bypass,
            None,
        )
        .unwrap();

        let result = handler.execute(EscalationProxyRequest::new(
            "printf escaped > marker.txt",
            outside.path().to_string_lossy(),
            5_000,
        ));

        assert!(result.response.is_none());
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("outside this job's granted guest paths"));
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "keep");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn host_handler_logs_command_and_complete_output() {
        let workspace = tempfile::tempdir().unwrap();
        let log_path = workspace.path().join("tools.jsonl");
        let logger = JobToolLogger::new(log_path.clone());
        let handler = Arc::new(
            HostEscalationHandler::new(
                tokio::runtime::Handle::current(),
                vec![Grant::read_write(
                    workspace.path().canonicalize().unwrap(),
                    "/workspace",
                )],
                PermissionMode::AcceptShell,
                Some(logger),
            )
            .unwrap(),
        );
        let result = tokio::task::spawn_blocking(move || {
            handler.execute(EscalationProxyRequest::new(
                "printf 'escalation-output'",
                "/workspace",
                5_000,
            ))
        })
        .await
        .unwrap();
        assert_eq!(
            result.response.as_ref().unwrap().stdout,
            "escalation-output"
        );

        let entries = std::fs::read_to_string(log_path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["kind"], "escalation_request");
        assert_eq!(entries[1]["kind"], "escalation_result");
        assert!(entries[0]["result_summary"]
            .as_str()
            .unwrap()
            .contains("escalation-output"));
        assert!(entries[1]["result_summary"]
            .as_str()
            .unwrap()
            .contains("escalation-output"));
    }
}
