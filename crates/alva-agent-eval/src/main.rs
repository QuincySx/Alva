//! alva-agent-eval — Lightweight agent eval playground.
//!
//! Spins up a local HTTP server with an embedded single-page UI.
//! The user picks a provider/model/tools, types a prompt, and watches
//! the full AgentEvent stream in real time via SSE.
//!
//! ```bash
//! cargo run -p alva-agent-eval
//! # open http://127.0.0.1:3000
//! ```

mod log_capture;
mod recorder;
mod skills;
mod store;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::stream::StreamExt;
use rust_embed::Embed;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio_stream::wrappers::UnboundedReceiverStream;

use alva_agent_core::builtins::{DanglingToolCallMiddleware, LoopDetectionMiddleware};
use alva_agent_core::event::AgentEvent;
use alva_agent_core::middleware::MiddlewareStack;
use alva_agent_core::run::run_agent;
use alva_agent_core::shared::Extensions;
use alva_agent_core::state::{AgentConfig, AgentState};
use alva_llm_provider::{AnthropicProvider, OpenAIChatProvider, OpenAIResponsesProvider, ProviderConfig};
use alva_types::session::{AgentSession, InMemorySession};
use alva_types::{
    AgentMessage, CancellationToken, LanguageModel, Message, ModelConfig, ToolRegistry,
};

// ---------------------------------------------------------------------------
// Embedded static assets
// ---------------------------------------------------------------------------

#[derive(Embed)]
#[folder = "static/"]
struct Assets;

/// Serve embedded static files. `GET /` → `index.html`.
async fn serve_static(path: Option<Path<String>>) -> impl IntoResponse {
    let path = path.map(|p| p.0).unwrap_or_else(|| "index.html".to_string());
    match Assets::get(&path) {
        Some(content) => {
            let mime = mime_guess::from_path(&path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data,
            )
                .into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

struct AppState {
    /// Pending event receivers keyed by run_id.
    runs: Mutex<HashMap<String, tokio::sync::mpsc::UnboundedReceiver<AgentEvent>>>,
    /// Session references for post-run message inspection.
    sessions: Mutex<HashMap<String, Arc<dyn AgentSession>>>,
    /// Captured tracing logs per run (in-memory, flushed to DB on completion).
    log_store: log_capture::LogStore,
    /// Persistent storage for completed runs.
    db: Arc<store::RunStore>,
}

// ---------------------------------------------------------------------------
// API types
// ---------------------------------------------------------------------------

#[derive(Deserialize, Clone)]
struct RunRequest {
    provider: String,
    #[serde(default)]
    api_key: Option<String>,
    model: String,
    #[serde(default)]
    base_url: Option<String>,
    system_prompt: String,
    user_prompt: String,
    /// If omitted, all builtin tools are registered.
    #[serde(default)]
    tools: Option<Vec<String>>,
    #[serde(default)]
    max_iterations: Option<u32>,
    /// Workspace directory for tools to operate on. If omitted, uses a temp dir.
    #[serde(default)]
    workspace: Option<String>,
    /// Custom headers (mutually exclusive with api_key).
    /// When non-empty, api_key is ignored and these headers are sent as-is.
    #[serde(default)]
    custom_headers: Option<std::collections::HashMap<String, String>>,
}

#[derive(Serialize)]
struct RunResponse {
    run_id: String,
    tools: Vec<String>,
}

#[derive(Serialize)]
struct ToolInfo {
    name: String,
    description: String,
}

#[derive(Deserialize)]
struct CompareRequest {
    run_a: RunRequest,
    run_b: RunRequest,
}

#[derive(Serialize)]
struct CompareResponse {
    run_id_a: String,
    run_id_b: String,
    tools_a: Vec<String>,
    tools_b: Vec<String>,
}

#[derive(Serialize)]
struct CompareResult {
    run_a: Option<recorder::RunRecord>,
    run_b: Option<recorder::RunRecord>,
    diff: CompareDiff,
}

#[derive(Serialize)]
struct CompareDiff {
    turns_a: usize,
    turns_b: usize,
    tokens_a: u64,
    tokens_b: u64,
    duration_a_ms: u64,
    duration_b_ms: u64,
    tool_calls_a: Vec<String>,
    tool_calls_b: Vec<String>,
    tools_only_a: Vec<String>,
    tools_only_b: Vec<String>,
}

// ---------------------------------------------------------------------------
// API routes
// ---------------------------------------------------------------------------

/// List all available builtin tools.
async fn list_tools() -> Json<Vec<ToolInfo>> {
    let mut registry = ToolRegistry::new();
    alva_agent_tools::register_builtin_tools(&mut registry);

    let tools = registry
        .definitions()
        .into_iter()
        .map(|d| ToolInfo {
            name: d.name,
            description: d.description,
        })
        .collect();

    Json(tools)
}

/// Internal: create an agent run and return (run_id, tool_names).
///
/// Builds the provider, tools, middleware, recorder, and spawns the agent task.
/// Stores the event receiver in `state.runs` and the session in `state.sessions`.
async fn create_run(
    state: &Arc<AppState>,
    req: RunRequest,
) -> Result<(String, Vec<String>), String> {
    let run_id = uuid::Uuid::new_v4().to_string();

    // -- 0. Normalize & validate inputs ------------------------------------
    let provider = req.provider.trim().to_string();
    let model = req.model.trim().to_string();
    let api_key = req.api_key.map(|k| k.trim().to_string()).unwrap_or_default();
    let base_url = req.base_url.map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
    let custom_headers = req.custom_headers.unwrap_or_default()
        .into_iter()
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, v)| !k.is_empty() && !v.is_empty())
        .collect::<std::collections::HashMap<_, _>>();

    if model.is_empty() {
        return Err("model is required".to_string());
    }

    // -- 1. Build provider --------------------------------------------------
    let provider_config = ProviderConfig {
        api_key,
        model,
        base_url: base_url
            .map(|u| alva_llm_provider::normalize_base_url(&u))
            .unwrap_or_else(|| match provider.as_str() {
                "anthropic" => "https://api.anthropic.com".to_string(),
                "openai-responses" => "https://api.openai.com".to_string(),
                _ => "https://api.openai.com/v1".to_string(),
            }),
        max_tokens: 8192,
        custom_headers,
    };

    let model: Arc<dyn LanguageModel> = match provider.as_str() {
        "anthropic" => Arc::new(AnthropicProvider::new(provider_config)),
        "openai-responses" => Arc::new(OpenAIResponsesProvider::new(provider_config)),
        _ => Arc::new(OpenAIChatProvider::new(provider_config)),
    };

    // -- 2. Build tool set --------------------------------------------------
    let mut registry = ToolRegistry::new();
    alva_agent_tools::register_builtin_tools(&mut registry);

    let mut tools: Vec<Arc<dyn alva_types::Tool>> = match &req.tools {
        None => registry.list_arc(),
        Some(names) => registry
            .list_arc()
            .into_iter()
            .filter(|t| names.contains(&t.name().to_string()))
            .collect(),
    };

    // Replace placeholder AgentTool with real AgentSpawnTool if user selected "agent"
    let has_agent_tool = tools.iter().any(|t| t.name() == "agent");
    if has_agent_tool {
        // Remove placeholder
        tools.retain(|t| t.name() != "agent");
        // Will be replaced with real spawn tool after model is set up (see below)
    }

    let tool_names: Vec<String> = tools.iter().map(|t| t.name().to_string()).collect();

    // -- 3. Resolve workspace ------------------------------------------------
    let (_tmp_guard, workspace_path) = if let Some(ref ws) = req.workspace {
        let p = std::path::PathBuf::from(ws);
        if !p.is_dir() {
            return Err(format!("workspace does not exist: {ws}"));
        }
        (None, p)
    } else {
        let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
        let p = tmp.path().to_path_buf();
        (Some(tmp), p)
    };

    // -- 3b. Wire real sub-agent support if requested -------------------------
    let max_iterations = req.max_iterations.unwrap_or(10);
    if has_agent_tool {
        let root_scope = Arc::new(alva_agent_scope::SpawnScopeImpl::root(
            model.clone(),
            tools.clone(),
            std::time::Duration::from_secs(300),
            max_iterations,
            3, // max sub-agent depth
        ));
        let spawn_tool = alva_app_core::plugins::agent_spawn::create_agent_spawn_tool(root_scope);
        tools.push(Arc::from(spawn_tool));
    }

    // -- 4. Assemble agent state + config -----------------------------------
    let session: Arc<dyn AgentSession> = Arc::new(InMemorySession::new());
    let session_ref = session.clone();

    let agent_state = AgentState {
        model,
        tools,
        session,
        extensions: Extensions::new(),
    };

    let mut middleware = MiddlewareStack::new();
    middleware.push_sorted(Arc::new(LoopDetectionMiddleware::new()));
    middleware.push_sorted(Arc::new(DanglingToolCallMiddleware::new()));

    let rec = Arc::new(recorder::RecorderMiddleware::new());
    middleware.push_sorted(rec.clone());

    let system_prompt = req.system_prompt;

    // Pre-fill config fields the middleware can't access via AgentState
    rec.set_config(system_prompt.clone(), max_iterations, vec![]);

    let agent_config = AgentConfig {
        middleware,
        system_prompt,
        max_iterations,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: Some(workspace_path),
        bus: None,
    };

    // -- 5. Spawn the agent run ---------------------------------------------
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let cancel = CancellationToken::new();
    let messages = vec![AgentMessage::Standard(Message::user(&req.user_prompt))];

    let rec_clone = rec.clone();
    let state_clone = state.clone();
    let run_id_for_record = run_id.clone();
    let log_store = state.log_store.clone();

    // Start capturing tracing events for this run
    log_store.start_capture(&run_id);

    tokio::spawn(async move {
        let _tmp_guard = _tmp_guard; // keep TempDir alive if used
        let mut st = agent_state;
        if let Err(e) = run_agent(&mut st, &agent_config, cancel, messages, tx).await {
            tracing::error!(error = %e, "agent run failed");
        }

        // Stop capturing and extract record
        log_store.stop_capture();
        let record = rec_clone.take_record();
        let logs = log_store.get_logs(&run_id_for_record);

        // Persist to SQLite
        state_clone.db.save(&run_id_for_record, &record, &logs);
    });

    // -- 6. Store handles ---------------------------------------------------
    state.runs.lock().await.insert(run_id.clone(), rx);
    state
        .sessions
        .lock()
        .await
        .insert(run_id.clone(), session_ref);

    Ok((run_id, tool_names))
}

/// Start an agent run and return the run_id.
/// The caller should then connect to GET /api/events/:run_id for the SSE stream.
async fn start_run(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RunRequest>,
) -> Result<Json<RunResponse>, (StatusCode, String)> {
    let (run_id, tools) = create_run(&state, req)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(RunResponse { run_id, tools }))
}

/// SSE event stream for a running agent.
async fn events(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let rx = state.runs.lock().await.remove(&run_id);

    match rx {
        Some(rx) => {
            let stream = UnboundedReceiverStream::new(rx).map(|event| {
                let json = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
                Ok::<_, std::convert::Infallible>(Event::default().data(json))
            });
            Sse::new(stream)
                .keep_alive(KeepAlive::default())
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, format!("run {run_id} not found")).into_response(),
    }
}

/// Retrieve session messages after a run completes.
async fn get_messages(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    let sessions = state.sessions.lock().await;
    match sessions.get(&run_id) {
        Some(session) => Json(session.messages()).into_response(),
        None => (StatusCode::NOT_FOUND, "run not found").into_response(),
    }
}

/// Retrieve the full run record for a completed run.
async fn get_record(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    match state.db.get_record(&run_id) {
        Some(record) => Json(record).into_response(),
        None => (StatusCode::NOT_FOUND, "record not found or still running").into_response(),
    }
}

/// List summaries of all completed runs (from DB).
async fn list_runs(State(state): State<Arc<AppState>>) -> Json<Vec<store::StoredRunSummary>> {
    Json(state.db.list())
}

// ---------------------------------------------------------------------------
// Compare endpoints
// ---------------------------------------------------------------------------

/// Start two agent runs concurrently for comparison.
async fn start_compare(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CompareRequest>,
) -> Result<Json<CompareResponse>, (StatusCode, String)> {
    let (run_id_a, tools_a) = create_run(&state, req.run_a)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let (run_id_b, tools_b) = create_run(&state, req.run_b)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(CompareResponse {
        run_id_a,
        run_id_b,
        tools_a,
        tools_b,
    }))
}

/// Retrieve records and diff summary for a pair of runs.
///
/// If a run has not yet completed its record will be `None` — the frontend
/// can poll until both are available.
async fn get_compare(
    State(state): State<Arc<AppState>>,
    Path((id_a, id_b)): Path<(String, String)>,
) -> impl IntoResponse {
    let rec_a = state.db.get_record(&id_a);
    let rec_b = state.db.get_record(&id_b);

    let diff = build_compare_diff(rec_a.as_ref(), rec_b.as_ref());

    Json(CompareResult {
        run_a: rec_a,
        run_b: rec_b,
        diff,
    })
    .into_response()
}

/// Build a diff summary from two (possibly absent) run records.
fn build_compare_diff(
    a: Option<&recorder::RunRecord>,
    b: Option<&recorder::RunRecord>,
) -> CompareDiff {
    let extract_tool_calls = |rec: Option<&recorder::RunRecord>| -> Vec<String> {
        rec.map(|r| {
            r.turns
                .iter()
                .flat_map(|t| t.tool_calls.iter().map(|tc| tc.tool_call.name.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
    };

    let tool_calls_a = extract_tool_calls(a);
    let tool_calls_b = extract_tool_calls(b);

    let set_a: HashSet<&str> = tool_calls_a.iter().map(|s| s.as_str()).collect();
    let set_b: HashSet<&str> = tool_calls_b.iter().map(|s| s.as_str()).collect();

    let tools_only_a: Vec<String> = set_a.difference(&set_b).map(|s| s.to_string()).collect();
    let tools_only_b: Vec<String> = set_b.difference(&set_a).map(|s| s.to_string()).collect();

    CompareDiff {
        turns_a: a.map(|r| r.turns.len()).unwrap_or(0),
        turns_b: b.map(|r| r.turns.len()).unwrap_or(0),
        tokens_a: a
            .map(|r| r.total_input_tokens + r.total_output_tokens)
            .unwrap_or(0),
        tokens_b: b
            .map(|r| r.total_input_tokens + r.total_output_tokens)
            .unwrap_or(0),
        duration_a_ms: a.map(|r| r.total_duration_ms).unwrap_or(0),
        duration_b_ms: b.map(|r| r.total_duration_ms).unwrap_or(0),
        tool_calls_a,
        tool_calls_b,
        tools_only_a,
        tools_only_b,
    }
}

// ---------------------------------------------------------------------------
// Run logs (captured tracing events)
// ---------------------------------------------------------------------------

/// Delete a run record from the DB.
async fn delete_run(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> impl IntoResponse {
    if state.db.delete(&run_id) {
        (StatusCode::OK, "deleted").into_response()
    } else {
        (StatusCode::NOT_FOUND, "run not found").into_response()
    }
}

/// Get captured tracing logs for a run (request/response bodies, tool timing, etc.)
/// Checks in-memory first (active run), then falls back to DB.
async fn get_logs(
    State(state): State<Arc<AppState>>,
    Path(run_id): Path<String>,
) -> Json<Vec<log_capture::LogEntry>> {
    let in_memory = state.log_store.get_logs(&run_id);
    if !in_memory.is_empty() {
        return Json(in_memory);
    }
    Json(state.db.get_logs(&run_id))
}

// ---------------------------------------------------------------------------
// Skill discovery
// ---------------------------------------------------------------------------

/// List standard skill source directories.
async fn list_skill_sources() -> Json<Vec<skills::SkillSourceInfo>> {
    Json(skills::discover_skill_sources())
}

#[derive(Deserialize)]
struct ScanSkillsRequest {
    path: String,
}

/// Scan a skill directory and return all found skills.
async fn scan_skills_handler(Json(req): Json<ScanSkillsRequest>) -> Json<Vec<skills::SkillInfo>> {
    Json(skills::scan_skills(std::path::Path::new(&req.path)).await)
}

// ---------------------------------------------------------------------------
// Directory browser
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct BrowseRequest {
    /// Directory to list. If omitted, lists home directory.
    #[serde(default)]
    path: Option<String>,
}

#[derive(Serialize)]
struct BrowseEntry {
    name: String,
    path: String,
    is_dir: bool,
}

#[derive(Serialize)]
struct BrowseResponse {
    current: String,
    parent: Option<String>,
    entries: Vec<BrowseEntry>,
}

/// Browse local directories so the user can pick a workspace.
async fn browse_dir(Json(req): Json<BrowseRequest>) -> Result<Json<BrowseResponse>, (StatusCode, String)> {
    let dir = match req.path {
        Some(p) => std::path::PathBuf::from(p),
        None => dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/")),
    };

    if !dir.is_dir() {
        return Err((StatusCode::BAD_REQUEST, format!("{} is not a directory", dir.display())));
    }

    let parent = dir.parent().map(|p| p.to_string_lossy().to_string());

    let mut entries = Vec::new();
    if let Ok(read) = std::fs::read_dir(&dir) {
        for entry in read.flatten() {
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip hidden files unless they are common project dirs
            if name.starts_with('.') && !matches!(name.as_str(), ".git" | ".alva" | ".claude") {
                continue;
            }
            entries.push(BrowseEntry {
                name,
                path: entry.path().to_string_lossy().to_string(),
                is_dir,
            });
        }
    }

    // Sort: dirs first, then by name
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));

    Ok(Json(BrowseResponse {
        current: dir.to_string_lossy().to_string(),
        parent,
        entries,
    }))
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    // Log capture: captures tracing events from providers/agent-core per run
    let log_store = log_capture::LogStore::new();

    // Tracing subscriber: terminal output + log capture layer
    // The capture layer intercepts events from alva_llm_provider and alva_agent_core,
    // buffering them per run_id for the web UI.
    // Terminal output defaults to warn; override with RUST_LOG=info or RUST_LOG=debug.
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(
            "warn,alva_llm_provider=debug,alva_agent_core=info"
        ));

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    tracing_subscriber::registry()
        .with(env_filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(log_capture::LogCaptureLayer::new(log_store.clone()))
        .init();

    let db = Arc::new(store::RunStore::open("alva-eval-runs.db"));

    let state = Arc::new(AppState {
        runs: Mutex::new(HashMap::new()),
        sessions: Mutex::new(HashMap::new()),
        log_store: log_store.clone(),
        db,
    });

    let app = Router::new()
        // API routes
        .route("/api/tools", get(list_tools))
        .route("/api/run", post(start_run))
        .route("/api/events/:run_id", get(events))
        .route("/api/messages/:run_id", get(get_messages))
        .route("/api/runs", get(list_runs))
        .route("/api/runs/:run_id", axum::routing::delete(delete_run))
        .route("/api/records/:run_id", get(get_record))
        .route("/api/logs/:run_id", get(get_logs))
        .route("/api/compare", post(start_compare))
        .route("/api/compare/:id_a/:id_b", get(get_compare))
        // Directory browser
        .route("/api/browse", post(browse_dir))
        // Skill discovery
        .route("/api/skills/sources", get(list_skill_sources))
        .route("/api/skills/scan", post(scan_skills_handler))
        // Static assets (index.html, style.css, app.js, ...)
        .route("/", get(|| serve_static(None)))
        .route("/*path", get(serve_static))
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("127.0.0.1:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();

    println!("alva-agent-eval running at http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
