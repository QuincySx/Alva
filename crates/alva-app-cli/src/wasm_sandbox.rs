// INPUT:  configured LanguageModel, bundled wasm-env injection, PermissionMode/SecurityGuard, canonical host grants, allowed domains, escalation executor, alva_sandbox_wasm proxies, optional host job logger, tokio runtime handle
// OUTPUT: run(model, grants, domains, permission/turn/resource policy, task) and resolve_worker_wasm() sidecar discovery
// POS:    CLI-owned wasm-tier host policy: bounded skill/model/audit/escalation imports, guest mounts, and sidecar execution behind spawn_blocking.

use std::any::Any;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use alva_agent_extension_builtin::request_escalation::{
    EscalationExecutor, EscalationRequest, NativeEscalationExecutor,
};
use alva_app_core::{
    PermissionDecision, PermissionMode, SandboxMode, SecurityDecision, SecurityGuard,
};
use alva_kernel_abi::{
    AgentError, CancellationToken, EffectDescription, LanguageModel, ModelConfig, Tool,
    ToolExecutionContext, ToolOutput,
};
use alva_llm_wire::{
    LlmProxyRequest, LlmProxyResponse, ToolDefinition, MAX_LLM_PROXY_RESPONSE_BYTES,
};
use alva_sandbox_wasm::{
    register_escalation_proxy, register_job_log_proxy, register_llm_proxy,
    register_wasm_environment_context_proxy, translate_guest_cwd, EscalationProxyRequest,
    EscalationProxyResult, EscalationResponse, Grant, RunLimits, RunRequest, SandboxRunner,
    WasmEnvironmentContext, MAX_WASM_MODULE_BYTES,
};
use async_trait::async_trait;
use futures::StreamExt;

use crate::job_log::JobToolLogger;

const PRIMARY_GUEST_PATH: &str = "/workspace";
const LLM_RESPONSE_ENVELOPE_RESERVE: usize = 128;
const MAX_GUEST_REQUESTED_OUTPUT_TOKENS: u32 = 32_768;
#[cfg(test)]
const DEFAULT_TEST_ESCALATION_CALL_BUDGET: u32 = 64;
const MAX_ENVIRONMENT_CONTEXT_CALLS: u32 = 4;

struct BoundedJsonSize {
    bytes: usize,
    limit: usize,
    exceeded: bool,
}

impl io::Write for BoundedJsonSize {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let next = self.bytes.saturating_add(bytes.len());
        if next > self.limit {
            self.exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                "serialized JSON exceeds byte budget",
            ));
        }
        self.bytes = next;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn bounded_json_size<T: serde::Serialize>(value: &T, limit: usize) -> Result<usize, String> {
    let mut counter = BoundedJsonSize {
        bytes: 0,
        limit,
        exceeded: false,
    };
    serde_json::to_writer(&mut counter, value).map_err(|error| {
        if counter.exceeded {
            format!("serialized JSON exceeds the {limit}-byte budget")
        } else {
            format!("serialize JSON: {error}")
        }
    })?;
    Ok(counter.bytes)
}

fn sanitize_guest_model_config(mut config: ModelConfig) -> ModelConfig {
    config.max_tokens = config
        .max_tokens
        .map(|tokens| tokens.min(MAX_GUEST_REQUESTED_OUTPUT_TOKENS));
    // `extra_body` is deliberately last-write-wins in native callers and can
    // replace fields such as model/max_tokens. A guest module is not a trusted
    // provider-policy source; future sanctioned overrides need an explicit
    // host-owned capability rather than this free-form channel.
    config.extra_body = None;
    config
}

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
    permission_mode: PermissionMode,
    allow_unsandboxed_execution: bool,
    deadline: Instant,
    max_calls: u32,
    calls: AtomicU32,
    guard: Mutex<SecurityGuard>,
    executor: Arc<dyn EscalationExecutor>,
    job_logger: Option<Arc<JobToolLogger>>,
}

impl HostEscalationHandler {
    #[cfg(test)]
    fn new(
        runtime_handle: tokio::runtime::Handle,
        grants: Vec<Grant>,
        permission_mode: PermissionMode,
        allow_unsandboxed_execution: bool,
        job_logger: Option<Arc<JobToolLogger>>,
    ) -> Result<Self, String> {
        Self::new_with_deadline(
            runtime_handle,
            grants,
            permission_mode,
            allow_unsandboxed_execution,
            Instant::now() + RunLimits::default().wall_clock,
            DEFAULT_TEST_ESCALATION_CALL_BUDGET,
            job_logger,
        )
    }

    fn new_with_deadline(
        runtime_handle: tokio::runtime::Handle,
        grants: Vec<Grant>,
        permission_mode: PermissionMode,
        allow_unsandboxed_execution: bool,
        deadline: Instant,
        max_calls: u32,
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
            permission_mode,
            allow_unsandboxed_execution,
            deadline,
            max_calls,
            calls: AtomicU32::new(0),
            guard: Mutex::new(guard),
            executor: Arc::new(NativeEscalationExecutor),
            job_logger,
        })
    }

    fn execute(&self, request: EscalationProxyRequest) -> EscalationProxyResult {
        let call_number = self.calls.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        if call_number > self.max_calls {
            return EscalationProxyResult::failure(format!(
                "host escalation call budget exhausted: {} calls allowed",
                self.max_calls
            ));
        }
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
        if self.permission_mode != PermissionMode::Bypass || !self.allow_unsandboxed_execution {
            return EscalationProxyResult::failure(
                "host escalation denied: commands execute outside the WASM sandbox and require \
                 both --permission-mode bypass and --dangerously-allow-unsandboxed",
            );
        }
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return EscalationProxyResult::failure(
                "host escalation denied: the WASM run deadline has expired",
            );
        }
        let host_cwd = match translate_guest_cwd(&self.grants, &request.cwd) {
            Ok(path) => path,
            Err(reason) => {
                return EscalationProxyResult::failure(format!(
                    "host escalation denied: invalid guest cwd: {reason}"
                ))
            }
        };
        let host_cwd_text = host_cwd.to_string_lossy().into_owned();
        let timeout_ms = request.timeout_ms.min(
            u64::try_from(remaining.as_millis())
                .unwrap_or(u64::MAX)
                .max(1),
        );
        let arguments = serde_json::json!({
            "command": request.command,
            "cwd": host_cwd_text,
            "timeout_ms": timeout_ms,
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
            let decision = guard.check_tool_call(
                "request_escalation",
                &arguments,
                &context,
                // Same self-description the escalation tool itself emits;
                // headless wasm escalation is then rejected once by policy
                // below, exactly as the pre-refactor Ask path did.
                &EffectDescription::of(format!(
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
                    "host escalation denied: --sandbox wasm runs only in headless -p mode, so Ask approval is rejected once; only an explicit --permission-mode bypass may execute outside the guest sandbox",
                )
            }
        }

        let native_request = EscalationRequest {
            command: arguments["command"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            cwd: host_cwd_text,
            timeout_ms,
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

    fn parameters_schema(&self, _ctx: &alva_kernel_abi::ToolSchemaContext) -> serde_json::Value {
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
/// Build the per-run resource ceilings passed to the sandbox. `wall_clock_secs`
/// is the CLI-resolved `--wasm-wall-clock-secs`/`ALVA_WASM_WALL_CLOCK_SECS`
/// override (flag beats env var; see `parse_wasm_wall_clock_secs` in
/// `main.rs`); `wasm_fuel` is the analogous `--wasm-fuel`/`ALVA_WASM_FUEL`
/// override (see `parse_wasm_fuel` in `main.rs`). Either `None` keeps the
/// library's corresponding `RunLimits::default()` field untouched;
/// `max_memory_bytes` always stays at the library default — neither CLI flag
/// touches it.
pub(crate) fn build_run_limits(wall_clock_secs: Option<u64>, wasm_fuel: Option<u64>) -> RunLimits {
    RunLimits {
        wall_clock: wall_clock_secs
            .map(Duration::from_secs)
            .unwrap_or(RunLimits::default().wall_clock),
        fuel: wasm_fuel.unwrap_or(RunLimits::default().fuel),
        ..RunLimits::default()
    }
}

#[allow(clippy::too_many_arguments)] // Sandbox policy inputs stay explicit at the host boundary.
pub(crate) async fn run(
    model: Arc<dyn LanguageModel>,
    host_grants: Vec<PathBuf>,
    allowed_domains: Vec<String>,
    permission_mode: PermissionMode,
    allow_unsandboxed_execution: bool,
    max_turns: u32,
    wasm_wall_clock_secs: Option<u64>,
    wasm_fuel: Option<u64>,
    task: String,
) -> Result<String, String> {
    let runtime_handle = tokio::runtime::Handle::current();
    let job_logger = JobToolLogger::from_env();
    let environment_skill = crate::bundled_skills::wasm_environment_skill_injection().await?;
    tokio::task::spawn_blocking(move || {
        let module = read_worker_module(&resolve_worker_wasm()?)?;
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
        let run_limits = build_run_limits(wasm_wall_clock_secs, wasm_fuel);
        let run_deadline = Instant::now()
            .checked_add(run_limits.wall_clock)
            .ok_or_else(|| "WASM wall-clock limit is too large for this platform".to_string())?;
        let grants = host_grants
            .into_iter()
            .zip(&guest_paths)
            .map(|(host, guest)| grant_for_permission_mode(host, guest, permission_mode))
            .collect::<Vec<_>>();
        let escalation_handler = Arc::new(HostEscalationHandler::new_with_deadline(
            runtime_handle.clone(),
            grants.clone(),
            permission_mode,
            allow_unsandboxed_execution,
            run_deadline,
            escalation_call_budget(max_turns),
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
            "--permission-mode".to_string(),
            permission_mode_arg(permission_mode).to_string(),
            "--max-turns".to_string(),
            max_turns.to_string(),
        ];
        for guest in &guest_paths {
            args.push("--grant".to_string());
            args.push(guest.clone());
        }

        let llm_calls = Arc::new(AtomicU32::new(0));
        let audit_error_warned = Arc::new(AtomicBool::new(false));
        let environment_context_calls = Arc::new(AtomicU32::new(0));
        let outcome = SandboxRunner::new()
            .run_with_imports(
                RunRequest {
                    module,
                    grants,
                    args,
                    allowed_domains,
                    limits: run_limits,
                },
                move |linker| {
                    let environment_skill = environment_skill.clone();
                    let environment_context_calls = Arc::clone(&environment_context_calls);
                    register_wasm_environment_context_proxy(linker, move || {
                        let call_number = environment_context_calls
                            .fetch_add(1, Ordering::Relaxed)
                            .saturating_add(1);
                        if call_number > MAX_ENVIRONMENT_CONTEXT_CALLS {
                            return Err(format!(
                                "WASM environment context call budget exhausted: \
                                 {MAX_ENVIRONMENT_CONTEXT_CALLS} calls allowed"
                            ));
                        }
                        Ok(WasmEnvironmentContext::new(environment_skill.clone()))
                    })?;
                    let job_logger = job_logger.clone();
                    let audit_error_warned = Arc::clone(&audit_error_warned);
                    register_job_log_proxy(linker, move |event| {
                        if let Some(logger) = &job_logger {
                            if let Err(error) = logger.record_event(event) {
                                if !audit_error_warned.swap(true, Ordering::Relaxed) {
                                    tracing::warn!(
                                        error = %error,
                                        "rejecting invalid or excessive wasm job audit events"
                                    );
                                }
                            }
                        }
                        Ok(())
                    })?;
                    let escalation_handler = Arc::clone(&escalation_handler);
                    register_escalation_proxy(linker, move |request| {
                        Ok(escalation_handler.execute(request))
                    })?;
                    let llm_calls = Arc::clone(&llm_calls);
                    let llm_runtime_handle = runtime_handle.clone();
                    register_llm_proxy(linker, move |request: LlmProxyRequest| {
                        let call_number =
                            llm_calls.fetch_add(1, Ordering::Relaxed).saturating_add(1);
                        if call_number > max_turns {
                            return Err(format!(
                                "WASM model-call budget exhausted: {max_turns} calls allowed"
                            ));
                        }
                        let remaining = run_deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            return Err("WASM run deadline expired before model call".to_string());
                        }
                        let request_config = sanitize_guest_model_config(request.config);
                        let tools: Vec<Box<dyn Tool>> = request
                            .tools
                            .into_iter()
                            .map(|definition| {
                                Box::new(DefinitionOnlyTool(definition)) as Box<dyn Tool>
                            })
                            .collect();
                        let tool_refs = tools.iter().map(Box::as_ref).collect::<Vec<_>>();
                        let events = llm_runtime_handle
                            .block_on(async {
                                tokio::time::timeout(remaining, async {
                                    let mut stream = model.stream(
                                        &request.messages,
                                        &tool_refs,
                                        &request_config,
                                    );
                                    let mut events = Vec::new();
                                    let mut encoded_event_bytes = 0_usize;
                                    let event_budget = MAX_LLM_PROXY_RESPONSE_BYTES
                                        .saturating_sub(LLM_RESPONSE_ENVELOPE_RESERVE);
                                    while let Some(event) = stream.next().await {
                                        let separator = usize::from(!events.is_empty());
                                        let remaining_event_budget = event_budget
                                            .saturating_sub(encoded_event_bytes)
                                            .saturating_sub(separator);
                                        let event_bytes =
                                            bounded_json_size(&event, remaining_event_budget)
                                                .map_err(|_| {
                                                    format!(
                                                        "model stream exceeds the \
                                                     {MAX_LLM_PROXY_RESPONSE_BYTES}-byte WASM \
                                                     proxy response limit"
                                                    )
                                                })?;
                                        encoded_event_bytes = encoded_event_bytes
                                            .saturating_add(separator)
                                            .saturating_add(event_bytes);
                                        events.push(event);
                                    }
                                    Ok::<_, String>(events)
                                })
                                .await
                            })
                            .map_err(|_| {
                                format!(
                                    "model call exceeded the remaining WASM run deadline of {} ms",
                                    remaining.as_millis()
                                )
                            })??;
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

fn permission_mode_arg(mode: PermissionMode) -> &'static str {
    match mode {
        PermissionMode::Ask => "ask",
        PermissionMode::AcceptEdits => "accept-edits",
        PermissionMode::AcceptShell => "accept-shell",
        PermissionMode::Plan => "plan",
        PermissionMode::Bypass => "bypass",
    }
}

fn escalation_call_budget(max_turns: u32) -> u32 {
    max_turns.saturating_mul(8).clamp(8, 1024)
}

fn read_worker_module(path: &Path) -> Result<Vec<u8>, String> {
    read_worker_module_with_limit(path, MAX_WASM_MODULE_BYTES)
}

fn read_worker_module_with_limit(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path)
        .map_err(|error| format!("open alva-worker-wasm.wasm {}: {error}", path.display()))?;
    let max_plus_one = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    let mut module = Vec::new();
    file.take(max_plus_one)
        .read_to_end(&mut module)
        .map_err(|error| format!("read alva-worker-wasm.wasm {}: {error}", path.display()))?;
    if module.len() > limit {
        return Err(format!(
            "alva-worker-wasm.wasm is {} bytes; limit is {limit} bytes",
            module.len()
        ));
    }
    Ok(module)
}

fn grant_for_permission_mode(host: PathBuf, guest: &str, mode: PermissionMode) -> Grant {
    if matches!(mode, PermissionMode::Ask | PermissionMode::Plan) {
        Grant::read_only(host, guest.to_string())
    } else {
        Grant::read_write(host, guest.to_string())
    }
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
    fn build_run_limits_keeps_library_default_when_unset() {
        // No CLI override -> the RunRequest gets exactly RunLimits::default(),
        // so the existing wall clock and fuel defaults in alva-sandbox-wasm
        // are untouched.
        assert_eq!(build_run_limits(None, None), RunLimits::default());
    }

    #[test]
    fn build_run_limits_threads_the_cli_override_into_wall_clock() {
        // This is the "does the CLI value actually reach RunRequest.limits"
        // proof: build_run_limits is the exact function wired into the
        // RunRequest constructed in `run` above (see the `limits:` field),
        // so asserting its output pins the override end to end without
        // needing a real 30s+ sandbox run.
        let limits = build_run_limits(Some(90), None);
        assert_eq!(limits.wall_clock, Duration::from_secs(90));
        // fuel keeps the library default since only wall_clock was overridden.
        assert_eq!(limits.fuel, RunLimits::default().fuel);
        // max_memory_bytes must stay at the library default; only wall_clock
        // and fuel are CLI-overridable per the task's scope.
        assert_eq!(
            limits.max_memory_bytes,
            RunLimits::default().max_memory_bytes
        );
    }

    #[test]
    fn build_run_limits_threads_the_cli_override_into_fuel() {
        let limits = build_run_limits(None, Some(123_456));
        assert_eq!(limits.fuel, 123_456);
        // wall_clock keeps the library default since only fuel was overridden.
        assert_eq!(limits.wall_clock, RunLimits::default().wall_clock);
        assert_eq!(
            limits.max_memory_bytes,
            RunLimits::default().max_memory_bytes
        );
    }

    #[test]
    fn build_run_limits_threads_both_overrides_independently() {
        let limits = build_run_limits(Some(45), Some(999));
        assert_eq!(limits.wall_clock, Duration::from_secs(45));
        assert_eq!(limits.fuel, 999);
    }

    #[test]
    fn ask_and_plan_use_read_only_wasm_preopens() {
        use alva_sandbox_wasm::Access;

        let host = PathBuf::from("/tmp/example");
        for mode in [PermissionMode::Ask, PermissionMode::Plan] {
            assert_eq!(
                grant_for_permission_mode(host.clone(), "/workspace", mode).access,
                Access::ReadOnly
            );
        }
        assert_eq!(
            grant_for_permission_mode(host, "/workspace", PermissionMode::AcceptEdits).access,
            Access::ReadWrite
        );
    }

    #[test]
    fn llm_proxy_event_sizing_is_bounded_without_allocating_an_encoded_copy() {
        let event = alva_llm_wire::StreamEvent::TextDelta {
            text: "x".repeat(1024),
        };
        assert!(bounded_json_size(&event, 2048).is_ok());
        let error = bounded_json_size(&event, 64).unwrap_err();
        assert!(error.contains("64-byte budget"), "{error}");
    }

    #[test]
    fn guest_model_config_cannot_override_host_cost_and_body_policy() {
        let mut extra_body = serde_json::Map::new();
        extra_body.insert("model".to_string(), serde_json::json!("expensive-model"));
        let config = sanitize_guest_model_config(ModelConfig {
            max_tokens: Some(u32::MAX),
            extra_body: Some(extra_body),
            ..ModelConfig::default()
        });
        assert_eq!(config.max_tokens, Some(MAX_GUEST_REQUESTED_OUTPUT_TOKENS));
        assert!(config.extra_body.is_none());
    }

    #[test]
    fn escalation_import_budget_is_bounded_even_for_extreme_turn_counts() {
        assert_eq!(escalation_call_budget(1), 8);
        assert_eq!(escalation_call_budget(20), 160);
        assert_eq!(escalation_call_budget(u32::MAX), 1024);
    }

    #[test]
    fn worker_module_read_stops_at_the_compilation_input_limit() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("worker.wasm");
        std::fs::write(&path, vec![0_u8; 65]).unwrap();
        let error = read_worker_module_with_limit(&path, 64).unwrap_err();
        assert!(error.contains("limit"), "{error}");
    }

    #[test]
    fn explicit_worker_override_must_be_a_file() {
        let temp = tempfile::tempdir().unwrap();
        let error = validate_worker_path(temp.path().to_path_buf(), "test")
            .expect_err("directory is not a worker module");
        assert!(error.contains("not a file"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a prebuilt alva-worker-wasm.wasm sidecar"]
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
            PermissionMode::AcceptEdits,
            false,
            20,
            None,
            None,
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
        let full_prompt = calls[0]
            .iter()
            .map(|message| message.text_content())
            .collect::<Vec<_>>()
            .join("\n");
        let environment_skill = crate::bundled_skills::wasm_environment_skill_injection()
            .await
            .unwrap();
        assert!(
            full_prompt.contains(&environment_skill),
            "production wasm bridge must deliver the full bundled environment skill: {full_prompt}"
        );
        assert!(calls[1]
            .iter()
            .flat_map(|message| &message.content)
            .filter_map(|block| block.as_tool_result())
            .flat_map(|(_, content, _)| content)
            .filter_map(|content| content.as_text())
            .any(|text| text.contains("hello wasm")));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a prebuilt alva-worker-wasm.wasm sidecar"]
    async fn bypass_wasm_golden_repairs_after_host_test_failure() {
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
            PermissionMode::Bypass,
            true,
            20,
            None,
            None,
            "Create the crate, run its tests, repair failures, and verify again".into(),
        )
        .await
        .expect("explicit bypass escalation loop succeeds");

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
    #[ignore = "requires a prebuilt alva-worker-wasm.wasm sidecar"]
    async fn ask_mode_denies_escalation_in_guest_and_finishes_gracefully() {
        let workspace = tempfile::tempdir().unwrap();
        let mock = MockLanguageModel::new()
            .with_response(make_tool_call_message(
                "request_escalation",
                json!({"command": "cargo test --offline", "cwd": "/workspace"}),
            ))
            .with_response(make_assistant_message(
                "Could not run tests because Ask cannot leave the WASM sandbox.",
            ));
        let recorded = mock.clone();

        let result = run(
            Arc::new(mock),
            vec![workspace.path().canonicalize().unwrap()],
            Vec::new(),
            PermissionMode::Ask,
            false,
            20,
            None,
            None,
            "Run tests, or report the exact host denial".into(),
        )
        .await
        .expect("Ask rejection is a recoverable tool result");

        assert!(result.contains("Ask cannot leave"));
        let calls = recorded.calls();
        assert_eq!(calls.len(), 2);
        let (denial, is_error) = tool_result_text(&calls[1], "request_escalation");
        assert!(is_error, "Ask rejection must be an error tool result");
        assert!(denial.contains("outside the WASM sandbox"), "{denial}");
        assert!(denial.contains("only bypass"), "{denial}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires a prebuilt alva-worker-wasm.wasm sidecar"]
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
            false,
            20,
            None,
            None,
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
        assert!(denial.contains("outside the WASM sandbox"), "{denial}");
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
            true,
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
    async fn accept_shell_never_executes_outside_wasm_confinement() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let marker = outside.path().join("marker.txt");
        let command = format!("printf escaped > '{}'", marker.display());
        let handler = HostEscalationHandler::new(
            tokio::runtime::Handle::current(),
            vec![Grant::read_write(
                workspace.path().canonicalize().unwrap(),
                "/workspace",
            )],
            PermissionMode::AcceptShell,
            false,
            None,
        )
        .unwrap();

        let result = handler.execute(EscalationProxyRequest::new(command, "/workspace", 5_000));

        assert!(result.response.is_none());
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("outside the WASM sandbox"));
        assert!(!marker.exists(), "host command must not execute");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bypass_still_requires_dangerous_unsandboxed_opt_in() {
        let workspace = tempfile::tempdir().unwrap();
        let marker = workspace.path().join("marker.txt");
        let handler = HostEscalationHandler::new(
            tokio::runtime::Handle::current(),
            vec![Grant::read_write(
                workspace.path().canonicalize().unwrap(),
                "/workspace",
            )],
            PermissionMode::Bypass,
            false,
            None,
        )
        .unwrap();

        let result = handler.execute(EscalationProxyRequest::new(
            "touch marker.txt",
            "/workspace",
            5_000,
        ));
        assert!(result.response.is_none());
        assert!(result
            .error
            .as_deref()
            .unwrap()
            .contains("--dangerously-allow-unsandboxed"));
        assert!(!marker.exists());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn escalation_import_rejects_calls_past_its_host_budget() {
        let workspace = tempfile::tempdir().unwrap();
        let handler = HostEscalationHandler::new(
            tokio::runtime::Handle::current(),
            vec![Grant::read_write(
                workspace.path().canonicalize().unwrap(),
                "/workspace",
            )],
            PermissionMode::AcceptShell,
            false,
            None,
        )
        .unwrap();
        for _ in 0..DEFAULT_TEST_ESCALATION_CALL_BUDGET {
            let result = handler.execute(EscalationProxyRequest::new("true", "/workspace", 1_000));
            assert!(result.error.unwrap().contains("outside the WASM sandbox"));
        }
        let result = handler.execute(EscalationProxyRequest::new("true", "/workspace", 1_000));
        assert!(result.error.unwrap().contains("call budget exhausted"));
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
                PermissionMode::Bypass,
                true,
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
