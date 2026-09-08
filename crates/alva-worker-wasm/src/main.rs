// INPUT:  WASI args including permission/turn budgets, host environment context, alva_agent_core, builtin file/escalation tools, RunScriptTool, alva_kernel_{abi,core}, alva_{llm_wire,sandbox_abi} proxy ABIs, futures, serde_json, std, host imports
// OUTPUT: alloc(len) wasm export plus permission-gated context/fetch/escalation/run_script/log-enabled agent and configurable file/stdout result channel
// POS:    WASIp1 worker running an SDK agent loop with host-supplied least-privilege policy, environment skill, and versioned capability proxies.

#[cfg(target_os = "wasi")]
mod context_proxy;

#[cfg(target_os = "wasi")]
mod escalation_proxy;

#[cfg(target_os = "wasi")]
mod http_proxy;

#[cfg(target_os = "wasi")]
mod job_log;

#[cfg(target_os = "wasi")]
mod run_script;

#[cfg(target_os = "wasi")]
use std::alloc::{self, Layout};
#[cfg(target_os = "wasi")]
use std::pin::Pin;
#[cfg(target_os = "wasi")]
use std::sync::Arc;
#[cfg(target_os = "wasi")]
use std::{env, fs, io, process, slice};

#[cfg(target_os = "wasi")]
use alva_agent_core::Agent;
#[cfg(target_os = "wasi")]
use alva_agent_extension_builtin::request_escalation::RequestEscalationTool;
#[cfg(target_os = "wasi")]
use alva_agent_extension_builtin::wrappers::CorePlugin;
#[cfg(target_os = "wasi")]
use alva_kernel_abi::{
    AgentError, AgentMessage, CancellationToken, CompletionResponse, LanguageModel, Message,
    ModelConfig, StreamEvent, Tool, ToolCall, ToolDefinition,
};
#[cfg(target_os = "wasi")]
use alva_kernel_core::{AgentEvent, AgentState, Middleware, MiddlewareError};
#[cfg(target_os = "wasi")]
use alva_llm_wire::{
    message_from_events, LlmProxyRequest, LlmProxyResponse, LLM_PROXY_ABI_VERSION,
    MAX_LLM_PROXY_REQUEST_BYTES, MAX_LLM_PROXY_RESPONSE_BYTES,
};
#[cfg(target_os = "wasi")]
use alva_sandbox_abi::{
    MAX_ESCALATION_PROXY_RESPONSE_BYTES, MAX_FETCH_PROXY_RESPONSE_BYTES,
    MAX_WASM_ENV_CONTEXT_RESPONSE_BYTES,
};
#[cfg(target_os = "wasi")]
use async_trait::async_trait;
#[cfg(target_os = "wasi")]
use futures::{executor::block_on, stream, Stream};
#[cfg(target_os = "wasi")]
#[link(wasm_import_module = "alva:host/llm")]
extern "C" {
    fn llm_complete(req_ptr: i32, req_len: i32) -> i64;
}

/// The layout every [`alloc`] result is created with: `len` bytes, align 1.
///
/// Alloc and dealloc must agree on the exact layout. This is why the guest
/// does not round-trip the host's buffer through `Vec`: `from_raw_parts`
/// demands the precise capacity the allocation was made with, while
/// `Vec::with_capacity(n)` only promises *at least* `n` — a mismatch would
/// free with the wrong layout, which is undefined behavior.
#[cfg(target_os = "wasi")]
fn response_layout(len: usize) -> Layout {
    Layout::from_size_align(len, 1).expect("response length fits a valid align-1 layout")
}

/// Allocates `len` bytes of guest linear memory for the host to fill.
///
/// Returns a pointer the host writes exactly `len` bytes into before
/// `llm_complete` returns. A zero-length request needs no allocation and
/// yields a null pointer, which the caller pairs with `resp_len == 0`.
#[cfg(target_os = "wasi")]
#[no_mangle]
pub extern "C" fn alloc(len: i32) -> i32 {
    let len = usize::try_from(len).expect("host requested a negative allocation");
    let max_response_bytes = MAX_LLM_PROXY_RESPONSE_BYTES
        .max(MAX_FETCH_PROXY_RESPONSE_BYTES)
        .max(MAX_ESCALATION_PROXY_RESPONSE_BYTES)
        .max(MAX_WASM_ENV_CONTEXT_RESPONSE_BYTES);
    assert!(
        len <= max_response_bytes,
        "host response exceeds the proxy response size limit"
    );
    if len == 0 {
        return 0;
    }
    // SAFETY: `len` is non-zero, so the layout has non-zero size as
    // `alloc::alloc` requires.
    let layout = response_layout(len);
    let pointer = unsafe { alloc::alloc(layout) };
    if pointer.is_null() {
        alloc::handle_alloc_error(layout);
    }
    pointer as i32
}

#[cfg(any(target_os = "wasi", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerPermissionMode {
    Ask,
    AcceptEdits,
    AcceptShell,
    Plan,
    Bypass,
}

#[cfg(any(target_os = "wasi", test))]
impl WorkerPermissionMode {
    #[cfg(target_os = "wasi")]
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "ask" => Ok(Self::Ask),
            "accept-edits" => Ok(Self::AcceptEdits),
            "accept-shell" => Ok(Self::AcceptShell),
            "plan" => Ok(Self::Plan),
            "bypass" => Ok(Self::Bypass),
            other => Err(format!("unknown worker permission mode {other:?}")),
        }
    }

    fn allows_tool(self, name: &str) -> bool {
        match name {
            "read_file" | "list_files" | "find_files" | "grep_search" => true,
            "create_file" | "file_edit" => {
                matches!(self, Self::AcceptEdits | Self::AcceptShell | Self::Bypass)
            }
            "run_script" => matches!(self, Self::AcceptShell | Self::Bypass),
            // Host escalation exits the WASM confinement boundary. Only an
            // explicit bypass opt-in may expose it to the guest.
            "request_escalation" => self == Self::Bypass,
            _ => false,
        }
    }

    fn denial_reason(self, name: &str) -> String {
        if name == "request_escalation" {
            format!(
                "host escalation executes outside the WASM sandbox and is not allowed in \
                 permission mode {self:?}; only bypass may request it, and the host separately \
                 requires --dangerously-allow-unsandboxed"
            )
        } else {
            format!("tool {name} is not allowed in WASM permission mode {self:?}")
        }
    }
}

#[cfg(target_os = "wasi")]
struct PermissionGateMiddleware {
    mode: WorkerPermissionMode,
}

#[cfg(target_os = "wasi")]
#[async_trait]
impl Middleware for PermissionGateMiddleware {
    async fn before_tool_call(
        &self,
        _state: &AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        if self.mode.allows_tool(&tool_call.name) {
            Ok(())
        } else {
            Err(MiddlewareError::Blocked {
                reason: self.mode.denial_reason(&tool_call.name),
            })
        }
    }

    fn name(&self) -> &str {
        "wasm-permission-gate"
    }
}

#[cfg(target_os = "wasi")]
struct ProxyModel;

#[cfg(target_os = "wasi")]
struct BoundedJsonBuffer {
    bytes: Vec<u8>,
    limit: usize,
    exceeded: bool,
}

#[cfg(target_os = "wasi")]
impl BoundedJsonBuffer {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
            exceeded: false,
        }
    }
}

#[cfg(target_os = "wasi")]
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

#[cfg(target_os = "wasi")]
impl ProxyModel {
    fn request_events(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Result<Vec<StreamEvent>, AgentError> {
        let proxy_tools = tools
            .iter()
            .map(|tool| ToolDefinition {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                parameters: tool.parameters_schema(&alva_kernel_abi::ToolSchemaContext::empty()),
            })
            .collect();
        let request = LlmProxyRequest::new(messages.to_vec(), proxy_tools, config.clone());
        let mut encoded = BoundedJsonBuffer::new(MAX_LLM_PROXY_REQUEST_BYTES);
        if let Err(error) = serde_json::to_writer(&mut encoded, &request) {
            return Err(AgentError::Other(if encoded.exceeded {
                format!("LLM proxy request exceeds the {MAX_LLM_PROXY_REQUEST_BYTES}-byte limit")
            } else {
                format!("serialize LLM proxy request: {error}")
            }));
        }
        let request = encoded.bytes;
        let req_len = i32::try_from(request.len())
            .map_err(|_| AgentError::Other("LLM proxy request exceeds ptr/len ABI limit".into()))?;
        let packed = unsafe { llm_complete(request.as_ptr() as i32, req_len) } as u64;
        let response = take_host_response(packed, "LLM proxy", MAX_LLM_PROXY_RESPONSE_BYTES)?;
        let response: LlmProxyResponse = serde_json::from_slice(&response)
            .map_err(|error| AgentError::Other(format!("decode LLM proxy response: {error}")))?;
        if !response.has_supported_version() {
            return Err(AgentError::Other(format!(
                "unsupported LLM proxy response version {}; guest supports {}",
                response.version, LLM_PROXY_ABI_VERSION
            )));
        }
        Ok(response.events)
    }
}

#[cfg(target_os = "wasi")]
#[async_trait]
impl LanguageModel for ProxyModel {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        let events = self.request_events(messages, tools, config)?;
        let message = message_from_events(events, next_response_id(), 0)
            .map_err(|error| AgentError::LlmError(error.to_string()))?;
        Ok(CompletionResponse::from_message(message))
    }

    fn stream(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        match self.request_events(messages, tools, config) {
            Ok(events) => Box::pin(stream::iter(events)),
            Err(error) => Box::pin(stream::iter(vec![
                StreamEvent::Start,
                StreamEvent::Error(error.to_string()),
            ])),
        }
    }

    fn model_id(&self) -> &str {
        "host-proxy"
    }

    fn provider_id(&self) -> &str {
        "host"
    }
}

#[cfg(target_os = "wasi")]
fn take_host_response(
    packed: u64,
    channel: &str,
    max_response_bytes: usize,
) -> Result<Vec<u8>, AgentError> {
    let resp_ptr = (packed >> 32) as u32 as usize;
    let resp_len = (packed & u32::MAX as u64) as u32 as usize;

    if resp_len == 0 {
        return Err(AgentError::Other(format!(
            "host returned an empty {channel} response"
        )));
    }
    if resp_ptr == 0 {
        return Err(AgentError::Other(format!(
            "host returned a null pointer for a non-empty {channel} response"
        )));
    }
    if resp_len > max_response_bytes {
        return Err(AgentError::Other(format!(
            "{channel} response is {resp_len} bytes; limit is {max_response_bytes} bytes"
        )));
    }
    // SAFETY: the host obtained this allocation from `alloc(resp_len)` and
    // initialized exactly `resp_len` bytes before returning. The guest copies
    // them out and frees with the same layout `alloc` used.
    let bytes = unsafe { slice::from_raw_parts(resp_ptr as *const u8, resp_len) }.to_vec();
    unsafe { alloc::dealloc(resp_ptr as *mut u8, response_layout(resp_len)) };
    Ok(bytes)
}

/// Message ids must be unique per response: a multi-turn run produces several
/// assistant messages, and anything keying on the id (session storage, tool_call
/// correlation) would fold them into one if they shared a literal. A counter is
/// enough here — the guest is one-shot and single-threaded, and a deterministic
/// id keeps tests readable. `uuid` would drag `getrandom` into the guest for no
/// added guarantee.
#[cfg(target_os = "wasi")]
fn next_response_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "host-proxy-response-{}",
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(target_os = "wasi")]
async fn run_worker(
    task: String,
    workspace: String,
    grants: Vec<String>,
    permission_mode: WorkerPermissionMode,
    max_turns: u32,
    script_limits: run_script::ScriptLimits,
) -> Result<String, AgentError> {
    // No tool timeout is installed on purpose. `ToolTimeoutMiddleware` needs a
    // `Sleeper`, and the only one available to a WASIp1 guest today is
    // `NoopSleeper`, whose `sleep` is `future::pending()` — it never resolves,
    // so `timeout`'s `select` can only ever return the work arm. Wiring it up
    // would read as "tools time out" while enforcing nothing; a hung tool would
    // still hang the worker forever, just less visibly. Ticket 06 (run_script
    // wants a real execution timeout) needs a WASI-clock-backed `Sleeper`
    // first; installing the middleware without one buys nothing.
    let grant_description = grants
        .iter()
        .enumerate()
        .map(|(index, path)| format!("grant {index}: {path}"))
        .collect::<Vec<_>>()
        .join(", ");
    let environment_skill = context_proxy::load_wasm_environment_skill()?;
    let system_prompt = format!(
        "## Runtime mount map\n\nRelative paths are rooted in {workspace}. The only host directories available through WASI are: {grant_description}.\n\n{environment_skill}"
    );
    let mut builder = Agent::builder()
        .model(Arc::new(ProxyModel))
        .workspace(&workspace)
        .system_prompt(&system_prompt)
        .max_iterations(max_turns)
        .plugin(Box::new(CorePlugin))
        .middleware(Arc::new(PermissionGateMiddleware {
            mode: permission_mode,
        }))
        .middleware(Arc::new(job_log::AuditLogMiddleware));
    // Keep the advertised tool surface stable across modes. PermissionGate
    // turns disallowed calls into recoverable tool errors, matching the native
    // agent's headless permission behavior instead of making tools disappear.
    builder = builder.tool(Box::new(RequestEscalationTool::new(Arc::new(
        escalation_proxy::HostImportEscalationExecutor,
    ))));
    builder = builder.tool(Box::new(
        run_script::RunScriptTool::new(&workspace).with_limits(script_limits),
    ));
    let agent = builder.build().await?;

    let mut events = std::pin::pin!(agent.run(
        vec![AgentMessage::Standard(Message::user(task))],
        CancellationToken::new(),
    ));
    // Drain the stream to completion. The run entry is lazily driven: each
    // poll advances the loop, so draining to `None` consumes every event of
    // the finished turn — `try_recv`-style early stops would silently lose
    // the final message.
    let mut final_text = None;
    use futures_util::StreamExt;
    while let Some(event) = events.next().await {
        if let AgentEvent::MessageEnd {
            message: AgentMessage::Standard(message),
        } = event
        {
            final_text = Some(message.text_content());
        }
    }

    final_text.ok_or_else(|| AgentError::Other("agent completed without a final message".into()))
}

#[cfg(target_os = "wasi")]
struct WorkerArgs {
    workspace: String,
    task: String,
    result: String,
    grants: Vec<String>,
    permission_mode: WorkerPermissionMode,
    max_turns: u32,
    /// Per-script ceilings. Absent flags keep `ScriptLimits::default()`.
    script_timeout_ms: Option<u64>,
    script_memory_bytes: Option<usize>,
}

#[cfg(target_os = "wasi")]
fn parse_worker_args() -> Result<WorkerArgs, String> {
    // Skip argv[0]: WASI follows the convention that the first argument is the
    // program name, so treating it as a flag would make `wasmtime run
    // alva-worker-wasm.wasm --workspace ...` — the natural way to drive the
    // guest by hand — fail with "unknown worker argument".
    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut workspace = None;
    let mut task = None;
    let mut result = None;
    let mut grants = Vec::new();
    let mut permission_mode = None;
    let mut max_turns = None;
    let mut script_timeout_ms = None;
    let mut script_memory_bytes = None;
    let mut index = 0;
    while index < args.len() {
        let flag = args[index].as_str();
        let value = args
            .get(index + 1)
            .cloned()
            .ok_or_else(|| format!("{flag} expects a value"))?;
        if flag != "--task" && value.starts_with("--") {
            return Err(format!("{flag} expects a value"));
        }
        match flag {
            "--workspace" => workspace = Some(value),
            "--task" => task = Some(value),
            "--result" => result = Some(value),
            "--grant" => grants.push(value),
            "--permission-mode" => permission_mode = Some(WorkerPermissionMode::parse(&value)?),
            "--max-turns" => {
                let parsed = value
                    .parse::<u32>()
                    .map_err(|error| format!("--max-turns {value:?}: {error}"))?;
                if parsed == 0 {
                    return Err("--max-turns must be greater than zero".to_string());
                }
                max_turns = Some(parsed);
            }
            "--script-timeout-ms" => {
                script_timeout_ms = Some(
                    value
                        .parse::<u64>()
                        .map_err(|error| format!("--script-timeout-ms {value:?}: {error}"))?,
                )
            }
            "--script-memory-bytes" => {
                script_memory_bytes = Some(
                    value
                        .parse::<usize>()
                        .map_err(|error| format!("--script-memory-bytes {value:?}: {error}"))?,
                )
            }
            unknown => return Err(format!("unknown worker argument {unknown:?}")),
        }
        index += 2;
    }
    let workspace = workspace.ok_or_else(|| "--workspace is required".to_string())?;
    let task = task.ok_or_else(|| "--task is required".to_string())?;
    let result = result.ok_or_else(|| "--result is required (`-` for stdout)".to_string())?;
    let permission_mode =
        permission_mode.ok_or_else(|| "--permission-mode is required".to_string())?;
    let max_turns = max_turns.ok_or_else(|| "--max-turns is required".to_string())?;
    if grants.is_empty() {
        return Err("at least one --grant guest path is required".to_string());
    }
    Ok(WorkerArgs {
        workspace,
        task,
        result,
        grants,
        permission_mode,
        max_turns,
        script_timeout_ms,
        script_memory_bytes,
    })
}

#[cfg(target_os = "wasi")]
fn main() {
    let outcome = parse_worker_args().and_then(|args| {
        let mut script_limits = run_script::ScriptLimits::default();
        if let Some(ms) = args.script_timeout_ms {
            script_limits.timeout = std::time::Duration::from_millis(ms);
        }
        if let Some(bytes) = args.script_memory_bytes {
            script_limits.memory_bytes = bytes;
        }
        let response = block_on(run_worker(
            args.task,
            args.workspace,
            args.grants,
            args.permission_mode,
            args.max_turns,
            script_limits,
        ))
        .map_err(|error| format!("sandboxed agent loop failed: {error}"))?;
        if args.result == "-" {
            print!("{response}");
            Ok(())
        } else {
            fs::write(&args.result, response)
                .map_err(|error| format!("write result to {}: {error}", args.result))
        }
    });

    if let Err(error) = outcome {
        eprintln!("worker failed: {error}");
        process::exit(1);
    }
}

#[cfg(not(target_os = "wasi"))]
fn main() {
    eprintln!("alva-worker-wasm is a wasm32-wasip1 guest binary");
}

#[cfg(test)]
mod tests {
    use super::WorkerPermissionMode as Mode;

    #[test]
    fn worker_permission_modes_expose_only_their_allowed_tools() {
        for mode in [Mode::Ask, Mode::Plan] {
            assert!(mode.allows_tool("read_file"));
            assert!(!mode.allows_tool("create_file"));
            assert!(!mode.allows_tool("run_script"));
            assert!(!mode.allows_tool("request_escalation"));
        }
        assert!(Mode::AcceptEdits.allows_tool("create_file"));
        assert!(!Mode::AcceptEdits.allows_tool("run_script"));
        assert!(Mode::AcceptShell.allows_tool("create_file"));
        assert!(Mode::AcceptShell.allows_tool("run_script"));
        assert!(!Mode::AcceptShell.allows_tool("request_escalation"));
        assert!(Mode::AcceptShell
            .denial_reason("request_escalation")
            .contains("outside the WASM sandbox"));
        assert!(Mode::Bypass.allows_tool("create_file"));
        assert!(Mode::Bypass.allows_tool("run_script"));
        assert!(Mode::Bypass.allows_tool("request_escalation"));
    }
}
