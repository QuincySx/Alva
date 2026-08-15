// INPUT:  alva-hub-desktop (HubStore, ProviderCatalog, run_persisted_with_target,
//         build_engine_for_target, SystemProbe, DTOs), axum, tokio
// OUTPUT: headless HTTP service: reuses the Hub Desktop business library over
//         HTTP (uplink) + SSE (downlink) so the shipped React UI can run in a browser.
// POS:    Alva Hub as a local service — one backend, many frontends.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{get, post},
    Json, Router,
};
use include_dir::{include_dir, Dir};
use serde_json::json;
use tokio::sync::broadcast;
use tracing::{info, warn};

use alva_hub_desktop::{
    build_engine_for_target, run_persisted_with_target, EngineTarget, HubStore, ProviderCatalog,
    RunRequest, SystemProbe, TimelineEvent,
};
use alva_llm_provider::ProviderConfig;

/// The built React UI (`alva-hub-desktop/web/dist`), embedded at compile time
/// so this one binary is a complete local service: API + static frontend.
static DIST: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../alva-hub-desktop/web/dist");

// ---------------------------------------------------------------------------
// App state — mirrors the Tauri DesktopState but without the Tauri AppHandle.
// ---------------------------------------------------------------------------

struct AppState {
    store: HubStore,
    providers: ProviderCatalog,
    history_dir: PathBuf,
    /// Timeline broadcast → SSE subscribers.
    events: broadcast::Sender<TimelineEvent>,
}

impl AppState {
    fn new(data_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("create data dir {}: {e}", data_dir.display()))?;

        let store = HubStore::open(data_dir.join("alva-hub.db"))?;
        let shared_config = dirs_home().join(".alva").join("config.json");
        let providers = ProviderCatalog::open_with_shared_and_legacy_config(
            data_dir.join("model-providers.json"),
            shared_config,
        )?;

        let (events, _) = broadcast::channel(1024);

        Ok(Self {
            store,
            providers,
            history_dir: data_dir.join("alva-history"),
            events,
        })
    }
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

// ---------------------------------------------------------------------------
// DTOs / wire shapes
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
struct RunBody {
    prompt: String,
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    engine_id: Option<String>,
    #[serde(default)]
    product_session_id: Option<String>,
}

/// Flattened timeline event for the browser, matching the Tauri
/// `hub_timeline` payload so the shipped React UI consumes it unchanged.
#[derive(Debug, serde::Serialize, Clone)]
struct WireTimelineEvent {
    #[serde(flatten)]
    event: TimelineEvent,
    run_token: Option<String>,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

async fn health(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let sessions = state.store.list_sessions().map(|s| s.len()).unwrap_or(0);
    Json(json!({
        "ok": true,
        "service": "alva-hub-server",
        "sessions": sessions,
    }))
}

/// GET /api/sessions — durable Hub session list (same shape as the Tauri
/// `list_hub_sessions` command).
async fn list_sessions(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.store.list_sessions() {
        Ok(sessions) => Json(sessions).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e }))).into_response(),
    }
}

/// GET /api/providers — model providers for the composer.
async fn list_providers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let workspace: Option<&std::path::Path> = None;
    match state.providers.routes(workspace) {
        Ok(routes) => Json(json!({ "routes": routes })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e }))).into_response(),
    }
}

/// POST /api/run — execute one product turn, streaming timeline to SSE.
async fn run(
    State(state): State<Arc<AppState>>,
    Json(body): Json<RunBody>,
) -> impl IntoResponse {
    let workspace = match body.workspace {
        Some(w) => PathBuf::from(w),
        None => std::env::current_dir().unwrap_or_default(),
    };

    let request = RunRequest {
        prompt: body.prompt,
        workspace: workspace.clone(),
        engine_id: body.engine_id.unwrap_or_else(|| "alva".to_string()),
        product_session_id: body.product_session_id,
        permission_profile: None,
    };

    let selected_target: Option<EngineTarget> = None;
    let run_token: Option<String> = None;

    // Clones captured into the async closures.
    let providers = state.providers.clone();
    let history_dir = state.history_dir.clone();
    let active_engines = Arc::new(Mutex::new(std::collections::HashMap::new()));
    let health_store = state.store.clone();
    let health_workspace = workspace.clone();

    let tx = state.events.clone();

    // Spawn the (potentially long) run on the runtime; emit each timeline
    // event to the broadcast channel.
    let outcome_fut = run_persisted_with_target(
        &state.store,
        {
            let providers = providers.clone();
            let workspace = workspace.clone();
            move |target, product_session_id, profile| {
                let provider_config = providers
                    .resolve_target(target, &workspace)?
                    .or_else(|| {
                        // fall back to env-based config for the "alva" engine
                        // when no provider catalog entry exists yet.
                        if target.engine_id == "alva" {
                            ProviderConfig::from_env().ok()
                        } else {
                            None
                        }
                    });
                let engine = build_engine_for_target(
                    target,
                    provider_config,
                    profile,
                    &workspace,
                    &history_dir,
                )?;
                active_engines
                    .lock()
                    .unwrap()
                    .insert(product_session_id.to_string(), Arc::clone(&engine));
                Ok(engine)
            }
        },
        {
            move |target| {
                // Fast veto: skip targets with missing binaries / signed-out
                // engines. Uses the same SystemProbe as the Tauri product.
                SystemProbe::new(Some(&health_store), Some(health_workspace.clone()))
                    .quick_veto_for_target(&target.engine_id, false)
            }
        },
        request,
        selected_target,
        {
            let tx = tx.clone();
            move |event: TimelineEvent| {
                let wire = WireTimelineEvent {
                    event,
                    run_token: run_token.clone(),
                };
                let _ = tx.send(wire.event.clone());
                // keep wire in scope for potential future per-client fan-out
                let _ = wire;
            }
        },
    );

    match outcome_fut.await {
        Ok(outcome) => Json(outcome).into_response(),
        Err(e) => {
            warn!(error = %e, "run failed");
            (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response()
        }
    }
}

/// GET /api/events — SSE stream of TimelineEvent, same payload shape as the
/// Tauri `hub_timeline` event so the React UI's listener works unchanged.
async fn events(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut rx = state.events.subscribe();
    let stream = async_stream::stream! {
        while let Ok(ev) = rx.recv().await {
            let wire = WireTimelineEvent { event: ev, run_token: None };
            match serde_json::to_string(&wire) {
                Ok(json) => {
                    yield Ok::<Event, std::convert::Infallible>(
                        Event::default().event("hub_timeline").data(json)
                    );
                }
                Err(_) => {}
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ---------------------------------------------------------------------------
// Static frontend (embedded dist)
// ---------------------------------------------------------------------------

/// Serve the embedded React UI. `/` and any non-API path fall back to
/// `index.html` (SPA routing); `/assets/*` serve the hashed bundle files.
async fn static_handler(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    if let Some(file) = DIST.get_file(path) {
        let mime = mime_for(path);
        return (
            [(header::CONTENT_TYPE, mime)],
            file.contents(),
        )
            .into_response();
    }

    // SPA fallback: any unknown non-asset path renders the shell.
    if let Some(index) = DIST.get_file("index.html") {
        return (
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            index.contents(),
        )
            .into_response();
    }

    (StatusCode::NOT_FOUND, "not found").into_response()
}

fn mime_for(path: &str) -> &'static str {
    if path.ends_with(".js") {
        "application/javascript"
    } else if path.ends_with(".css") {
        "text/css"
    } else if path.ends_with(".svg") {
        "image/svg+xml"
    } else if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else {
        "application/octet-stream"
    }
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

    let data_dir = std::env::var("ALVA_HUB_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| dirs_home().join(".alva").join("hub-server"));
    let state = Arc::new(AppState::new(data_dir)?);

    let addr = std::env::var("ALVA_HUB_ADDR").unwrap_or_else(|_| "127.0.0.1:8790".to_string());
    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/sessions", get(list_sessions))
        .route("/api/providers", get(list_providers))
        .route("/api/run", post(run))
        .route("/api/events", get(events))
        .fallback(static_handler)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    info!("alva-hub-server listening on http://{addr}");
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {e}"))
}
