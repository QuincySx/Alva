// INPUT:  axum, serde_json, alva_llm_provider::AliasRouter, alva_llm_wire::adapter::{ProtocolAdapter, DecodedResponse},
//         alva_kernel_abi::tool::Tool, crate::raw_tool::RawTool
// OUTPUT: GatewayError, app(), serve()
// POS:    Axum HTTP server wiring. Three POST routes map inbound protocol adapters through AliasRouter
//         to upstream LanguageModel::complete, then re-encode the response. Streaming (Task 6.5) returns 501.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{Response, StatusCode};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Router;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use alva_kernel_abi::base::error::AgentError;
use alva_llm_provider::AliasRouter;
use alva_llm_wire::adapter::{AdapterError, DecodedResponse, ProtocolAdapter};
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
    /// Streaming was requested but is not yet implemented (HTTP 501, Task 6.5).
    NotImplemented(&'static str),
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
            GatewayError::NotImplemented(_) => StatusCode::NOT_IMPLEMENTED,
            GatewayError::Upstream(_) => StatusCode::BAD_GATEWAY,
            GatewayError::EncodeResponse(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn message(&self) -> String {
        match self {
            GatewayError::DecodeRequest(e) => format!("decode request: {e}"),
            GatewayError::UnknownModel(m) => format!("unknown model alias: {m}"),
            GatewayError::NotImplemented(feature) => {
                format!("{feature} not yet implemented")
            }
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
// Shared handler
// ---------------------------------------------------------------------------

/// Core request-handling pipeline shared by all three routes.
///
/// Flow:
/// 1. `inbound.decode_request(body)` → neutral `DecodedRequest`  (400 on error)
/// 2. Stream check → 501 if `req.stream`
/// 3. `router.resolve(model)` → `Arc<dyn LanguageModel>`          (404 if missing)
/// 4. Wrap `ToolDefinition`s in `RawTool` to satisfy `&[&dyn Tool]`
/// 5. `lm.complete(messages, tools, config).await`                 (502 on error)
/// 6. `inbound.encode_response(DecodedResponse)`                   (500 on error)
/// 7. Inject real model alias into the response JSON (`model` field was `""`)
/// 8. Return `200 application/json`
async fn handle(
    inbound: &(dyn ProtocolAdapter + Send + Sync),
    router: &AliasRouter,
    body: Value,
) -> Result<axum::response::Response, GatewayError> {
    // 1. Decode inbound request
    let req = inbound
        .decode_request(&body)
        .map_err(GatewayError::DecodeRequest)?;

    // 2. Streaming not yet implemented (Task 6.5)
    if req.stream {
        return Err(GatewayError::NotImplemented("streaming"));
    }

    // 3. Resolve the model alias
    let lm = router
        .resolve(&req.model)
        .ok_or_else(|| GatewayError::UnknownModel(req.model.clone()))?;

    // 4. Wrap tool definitions in passthrough RawTool wrappers
    let raw_tools: Vec<RawTool> = req
        .tools
        .iter()
        .map(|t| RawTool::new(t.name.clone(), t.description.clone(), t.parameters.clone()))
        .collect();
    let tool_refs: Vec<&dyn Tool> = raw_tools.iter().map(|t| t as &dyn Tool).collect();

    // 5. Call upstream
    let cr = lm
        .complete(&req.messages, &tool_refs, &req.config)
        .await
        .map_err(GatewayError::Upstream)?;

    // 6. Re-encode in the inbound protocol's format
    let dr = DecodedResponse {
        message: cr.message.clone(),
        usage: cr.message.usage.clone(),
    };
    let mut out = inbound
        .encode_response(&dr)
        .map_err(GatewayError::EncodeResponse)?;

    // 7. MODEL ECHO FIX: encode_response emits model:"" — inject the real alias
    if let Some(obj) = out.as_object_mut() {
        obj.insert("model".into(), Value::String(req.model.clone()));
    }

    // 8. Return JSON 200
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
    let adapter = OpenAIResponsesAdapter;
    match handle(&adapter, &state.router, body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

/// POST /v1/chat/completions  →  OpenAI Chat Completions adapter
async fn handle_chat(
    State(state): State<GatewayState>,
    axum::Json(body): axum::Json<Value>,
) -> axum::response::Response {
    let adapter = OpenAIChatAdapter;
    match handle(&adapter, &state.router, body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

/// POST /v1/messages  →  Anthropic Messages adapter
async fn handle_messages(
    State(state): State<GatewayState>,
    axum::Json(body): axum::Json<Value>,
) -> axum::response::Response {
    let adapter = AnthropicAdapter;
    match handle(&adapter, &state.router, body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
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
