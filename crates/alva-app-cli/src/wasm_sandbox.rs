// INPUT:  configured LanguageModel, canonical host grants, allowed domains, alva_sandbox_wasm runner/proxy, alva_llm_wire DTOs, optional host job logger, tokio runtime handle
// OUTPUT: run(model, grants, allowed_domains, task) and resolve_worker_wasm() production sidecar discovery
// POS:    CLI-owned wasm-tier host policy: artifact discovery, guest mounts/args, and true provider streaming behind spawn_blocking.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use alva_kernel_abi::{AgentError, LanguageModel, Tool, ToolExecutionContext, ToolOutput};
use alva_llm_wire::{LlmProxyRequest, LlmProxyResponse, ToolDefinition};
use alva_sandbox_wasm::{
    register_job_log_proxy, register_llm_proxy, Grant, RunLimits, RunRequest, SandboxRunner,
};
use async_trait::async_trait;
use futures::StreamExt;

use crate::job_log::JobToolLogger;

const PRIMARY_GUEST_PATH: &str = "/workspace";

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
            .collect();
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
}
