// INPUT:  axum, serde_json, alva_llm_provider::AliasRouter, alva_llm_wire::adapter::{ProtocolAdapter, DecodedResponse},
//         alva_kernel_abi::tool::Tool, crate::raw_tool::RawTool
// OUTPUT: GatewayError, app(), serve()
// POS:    Axum HTTP server wiring. Three POST routes map inbound protocol adapters through AliasRouter
//         to upstream LanguageModel::complete (non-streaming) or LanguageModel::stream (SSE passthrough).
//         Task 6.5: streaming branch drives stream() through encode_stream_event, writes SSE to client.

use std::convert::Infallible;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::response::sse::{Event, Sse};
use axum::routing::post;
use axum::Router;
use futures::StreamExt;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use alva_kernel_abi::base::error::AgentError;
use alva_llm_provider::AliasRouter;
use alva_llm_wire::adapter::{AdapterError, DecodedResponse, ProtocolAdapter, StreamEncodeState};
use alva_llm_wire::adapter::openai_chat::OpenAIChatAdapter;
use alva_llm_wire::adapter::openai_responses::OpenAIResponsesAdapter;
use alva_llm_wire::adapter::anthropic::AnthropicAdapter;
use alva_kernel_abi::tool::Tool;

use crate::raw_tool::RawTool;

// ---------------------------------------------------------------------------
// GatewayError
// ---------------------------------------------------------------------------

/// Errors produced by the gateway's request-handling pipeline.
///
/// Each variant maps to a specific HTTP status code. Error bodies are emitted
/// as `{"error":{"message":"..."}}` — a generic envelope acceptable for v1.
/// Per-protocol error shaping (OpenAI vs Anthropic envelopes) is a future refinement.
#[derive(Debug)]
pub enum GatewayError {
    /// Inbound request body could not be decoded (HTTP 400).
    DecodeRequest(AdapterError),
    /// The requested model alias is not registered in the router (HTTP 404).
    UnknownModel(String),
    /// Upstream LLM call failed (HTTP 502).
    Upstream(AgentError),
    /// The response could not be re-encoded into the inbound protocol's shape (HTTP 500).
    EncodeResponse(AdapterError),
}

impl GatewayError {
    fn status(&self) -> StatusCode {
        match self {
            GatewayError::DecodeRequest(_) => StatusCode::BAD_REQUEST,
            GatewayError::UnknownModel(_) => StatusCode::NOT_FOUND,
            GatewayError::Upstream(_) => StatusCode::BAD_GATEWAY,
            GatewayError::EncodeResponse(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn message(&self) -> String {
        match self {
            GatewayError::DecodeRequest(e) => format!("decode request: {e}"),
            GatewayError::UnknownModel(m) => format!("unknown model alias: {m}"),
            GatewayError::Upstream(e) => format!("upstream error: {e}"),
            GatewayError::EncodeResponse(e) => format!("encode response: {e}"),
        }
    }
}

impl axum::response::IntoResponse for GatewayError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status();
        let body = json!({ "error": { "message": self.message() } });
        (status, axum::Json(body)).into_response()
    }
}

// ---------------------------------------------------------------------------
// InboundProtocol — owned enum for constructing the concrete adapter
// ---------------------------------------------------------------------------
//
// The streaming SSE response outlives the `handle` call frame, so we cannot
// move a `&dyn ProtocolAdapter` (borrowed from the caller) into the stream
// closure. Instead we pass an owned enum that can reconstruct the zero-sized
// adapter inside the stream body.

#[derive(Clone, Copy)]
enum InboundProtocol {
    Responses,
    Chat,
    Anthropic,
}

impl InboundProtocol {
    /// Return a concrete adapter boxed as the trait object.
    /// All three adapters are unit / zero-sized structs (`Copy`), so
    /// constructing them is free.
    fn as_adapter(self) -> Box<dyn ProtocolAdapter + Send + Sync> {
        match self {
            InboundProtocol::Responses => Box::new(OpenAIResponsesAdapter::new()),
            InboundProtocol::Chat => Box::new(OpenAIChatAdapter::new()),
            InboundProtocol::Anthropic => Box::new(AnthropicAdapter::new()),
        }
    }
}

// ---------------------------------------------------------------------------
// Non-streaming path
// ---------------------------------------------------------------------------

/// Core request-handling pipeline for the non-streaming branch.
///
/// Flow:
/// 1. `inbound.decode_request(body)` → neutral `DecodedRequest`  (400 on error)
/// 2. `router.resolve(model)` → `Arc<dyn LanguageModel>`          (404 if missing)
/// 3. Wrap `ToolDefinition`s in `RawTool` to satisfy `&[&dyn Tool]`
/// 4. `lm.complete(messages, tools, config).await`                 (502 on error)
/// 5. `inbound.encode_response(DecodedResponse)`                   (500 on error)
/// 6. Inject real model alias into the response JSON (`model` field was `""`)
/// 7. Return `200 application/json`
async fn handle_non_streaming(
    inbound: &(dyn ProtocolAdapter + Send + Sync),
    router: &AliasRouter,
    body: Value,
) -> Result<axum::response::Response, GatewayError> {
    // 1. Decode inbound request
    let req = inbound
        .decode_request(&body)
        .map_err(GatewayError::DecodeRequest)?;

    // 2. Resolve the model alias
    let lm = router
        .resolve(&req.model)
        .ok_or_else(|| GatewayError::UnknownModel(req.model.clone()))?;

    // 3. Wrap tool definitions in passthrough RawTool wrappers
    let raw_tools: Vec<RawTool> = req
        .tools
        .iter()
        .map(|t| RawTool::new(t.name.clone(), t.description.clone(), t.parameters.clone()))
        .collect();
    let tool_refs: Vec<&dyn Tool> = raw_tools.iter().map(|t| t as &dyn Tool).collect();

    // 4. Call upstream
    let cr = lm
        .complete(&req.messages, &tool_refs, &req.config)
        .await
        .map_err(GatewayError::Upstream)?;

    // 5. Re-encode in the inbound protocol's format
    let dr = DecodedResponse {
        message: cr.message.clone(),
        usage: cr.message.usage.clone(),
    };
    let mut out = inbound
        .encode_response(&dr)
        .map_err(GatewayError::EncodeResponse)?;

    // 6. MODEL ECHO FIX: encode_response emits model:"" — inject the real alias
    if let Some(obj) = out.as_object_mut() {
        obj.insert("model".into(), Value::String(req.model.clone()));
    }

    // 7. Return JSON 200
    let body_bytes = serde_json::to_vec(&out)
        .map_err(|e| GatewayError::EncodeResponse(AdapterError::UnexpectedFormat(e.to_string())))?;

    let response = Response::builder()
        .status(StatusCode::OK)
        .header("Content-Type", "application/json")
        .body(Body::from(body_bytes))
        .unwrap();

    Ok(response)
}

// ---------------------------------------------------------------------------
// Streaming path (Task 6.5)
// ---------------------------------------------------------------------------

/// Handle a streaming request by driving `LanguageModel::stream()` through
/// the inbound adapter's `encode_stream_event` and writing the resulting
/// frames as an SSE response.
///
/// The `InboundProtocol` enum (not a `&dyn` borrow) is passed so it can be
/// moved into the async stream closure without lifetime issues.
async fn handle_streaming(
    protocol: InboundProtocol,
    router: &AliasRouter,
    body: Value,
) -> Result<axum::response::Response, GatewayError> {
    // Decode request using the concrete adapter (owned, no borrow)
    let adapter = protocol.as_adapter();
    let req = adapter
        .decode_request(&body)
        .map_err(GatewayError::DecodeRequest)?;

    // Resolve the model alias
    let lm = router
        .resolve(&req.model)
        .ok_or_else(|| GatewayError::UnknownModel(req.model.clone()))?;

    // Build raw tools, call stream() inside a scoped block so tool_refs
    // are dropped before we move data into the stream closure.
    let event_stream = {
        let raw_tools: Vec<RawTool> = req
            .tools
            .iter()
            .map(|t| RawTool::new(t.name.clone(), t.description.clone(), t.parameters.clone()))
            .collect();
        let tool_refs: Vec<&dyn Tool> = raw_tools.iter().map(|t| t as &dyn Tool).collect();
        // The provider builds its request body synchronously inside stream()
        // (see openai_chat.rs); the returned Pin<Box<dyn Stream>> owns its
        // own copy of all data. tool_refs (and raw_tools) are safe to drop here.
        lm.stream(&req.messages, &tool_refs, &req.config)
        // raw_tools, tool_refs dropped here — safe because stream() already captured what it needs
    };

    // Build SSE output stream.
    // `protocol` (Copy enum) and `StreamEncodeState` (owned) live in the closure,
    // satisfying 'static lifetime required by axum's Sse<S>.
    let output_stream = async_stream::stream! {
        let mut st = StreamEncodeState::default();
        // Pin the upstream stream so we can poll it with StreamExt::next.
        tokio::pin!(event_stream);

        while let Some(ev) = event_stream.next().await {
            // Construct the concrete adapter fresh for each batch of frames
            // (zero-cost — all adapters are unit/ZST).
            let inbound = protocol.as_adapter();
            match inbound.encode_stream_event(&ev, &mut st) {
                Ok(frames) => {
                    for frame in frames {
                        let sse_event = match &frame.data {
                            // [DONE] sentinel — Chat Completions terminates with this literal string.
                            // The SseFrame carries it as Value::String("[DONE]"); we must emit it
                            // as a raw data line, NOT JSON-quoted.
                            Value::String(s) if s == "[DONE]" => {
                                Event::default().data("[DONE]")
                            }
                            // Named event frame — encode data as JSON
                            other => {
                                let data_str = match serde_json::to_string(other) {
                                    Ok(s) => s,
                                    Err(e) => {
                                        // Yield an error event rather than silently dropping.
                                        yield Ok::<Event, Infallible>(
                                            Event::default()
                                                .event("error")
                                                .data(format!("{{\"error\":\"serialize: {e}\"}}"))
                                        );
                                        continue;
                                    }
                                };
                                if let Some(event_name) = frame.event {
                                    Event::default().event(event_name).data(data_str)
                                } else {
                                    Event::default().data(data_str)
                                }
                            }
                        };
                        yield Ok::<Event, Infallible>(sse_event);
                    }
                }
                Err(e) => {
                    // Encode error → yield an error SSE frame and continue.
                    yield Ok::<Event, Infallible>(
                        Event::default()
                            .event("error")
                            .data(format!("{{\"error\":\"encode_stream_event: {e}\"}}"))
                    );
                }
            }
        }
    };

    Ok(Sse::new(output_stream).into_response())
}

// ---------------------------------------------------------------------------
// Unified dispatch — chooses streaming vs non-streaming
// ---------------------------------------------------------------------------

async fn handle(
    protocol: InboundProtocol,
    router: &AliasRouter,
    body: Value,
) -> axum::response::Response {
    // Peek at the body to determine whether streaming was requested.
    // We decode the `stream` field directly from the raw JSON rather than
    // calling decode_request twice (which would be fine but wasteful).
    let wants_stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

    if wants_stream {
        match handle_streaming(protocol, router, body).await {
            Ok(r) => r,
            Err(e) => e.into_response(),
        }
    } else {
        let adapter = protocol.as_adapter();
        match handle_non_streaming(adapter.as_ref(), router, body).await {
            Ok(r) => r,
            Err(e) => e.into_response(),
        }
    }
}

// ---------------------------------------------------------------------------
// Route state
// ---------------------------------------------------------------------------

/// Shared state threaded through all axum handlers.
#[derive(Clone)]
struct GatewayState {
    router: Arc<AliasRouter>,
}

// ---------------------------------------------------------------------------
// Individual route handlers
// ---------------------------------------------------------------------------

/// POST /v1/responses  →  OpenAI Responses API adapter
async fn handle_responses(
    State(state): State<GatewayState>,
    axum::Json(body): axum::Json<Value>,
) -> axum::response::Response {
    handle(InboundProtocol::Responses, &state.router, body).await
}

/// POST /v1/chat/completions  →  OpenAI Chat Completions adapter
async fn handle_chat(
    State(state): State<GatewayState>,
    axum::Json(body): axum::Json<Value>,
) -> axum::response::Response {
    handle(InboundProtocol::Chat, &state.router, body).await
}

/// POST /v1/messages  →  Anthropic Messages adapter
async fn handle_messages(
    State(state): State<GatewayState>,
    axum::Json(body): axum::Json<Value>,
) -> axum::response::Response {
    handle(InboundProtocol::Anthropic, &state.router, body).await
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Build the axum `Router` for the gateway.
///
/// Exposes three routes:
/// - `POST /v1/responses`         → OpenAI Responses API adapter
/// - `POST /v1/chat/completions`  → OpenAI Chat Completions adapter
/// - `POST /v1/messages`          → Anthropic Messages adapter
///
/// Wrap `router` in `Arc::new(router)` before calling this function, or use
/// the convenience wrapper [`serve`] for production use.
pub fn app(router: Arc<AliasRouter>) -> Router {
    let state = GatewayState { router };
    Router::new()
        .route("/v1/responses", post(handle_responses))
        .route("/v1/chat/completions", post(handle_chat))
        .route("/v1/messages", post(handle_messages))
        .with_state(state)
}

/// Start the gateway HTTP server, binding to `addr` (e.g. `"127.0.0.1:8787"`).
///
/// This function does not return unless the server fails to bind or the runtime
/// is shut down. For tests, use [`app`] directly and serve on an ephemeral port.
pub async fn serve(router: AliasRouter, addr: &str) -> Result<(), String> {
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    let router = Arc::new(router);
    axum::serve(listener, app(router))
        .await
        .map_err(|e| format!("server error: {e}"))
}
