// INPUT:  alva_agent_core, alva_agent_extension_builtin::CorePlugin, alva_kernel_abi, alva_kernel_core, futures, serde, serde_json, std::{alloc, fs, slice, sync::Arc}, host import alva:host/llm::llm_complete
// OUTPUT: alloc(len) wasm export, ProxyModel, /work/result.txt containing the sandboxed agent's final answer
// POS:    WASIp1 worker running the SDK agent loop with local WASI file tools and a blocking host-proxied model.

#[cfg(target_os = "wasi")]
use std::alloc::{self, Layout};
#[cfg(target_os = "wasi")]
use std::collections::HashMap;
#[cfg(target_os = "wasi")]
use std::pin::Pin;
#[cfg(target_os = "wasi")]
use std::sync::Arc;
#[cfg(target_os = "wasi")]
use std::{fs, slice};

#[cfg(target_os = "wasi")]
use alva_agent_core::Agent;
#[cfg(target_os = "wasi")]
use alva_agent_extension_builtin::wrappers::CorePlugin;
#[cfg(target_os = "wasi")]
use alva_kernel_abi::{
    AgentError, AgentMessage, CancellationToken, CompletionResponse, ContentBlock, LanguageModel,
    Message, MessageRole, ModelConfig, StreamEvent, Tool,
};
#[cfg(target_os = "wasi")]
use alva_kernel_core::AgentEvent;
#[cfg(target_os = "wasi")]
use async_trait::async_trait;
#[cfg(target_os = "wasi")]
use futures::{executor::block_on, stream, Stream};
#[cfg(target_os = "wasi")]
use serde::Serialize;

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
    if len == 0 {
        return 0;
    }
    // SAFETY: `len` is non-zero, so the layout has non-zero size as
    // `alloc::alloc` requires.
    unsafe { alloc::alloc(response_layout(len)) as i32 }
}

#[cfg(target_os = "wasi")]
#[derive(Serialize)]
struct LlmProxyRequest<'a> {
    messages: &'a [Message],
    tools: Vec<ProxyToolDefinition>,
    config: &'a ModelConfig,
}

#[cfg(target_os = "wasi")]
#[derive(Serialize)]
struct ProxyToolDefinition {
    name: String,
    description: String,
    parameters_schema: serde_json::Value,
}

#[cfg(target_os = "wasi")]
struct ProxyModel;

#[cfg(target_os = "wasi")]
impl ProxyModel {
    fn request_events(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Result<Vec<StreamEvent>, AgentError> {
        let request = LlmProxyRequest {
            messages,
            tools: tools
                .iter()
                .map(|tool| ProxyToolDefinition {
                    name: tool.name().to_string(),
                    description: tool.description().to_string(),
                    parameters_schema: tool.parameters_schema(),
                })
                .collect(),
            config,
        };
        let request = serde_json::to_vec(&request)
            .map_err(|error| AgentError::Other(format!("serialize LLM proxy request: {error}")))?;
        let req_len = i32::try_from(request.len())
            .map_err(|_| AgentError::Other("LLM proxy request exceeds ptr/len ABI limit".into()))?;
        let packed = unsafe { llm_complete(request.as_ptr() as i32, req_len) } as u64;
        let response = take_host_response(packed)?;
        serde_json::from_slice(&response)
            .map_err(|error| AgentError::Other(format!("decode LLM proxy response: {error}")))
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
        Ok(CompletionResponse::from_message(message_from_events(
            events,
        )?))
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
fn take_host_response(packed: u64) -> Result<Vec<u8>, AgentError> {
    let resp_ptr = (packed >> 32) as u32 as usize;
    let resp_len = (packed & u32::MAX as u64) as u32 as usize;

    if resp_len == 0 {
        return Err(AgentError::Other(
            "host returned an empty LLM proxy response".into(),
        ));
    }
    if resp_ptr == 0 {
        return Err(AgentError::Other(
            "host returned a null pointer for a non-empty response".into(),
        ));
    }
    // SAFETY: the host obtained this allocation from `alloc(resp_len)` and
    // initialized exactly `resp_len` bytes before returning. The guest copies
    // them out and frees with the same layout `alloc` used.
    let bytes = unsafe { slice::from_raw_parts(resp_ptr as *const u8, resp_len) }.to_vec();
    unsafe { alloc::dealloc(resp_ptr as *mut u8, response_layout(resp_len)) };
    Ok(bytes)
}

#[cfg(target_os = "wasi")]
fn message_from_events(events: Vec<StreamEvent>) -> Result<Message, AgentError> {
    let mut text = String::new();
    let mut reasoning = Vec::new();
    let mut usage = None;
    let mut calls: Vec<(String, String, String)> = Vec::new();
    let mut call_indices = HashMap::<String, usize>::new();
    let mut last_call = None;

    for event in events {
        match event {
            StreamEvent::TextDelta { text: delta } => text.push_str(&delta),
            StreamEvent::ReasoningBlock { text, signature } => {
                reasoning.push(ContentBlock::Reasoning { text, signature });
            }
            StreamEvent::ToolCallStart { id, name } => {
                let index = *call_indices.entry(id.clone()).or_insert_with(|| {
                    calls.push((id, String::new(), String::new()));
                    calls.len() - 1
                });
                calls[index].1 = name;
                last_call = Some(index);
            }
            StreamEvent::ToolCallDelta {
                id,
                name,
                arguments_delta,
            } => {
                let index = if id.is_empty() {
                    last_call.ok_or_else(|| {
                        AgentError::LlmError("tool-call delta has no id or preceding call".into())
                    })?
                } else if let Some(index) = call_indices.get(&id).copied() {
                    index
                } else {
                    calls.push((id.clone(), String::new(), String::new()));
                    let index = calls.len() - 1;
                    call_indices.insert(id, index);
                    index
                };
                if let Some(name) = name.filter(|name| !name.is_empty()) {
                    calls[index].1 = name;
                }
                calls[index].2.push_str(&arguments_delta);
                last_call = Some(index);
            }
            StreamEvent::Usage(value) => usage = Some(value),
            StreamEvent::Error(error) => return Err(AgentError::LlmError(error)),
            StreamEvent::Start
            | StreamEvent::Done
            | StreamEvent::ReasoningDelta { .. }
            | StreamEvent::ToolCallEnd { .. }
            | StreamEvent::Stop { .. } => {}
        }
    }

    let mut content = reasoning;
    if !text.is_empty() {
        content.push(ContentBlock::Text { text });
    }
    for (id, name, arguments) in calls {
        let input = serde_json::from_str(&arguments).map_err(|error| {
            AgentError::LlmError(format!(
                "invalid tool arguments for '{name}' ({id}): {error}"
            ))
        })?;
        content.push(ContentBlock::ToolUse { id, name, input });
    }

    Ok(Message {
        id: next_response_id(),
        role: MessageRole::Assistant,
        content,
        tool_call_id: None,
        usage,
        timestamp: 0,
    })
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
async fn run_worker(task: String) -> Result<String, AgentError> {
    // No tool timeout is installed on purpose. `ToolTimeoutMiddleware` needs a
    // `Sleeper`, and the only one available to a WASIp1 guest today is
    // `NoopSleeper`, whose `sleep` is `future::pending()` — it never resolves,
    // so `timeout`'s `select` can only ever return the work arm. Wiring it up
    // would read as "tools time out" while enforcing nothing; a hung tool would
    // still hang the worker forever, just less visibly. Ticket 06 (run_script
    // wants a real execution timeout) needs a WASI-clock-backed `Sleeper`
    // first; installing the middleware without one buys nothing.
    let agent = Agent::builder()
        .model(Arc::new(ProxyModel))
        .workspace("/work")
        .system_prompt(
            "Complete the user's task using the available file tools. All relative paths are rooted in /work.",
        )
        .plugin(Box::new(CorePlugin))
        .build()
        .await?;

    let mut events = agent
        .run(
            vec![AgentMessage::Standard(Message::user(task))],
            CancellationToken::new(),
        )
        .await?;
    // Drain to channel close rather than to "nothing buffered right now":
    // `run` sends every event before it returns (its sender is consumed by
    // `run_agent`, so the channel is already closed here) — but `try_recv`
    // would silently stop at the first gap if events ever start arriving while
    // the loop is still running, losing the final message.
    let mut final_text = None;
    while let Some(event) = events.recv().await {
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
fn main() {
    let task = fs::read_to_string("/work/task.txt").expect("read /work/task.txt");
    let response = block_on(run_worker(task)).expect("run sandboxed agent loop");

    fs::write("/work/result.txt", response).expect("write /work/result.txt");
}

#[cfg(not(target_os = "wasi"))]
fn main() {
    eprintln!("alva-worker-wasm is a wasm32-wasip1 guest binary");
}
