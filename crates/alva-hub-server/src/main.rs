// INPUT:  alva-hub-core::HubRuntime, alva-engine-adapter-alva::{AlvaAdapter, DefaultBaseAgentFactory},
//         alva-kernel-abi::LanguageModel, alva-llm-provider, axum, tokio
// OUTPUT: headless HTTP service that hosts a multi-engine HubRuntime and streams session events over SSE
// POS:    Alva Hub as a local service — one backend, many frontends (web/TUI/desktop skins)

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::{get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::info;

use alva_engine_adapter_alva::{AlvaAdapter, DefaultBaseAgentFactory, DefaultBaseAgentFactoryConfig};
use alva_engine_runtime::RuntimeRequest;
use alva_hub_core::{HubEvent, HubRequest, HubRuntime, ProductSessionId};
use alva_kernel_abi::LanguageModel;
use alva_llm_provider::ProviderConfig;

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct AppState {
    hub: HubRuntime,
    /// Broadcast every HubEvent so SSE subscribers observe the same timeline.
    events: broadcast::Sender<HubEvent>,
}

impl AppState {
    fn new() -> Result<Self, String> {
        let (model, kind) = build_model_from_env()?;
        let workspace = std::env::current_dir().unwrap_or_default();
        let factory = Arc::new(DefaultBaseAgentFactory::new(
            DefaultBaseAgentFactoryConfig::new(model, workspace),
        ));
        let adapter = Arc::new(AlvaAdapter::new(factory));

        let hub = HubRuntime::new();
        hub.register_engine("alva", adapter).map_err(|e| e.to_string())?;

        // If the user pointed ALVA_MODEL_KIND at a non-openai provider, we can
        // only serve the "alva" engine id anyway (single-engine first pass).
        let _ = kind;

        let (events, _) = broadcast::channel(1024);

        Ok(Self { hub, events })
    }
}

/// Resolve a `LanguageModel` from the standard env (ALVA_API_KEY/ALVA_MODEL/
/// ALVA_BASE_URL/ALVA_PROVIDER_KIND via `ProviderConfig::from_env`).
///
/// `ProviderConfig::from_env` already resolves the provider kind from
/// `ALVA_PROVIDER_KIND` into `config.kind`; we pass that straight through to
/// `build_language_model` rather than reading a second, divergent variable.
///
/// Without an API key we return a reporting model so the server still boots
/// and every request produces a clear error instead of a crash.
fn build_model_from_env() -> Result<(Arc<dyn LanguageModel>, Option<String>), String> {
    match ProviderConfig::from_env() {
        Ok(config) => {
            let kind = config.kind.clone();
            let model = alva_llm_provider::build_language_model(kind.as_deref(), config);
            Ok((model, kind))
        }
        Err(_) => {
            // from_env errors when there is no api key (and no config files).
            // Return a reporting model so the server can boot and tell the
            // browser what to configure.
            Ok((Arc::new(MissingKeyModel), None))
        }
    }
}

// ---------------------------------------------------------------------------
// DTOs — serde wire shapes only; no engine internals cross the boundary.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RunRequestDto {
    prompt: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    resume_session: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
}

#[derive(Debug, Serialize)]
struct RunAcceptedDto {
    product_session_id: String,
}

#[derive(Debug, Serialize)]
struct SessionDto {
    product_session_id: String,
    engine_id: String,
    status: String,
    engine_session_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct WireEvent<'a> {
    product_session_id: &'a str,
    engine_id: &'a str,
    #[serde(flatten)]
    event: &'a alva_engine_runtime::RuntimeEvent,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Serve the embedded single-page UI at `/`.
async fn index() -> impl IntoResponse {
    (
        [("content-type", "text/html; charset=utf-8")],
        include_str!("index.html"),
    )
}

async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let engine_ids = state.hub.engine_ids();
    let engines: Vec<&str> = engine_ids.iter().map(|e| e.as_str()).collect();
    Json(serde_json::json!({
        "ok": true,
        "service": "alva-hub-server",
        "engines": engines,
    }))
}

async fn list_sessions(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sessions: Vec<SessionDto> = state
        .hub
        .sessions()
        .into_iter()
        .map(|s| SessionDto {
            product_session_id: s.product_session_id.as_str().to_string(),
            engine_id: s.engine_id.as_str().to_string(),
            status: format!("{:?}", s.status),
            engine_session_id: s.engine_session_id,
        })
        .collect();
    Json(sessions)
}

async fn run(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RunRequestDto>,
) -> impl IntoResponse {
    // A new session mints its own id up front so the HTTP response can carry
    // it; a resume reuses the client-supplied id. `HubRequest::resume` handles
    // both: for an unknown id `HubRuntime::execute` creates the session.
    let product_session_id = match body.resume_session.clone() {
        Some(rs) => ProductSessionId::from_existing(rs),
        None => ProductSessionId::new(),
    };

    let mut req = RuntimeRequest::new(body.prompt);
    if let Some(cwd) = body.cwd {
        req = req.with_cwd(cwd);
    }
    if let Some(sp) = body.system_prompt {
        req.system_prompt = Some(sp);
    }

    let hub_req = HubRequest::new("alva", req).resume(product_session_id.clone());

    match state.hub.execute(hub_req) {
        Ok(stream) => {
            let tx = state.events.clone();
            tokio::spawn(async move {
                let mut stream = stream;
                while let Some(ev) = stream.next().await {
                    if tx.send(ev).is_err() {
                        break; // no subscribers left
                    }
                }
            });
            (
                StatusCode::ACCEPTED,
                Json(RunAcceptedDto {
                    product_session_id: product_session_id.as_str().to_string(),
                }),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn events(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut rx = state.events.subscribe();
    let stream = async_stream::stream! {
        while let Ok(ev) = rx.recv().await {
            let wire = WireEvent {
                product_session_id: ev.product_session_id.as_str(),
                engine_id: ev.engine_id.as_str(),
                event: &ev.event,
            };
            match serde_json::to_string(&wire) {
                Ok(json) => {
                    yield Ok::<Event, std::convert::Infallible>(
                        Event::default().event("hub_event").data(json)
                    );
                }
                Err(_) => {}
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), String> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,alva_hub_server=info")),
        )
        .init();

    let state = Arc::new(AppState::new()?);

    let addr = std::env::var("ALVA_HUB_ADDR").unwrap_or_else(|_| "127.0.0.1:8790".to_string());
    let app = Router::new()
        .route("/", get(index))
        .route("/api/health", get(health))
        .route("/api/sessions", get(list_sessions))
        .route("/api/run", post(run))
        .route("/api/events", get(events))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    info!("alva-hub-server listening on http://{addr}");
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {e}"))
}

// ---------------------------------------------------------------------------
// Missing-key model — boots without credentials, reports a clear per-call error.
// ---------------------------------------------------------------------------

use alva_kernel_abi::{AgentError, CompletionResponse, Message, ModelConfig, StreamEvent};

struct MissingKeyModel;

#[async_trait::async_trait]
impl LanguageModel for MissingKeyModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn alva_kernel_abi::Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        Err(AgentError::ConfigError(
            "ALVA_API_KEY (and optionally ALVA_MODEL / ALVA_BASE_URL) is not set — nothing to run"
                .to_string(),
        ))
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn alva_kernel_abi::Tool],
        _config: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
        Box::pin(futures_util::stream::iter([
            StreamEvent::Error("ALVA_API_KEY is not set".to_string()),
            StreamEvent::Done,
        ]))
    }

    fn model_id(&self) -> &str {
        "missing-key"
    }

    fn provider_id(&self) -> &str {
        "env"
    }
}
