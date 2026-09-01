// INPUT:  alva-hub-desktop (HubStore, ProviderCatalog, run_persisted_with_target,
//         build_engine_for_target, SystemProbe, DTOs), axum, tokio
// OUTPUT: headless HTTP service: reuses Hub sessions, engines, Inspector, and timeline over HTTP/SSE
// POS:    Alva Hub as a local service — one backend, many frontends.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, Query, State},
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
    build_engine_for_target_with_inspector, permission_eligibility, probe_readiness,
    run_persisted_with_target, EngineTarget, HubInspectorService, HubStore, PermissionProfile,
    ProviderCatalog, RunRequest, SaveProviderProfile, SystemProbe, TimelineEvent,
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
    inspector: HubInspectorService,
    /// Timeline broadcast → SSE subscribers (wire shape with run_token echo).
    events: broadcast::Sender<WireTimelineEvent>,
    /// Live engines so Stop/permission routing can reach the running adapter.
    active_engines: std::sync::Arc<
        Mutex<std::collections::HashMap<String, Arc<dyn alva_engine_runtime::EngineRuntime>>>,
    >,
    permission_router: Arc<alva_hub_desktop::PermissionRouter>,
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

        let history_dir = data_dir.join("alva-history");
        let inspector = HubInspectorService::open(&history_dir)?;
        Ok(Self {
            store,
            providers,
            history_dir,
            inspector,
            events,
            active_engines: std::sync::Arc::new(Mutex::new(std::collections::HashMap::new())),
            permission_router: Arc::new(alva_hub_desktop::PermissionRouter::new()),
        })
    }
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

fn timeline_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
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
    #[serde(default)]
    selected_target: Option<EngineTarget>,
    #[serde(default)]
    permission_profile: Option<PermissionProfile>,
    #[serde(default)]
    run_token: Option<String>,
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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
            .into_response(),
    }
}

/// GET /api/sessions/:id — one durable session detail (same shape as the
/// Tauri `get_hub_session` command).
async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.store.get_session(&id) {
        Ok(Some(session)) => Json(session).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "session not found" })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
            .into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct InspectionQuery {
    native_session_id: Option<String>,
}

/// GET /api/sessions/:id/inspection — stable Hub Inspector projection over
/// the authoritative Alva native record repository.
async fn get_inspection(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(query): Query<InspectionQuery>,
) -> impl IntoResponse {
    match state
        .inspector
        .inspect(&state.store, &id, query.native_session_id.as_deref())
        .await
    {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(error) => (StatusCode::NOT_FOUND, Json(json!({ "error": error }))).into_response(),
    }
}

/// GET /api/providers — provider profiles (same shape as Tauri
/// `list_model_providers`).
async fn list_providers(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.providers.list(None) {
        Ok(providers) => Json(providers).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
            .into_response(),
    }
}

/// POST /api/providers — save a provider profile (same shape as Tauri
/// `save_model_provider`). Accepts an optional API key; `clear_api_key`
/// empties the stored key.
async fn save_provider(
    State(state): State<Arc<AppState>>,
    Json(provider): Json<SaveProviderProfile>,
) -> impl IntoResponse {
    match state.providers.save(provider, None) {
        Ok(saved) => Json(saved).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

/// GET /api/routes — effective model routes (same shape as Tauri
/// `list_model_routes`).
async fn list_routes(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.providers.routes(None) {
        Ok(routes) => Json(routes).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
            .into_response(),
    }
}

/// GET /api/default-route — the persisted default model route, if set.
async fn get_default_route(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.providers.default_target(None) {
        Ok(route) => Json(route).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
            .into_response(),
    }
}

/// POST /api/default-route — persist the global default model route.
async fn set_default_route(
    State(state): State<Arc<AppState>>,
    Json(target): Json<EngineTarget>,
) -> impl IntoResponse {
    match state.providers.set_default_target(target, None) {
        Ok(route) => Json(route).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

async fn get_global_fallback_policy(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    match state.store.global_fallback_policy() {
        Ok(policy) => Json(policy).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
            .into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct FallbackBody {
    targets: Vec<EngineTarget>,
}

async fn set_global_fallback_policy(
    State(state): State<Arc<AppState>>,
    Json(body): Json<FallbackBody>,
) -> impl IntoResponse {
    match state.store.set_global_fallback_policy(body.targets) {
        Ok(policy) => Json(policy).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct SessionFallbackBody {
    targets: Vec<EngineTarget>,
}

async fn set_session_fallback_policy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<SessionFallbackBody>,
) -> impl IntoResponse {
    match state.store.set_session_fallback_policy(&id, body.targets) {
        Ok(policy) => Json(policy).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

async fn get_session_fallback_policy(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.store.get_session(&id) {
        Ok(Some(detail)) => Json(detail.fallback_policy).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "session not found" })),
        )
            .into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct ReadinessQuery {
    workspace: Option<String>,
}

async fn get_readiness(
    State(state): State<Arc<AppState>>,
    axum::extract::Query(query): axum::extract::Query<ReadinessQuery>,
) -> impl IntoResponse {
    let workspace = query.workspace.map(PathBuf::from);
    let probe = match (|| -> Result<SystemProbe, String> {
        let mut provider_engines = std::collections::HashSet::new();
        for route in state.providers.routes(workspace.as_deref())? {
            if route.available
                && state
                    .providers
                    .target_uses_api(&route.target, workspace.as_deref())?
            {
                provider_engines.insert(route.target.engine_id);
            }
        }
        let mut p = SystemProbe::new(Some(&state.store), workspace.clone());
        for engine_id in provider_engines {
            p = p.with_provider_authenticated(engine_id, true);
        }
        Ok(p)
    })() {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
                .into_response()
        }
    };
    let readiness = probe_readiness(&probe);
    Json(readiness).into_response()
}

#[derive(Debug, serde::Deserialize)]
struct EligibilityQuery {
    profile: String,
}

async fn get_permission_eligibility(
    axum::extract::Query(query): axum::extract::Query<EligibilityQuery>,
) -> impl IntoResponse {
    let profile = match query.profile.as_str() {
        "interactive" => PermissionProfile::Interactive,
        "workspace_write" => PermissionProfile::WorkspaceWrite,
        "unrestricted" => PermissionProfile::Unrestricted,
        other => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("unknown profile {other}") })),
            )
                .into_response()
        }
    };
    Json(permission_eligibility(profile)).into_response()
}

#[derive(Debug, serde::Deserialize)]
struct SwitchBody {
    target: EngineTarget,
    reason: String,
}

async fn switch_engine(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<SwitchBody>,
) -> impl IntoResponse {
    // canonical route check
    let session = match state.store.get_session(&id) {
        Ok(Some(d)) => d,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "session not found" })),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
                .into_response()
        }
    };
    let workspace = PathBuf::from(session.session.workspace);
    let canonical = match canonical_available_route(
        &state.store,
        &state.providers,
        Some(&workspace),
        &body.target,
    ) {
        Ok(r) => r.target,
        Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    };
    match state
        .store
        .request_engine_switch(&id, &canonical, &body.reason)
    {
        Ok(outcome) => Json(outcome).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct PermissionRespondBody {
    request_id: String,
    allow: bool,
    message: Option<String>,
}

async fn respond_permission(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<PermissionRespondBody>,
) -> impl IntoResponse {
    let decision = if body.allow {
        alva_engine_runtime::PermissionDecision::Allow {
            updated_input: None,
        }
    } else {
        alva_engine_runtime::PermissionDecision::Deny {
            message: body.message.unwrap_or_else(|| "Denied".into()),
        }
    };
    match state
        .permission_router
        .respond(&id, &body.request_id, decision)
        .await
    {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

async fn get_permission_groups(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(state.permission_router.groups()).into_response()
}

#[derive(Debug, serde::Deserialize)]
struct GroupRespondBody {
    group_key: String,
    allow: bool,
    request_ids: Option<Vec<String>>,
}

async fn respond_permission_group(
    State(state): State<Arc<AppState>>,
    Json(body): Json<GroupRespondBody>,
) -> impl IntoResponse {
    let mut members = state.permission_router.group_members(&body.group_key);
    if let Some(selected) = &body.request_ids {
        members.retain(|m| selected.contains(&m.request_id));
    }
    if members.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "no pending requests" })),
        )
            .into_response();
    }
    match state
        .permission_router
        .respond_many(&members, body.allow, None)
        .await
    {
        Ok(outcome) => {
            if !outcome.failures.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": outcome.failures.join("; ") })),
                )
                    .into_response();
            }
            Json(json!({ "answered": outcome.answered.len() })).into_response()
        }
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct MemoryBody {
    entry: String,
}

async fn add_memory(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<MemoryBody>,
) -> impl IntoResponse {
    match state.store.add_session_memory(&id, &body.entry) {
        Ok(mem) => Json(mem).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct TranscriptBody {
    enabled: bool,
}

async fn set_transcript(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<TranscriptBody>,
) -> impl IntoResponse {
    match state.store.set_transcript_sharing(&id, body.enabled) {
        Ok(()) => Json(json!({ "ok": true })).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

async fn stop_attempt(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let detail = match state.store.get_session(&id) {
        Ok(Some(d)) => d,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "session not found" })),
            )
                .into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
                .into_response()
        }
    };
    if detail.session.status != "running" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "no running attempt" })),
        )
            .into_response();
    }
    let engine = match state.active_engines.lock().unwrap().get(&id).cloned() {
        Some(e) => e,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "no live engine" })),
            )
                .into_response()
        }
    };
    let engine_session_id = match detail.session.engine_session_id {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "no engine session" })),
            )
                .into_response()
        }
    };
    if let Err(e) = state.store.record_attempt_cancelled(&id, "User stopped") {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e })),
        )
            .into_response();
    }
    state.permission_router.clear_session(&id);
    match engine.cancel(&engine_session_id).await {
        Ok(()) => {
            let _ = state.events.send(WireTimelineEvent {
                event: TimelineEvent {
                    created_at: timeline_now_ms(),
                    status: alva_hub_desktop::TimelineStatus::Interrupted,
                    engine_id: detail.session.engine_id,
                    product_session_id: id,
                    message: Some("User stopped".into()),
                    item_id: None,
                    activity: None,
                    permission_request: None,
                },
                run_token: None,
            });
            Json(json!({ "ok": true })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

fn effective_model_routes(
    store: &HubStore,
    providers: &ProviderCatalog,
    workspace: Option<PathBuf>,
) -> Result<Vec<alva_hub_desktop::ModelRoute>, String> {
    let mut routes = providers.routes(workspace.as_deref())?;
    let probe = {
        let mut provider_engines = std::collections::HashSet::new();
        for route in &routes {
            if route.available && providers.target_uses_api(&route.target, workspace.as_deref())? {
                provider_engines.insert(route.target.engine_id.clone());
            }
        }
        let mut p = SystemProbe::new(Some(store), workspace.clone());
        for engine_id in provider_engines {
            p = p.with_provider_authenticated(engine_id, true);
        }
        p
    };
    for route in &mut routes {
        if !route.available {
            continue;
        }
        let uses_api = providers.target_uses_api(&route.target, workspace.as_deref())?;
        if let Some(reason) = probe.quick_veto_for_target(&route.target.engine_id, uses_api) {
            route.available = false;
            route.unavailable_reason = Some(reason);
        }
    }
    Ok(routes)
}

fn canonical_available_route(
    store: &HubStore,
    providers: &ProviderCatalog,
    workspace: Option<&std::path::Path>,
    requested: &EngineTarget,
) -> Result<alva_hub_desktop::ModelRoute, String> {
    let mut route = effective_model_routes(store, providers, workspace.map(|p| p.to_path_buf()))?
        .into_iter()
        .find(|r| {
            r.target.engine_id == requested.engine_id
                && r.target.provider_id == requested.provider_id
                && r.target.model_id == requested.model_id
        })
        .ok_or_else(|| {
            format!(
                "route '{}/{}/{}' is not configured",
                requested.engine_id, requested.provider_id, requested.model_id
            )
        })?;
    if !route.available {
        return Err(route
            .unavailable_reason
            .clone()
            .unwrap_or_else(|| "the selected model route is unavailable".into()));
    }
    let canonical = providers
        .route_for_target(requested, workspace)?
        .ok_or_else(|| {
            format!(
                "route '{}/{}/{}' is not configured",
                requested.engine_id, requested.provider_id, requested.model_id
            )
        })?;
    route.target = canonical.target;
    Ok(route)
}

/// POST /api/run — execute one product turn, streaming timeline to SSE.
async fn run(State(state): State<Arc<AppState>>, Json(body): Json<RunBody>) -> impl IntoResponse {
    let workspace = match body.workspace {
        Some(w) => PathBuf::from(w),
        None => std::env::current_dir().unwrap_or_default(),
    };

    let request = RunRequest {
        prompt: body.prompt,
        workspace: workspace.clone(),
        engine_id: body.engine_id.unwrap_or_else(|| "alva".to_string()),
        product_session_id: body.product_session_id.clone(),
        permission_profile: body.permission_profile,
    };

    let selected_target: Option<EngineTarget> = if request.product_session_id.is_none() {
        if body
            .run_token
            .as_deref()
            .filter(|t| !t.trim().is_empty())
            .is_none()
        {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "a new Hub run requires a correlation token" })),
            )
                .into_response();
        }
        let requested = match body.selected_target {
            Some(t) => t,
            None => match state.providers.default_target(Some(&request.workspace)) {
                Ok(Some(route)) => route.target,
                Ok(None) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": "Choose an available default model in Settings before starting a session." })),
                    )
                        .into_response()
                }
                Err(e) => return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
            },
        };
        match canonical_available_route(
            &state.store,
            &state.providers,
            Some(&request.workspace),
            &requested,
        ) {
            Ok(route) => Some(route.target),
            Err(e) => {
                return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response()
            }
        }
    } else {
        if body.selected_target.is_some() {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "a resumed Hub session cannot override its pinned model route" })),
            )
                .into_response();
        }
        None
    };
    let run_token = body.run_token.clone();

    let providers = state.providers.clone();
    let history_dir = state.history_dir.clone();
    let inspector = state.inspector.clone();
    let health_store = state.store.clone();
    let health_workspace = workspace.clone();
    let active_engines_for_build = state.active_engines.clone();
    let active_engines_for_emit = state.active_engines.clone();
    let permission_router = state.permission_router.clone();
    let autonomy_store = state.store.clone();
    let request_product_session = request.product_session_id.clone().unwrap_or_default();
    let approval_scope = state
        .store
        .collaboration_scope(&request_product_session)
        .unwrap_or_else(|_| request_product_session.clone());
    let autonomous_run = state
        .store
        .is_autonomous_scope(&request_product_session)
        .unwrap_or(false);
    let requester_label = match state.store.team_worker_label(&request_product_session) {
        Ok(Some(label)) => label,
        Ok(None) => format!("{} session", request.engine_id),
        Err(_) => format!("{} session", request.engine_id),
    };

    let tx = state.events.clone();

    // Spawn the (potentially long) run on the runtime; emit each timeline
    // event to the broadcast channel.
    let active_engines_for_build_cloned = active_engines_for_build.clone();
    let outcome_fut = run_persisted_with_target(
        &state.store,
        {
            let providers = providers.clone();
            let workspace = workspace.clone();
            let active_engines_for_build = active_engines_for_build_cloned.clone();
            move |target, product_session_id, profile| {
                let provider_config = providers.resolve_target(target, &workspace)?.or_else(|| {
                    // fall back to env-based config for the "alva" engine
                    // when no provider catalog entry exists yet.
                    if target.engine_id == "alva" {
                        ProviderConfig::from_env().ok()
                    } else {
                        None
                    }
                });
                let engine = build_engine_for_target_with_inspector(
                    target,
                    provider_config,
                    profile,
                    &workspace,
                    &history_dir,
                    &inspector,
                )?;
                active_engines_for_build
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
            let run_token = run_token.clone();
            let permission_router = permission_router.clone();
            let active_engines = active_engines_for_emit.clone();
            let autonomy_store = autonomy_store.clone();
            let approval_scope = approval_scope.clone();
            let requester_label = requester_label.clone();
            move |event: TimelineEvent| {
                if let Some(info) = &event.permission_request {
                    let engine = active_engines
                        .lock()
                        .unwrap()
                        .get(&event.product_session_id)
                        .cloned();
                    if let Some(engine) = engine {
                        permission_router.register(
                            &event.product_session_id,
                            &info.request_id,
                            engine,
                            &info.engine_session_id,
                            alva_hub_desktop::PermissionMember {
                                scope: if approval_scope.is_empty() {
                                    event.product_session_id.clone()
                                } else {
                                    approval_scope.clone()
                                },
                                product_session_id: event.product_session_id.clone(),
                                request_id: info.request_id.clone(),
                                requester: requester_label.clone(),
                                tool_name: info.tool_name.clone(),
                                resource: info.resource.clone(),
                            },
                        );
                    }
                    if autonomous_run {
                        let router = permission_router.clone();
                        let store = autonomy_store.clone();
                        let session = event.product_session_id.clone();
                        let request_id = info.request_id.clone();
                        let tool_name = info.tool_name.clone();
                        let resource = info.resource.clone();
                        tokio::spawn(async move {
                            let _ = router
                                .respond(
                                    &session,
                                    &request_id,
                                    alva_engine_runtime::PermissionDecision::Allow {
                                        updated_input: None,
                                    },
                                )
                                .await;
                            let summary = format!(
                                "Auto-allowed '{}'{} (autonomous TeamRun)",
                                tool_name,
                                resource.map(|r| format!(" on {r}")).unwrap_or_default()
                            );
                            let _ = store.record_permission_decision(&session, &summary);
                        });
                    }
                }
                if matches!(
                    event.status,
                    alva_hub_desktop::TimelineStatus::Completed
                        | alva_hub_desktop::TimelineStatus::Failed
                        | alva_hub_desktop::TimelineStatus::Interrupted
                        | alva_hub_desktop::TimelineStatus::Fallback
                        | alva_hub_desktop::TimelineStatus::Exhausted
                ) {
                    permission_router.clear_session(&event.product_session_id);
                }
                let wire = WireTimelineEvent {
                    event: event.clone(),
                    run_token: run_token.clone(),
                };
                let _ = tx.send(wire);
            }
        },
    );

    let outcome = match outcome_fut.await {
        Ok(outcome) => outcome,
        Err(e) => {
            warn!(error = %e, "run failed");
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response();
        }
    };
    // clean engine registry for this run
    {
        let mut map = active_engines_for_build.lock().unwrap();
        // remove any entries inserted for this product session (including fallbacks)
        map.retain(|k, _| k != &outcome.product_session_id);
        permission_router.clear_session(&outcome.product_session_id);
    }
    Json(outcome).into_response()
}

/// GET /api/events — SSE stream of TimelineEvent, same payload shape as the
/// Tauri `hub_timeline` event so the React UI's listener works unchanged.
async fn events(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut rx = state.events.subscribe();
    let stream = async_stream::stream! {
        while let Ok(wire) = rx.recv().await {
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

/// Serve the React UI. Prefers local disk `web/dist` during development so rebuilds
/// take effect immediately upon browser refresh; falls back to embedded `DIST`.
async fn static_handler(uri: axum::http::Uri) -> Response {
    let raw_path = uri.path().trim_start_matches('/');
    let path = if raw_path.is_empty() {
        "index.html"
    } else {
        raw_path
    };

    let disk_dist =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../alva-hub-desktop/web/dist");
    let candidate = disk_dist.join(path);
    if candidate.is_file() {
        if let Ok(contents) = std::fs::read(&candidate) {
            let mime = mime_for(path);
            return ([(header::CONTENT_TYPE, mime)], contents).into_response();
        }
    }

    if let Some(file) = DIST.get_file(path) {
        let mime = mime_for(path);
        return ([(header::CONTENT_TYPE, mime)], file.contents()).into_response();
    }

    // SPA fallback: any unknown non-asset path renders the shell.
    let disk_index = disk_dist.join("index.html");
    if disk_index.is_file() {
        if let Ok(contents) = std::fs::read(&disk_index) {
            return (
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                contents,
            )
                .into_response();
        }
    }

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
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info,alva_hub_server=info")
            }),
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
        .route("/api/sessions/:id", get(get_session))
        .route("/api/sessions/:id/inspection", get(get_inspection))
        .route("/api/providers", get(list_providers).post(save_provider))
        .route("/api/routes", get(list_routes))
        .route(
            "/api/default-route",
            get(get_default_route).post(set_default_route),
        )
        .route("/api/run", post(run))
        .route("/api/events", get(events))
        .route(
            "/api/fallback-policy",
            get(get_global_fallback_policy).post(set_global_fallback_policy),
        )
        .route(
            "/api/sessions/:id/fallback-policy",
            get(get_session_fallback_policy).post(set_session_fallback_policy),
        )
        .route("/api/readiness", get(get_readiness))
        .route(
            "/api/permission-eligibility",
            get(get_permission_eligibility),
        )
        .route("/api/permission-groups", get(get_permission_groups))
        .route("/api/permission/respond", post(respond_permission_group))
        .route("/api/sessions/:id/switch", post(switch_engine))
        .route("/api/sessions/:id/stop", post(stop_attempt))
        .route("/api/sessions/:id/permission", post(respond_permission))
        .route("/api/sessions/:id/memory", post(add_memory))
        .route("/api/sessions/:id/transcript", post(set_transcript))
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
