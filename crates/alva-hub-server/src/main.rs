// INPUT:  alva-hub-desktop (HubStore, ProviderCatalog, run_persisted_with_target,
//         build_engine_for_target, SystemProbe, DTOs), axum, tokio
// OUTPUT: headless HTTP service: reuses Hub sessions, engines, Inspector, and timeline over HTTP/SSE
// POS:    Alva Hub as a local service — one backend, many frontends.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    routing::{delete, get, post},
    Json, Router,
};
use include_dir::{include_dir, Dir};
use serde_json::json;
use tokio::sync::broadcast;
use tracing::{info, warn};

use alva_hub_core::ProductSessionId;
use alva_hub_desktop::{
    build_engine_for_target_with_inspector, permission_eligibility, probe_readiness,
    run_persisted_with_target, ActiveHubs, EngineTarget, HubInspectorService, HubStore,
    PermissionProfile, ProviderCatalog, RunRequest, SaveProviderProfile, CreateProviderProfile,
    SystemProbe, TimelineEvent, TimelineStatus,
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
    /// R51: live per-session [`alva_hub_core::HubRuntime`] control planes,
    /// published by the run seam for the duration of each turn. Stop routes
    /// through `HubRuntime::cancel` instead of hand-rolled engine calls —
    /// same control plane as the Tauri product.
    active_hubs: Arc<ActiveHubs>,
    /// R51: sessions whose Stop was acked but whose engine stream has not
    /// ended yet. The cancelled terminal is recorded only when the run's own
    /// terminal event arrives, never on the cancel ack itself.
    /// R77: keyed pid -> attempt_id — the marker belongs to the attempt that
    /// was running when the Stop was acked, and only the run settling that
    /// same attempt may consume it.
    stopping: Arc<Mutex<HashMap<String, String>>>,
    permission_router: Arc<alva_hub_desktop::PermissionRouter>,
    /// R65: 集成测试注入 fake engine 的唯一接缝；生产路径恒为 None。
    #[cfg(test)]
    test_engine_factory: Option<
        Arc<
            dyn Fn(
                    &EngineTarget,
                    &str,
                    PermissionProfile,
                ) -> Result<Arc<dyn alva_engine_runtime::EngineRuntime>, String>
                + Send
                + Sync,
        >,
    >,
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
            active_hubs: Arc::new(Mutex::new(std::collections::HashMap::new())),
            stopping: Arc::new(Mutex::new(HashMap::new())),
            permission_router: Arc::new(alva_hub_desktop::PermissionRouter::new()),
            #[cfg(test)]
            test_engine_factory: None,
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

/// POST /api/providers/create — create a user-defined API connection (same
/// shape as Tauri `create_model_provider`). The catalog assigns the id.
async fn create_provider(
    State(state): State<Arc<AppState>>,
    Json(provider): Json<CreateProviderProfile>,
) -> impl IntoResponse {
    match state.providers.create(provider, None) {
        Ok(created) => Json(created).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response(),
    }
}

/// DELETE /api/providers/:id — delete a user-created connection (same shape
/// as Tauri `delete_model_provider`).
async fn delete_provider(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.providers.delete(&id, None) {
        Ok(removed) => Json(removed).into_response(),
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
        Ok(()) => Json(json!({ "ok": true, "outcome": "delivered" })).into_response(),
        Err(e) if alva_hub_desktop::is_stale_permission_error(&e) => {
            // Never record an audit row for a stale card (ticket 03): the
            // engine already auto-denied or the attempt ended.
            Json(json!({ "ok": false, "outcome": "expired" })).into_response()
        }
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
    // R51: route through the live Hub control plane (published by the run
    // seam) instead of calling the engine directly. HubRuntime::cancel
    // reaches the native session via its RunHandle, and releases the Hub
    // claim itself when the engine cannot cancel or no native session has
    // started yet — including the early-start window.
    let hub = match state.active_hubs.lock().unwrap().get(&id).cloned() {
        Some(hub) => hub,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "no live engine" })),
            )
                .into_response()
        }
    };
    // R77/R91: 取消事实绑定"Stop 发起时正在运行的 attempt"——身份必须在
    // await cancel 之前捕获。cancel 返回时若该 attempt 已不是运行中的那个
    //（A 已完成且 B 已认领——同分支 resume 也会铸造新 attempt——或 A 已终
    // 态），则本次迟到的 ack 什么都不写、不插 marker、不发 stopping 事件
    // ——B 的审批与运行完全不受旧 Stop 影响。
    let stop_attempt = match state.store.running_attempt_id(&id) {
        Ok(attempt) => attempt.unwrap_or_default(),
        Err(e) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": e })))
                .into_response();
        }
    };
    // R86: 审批路由不得在 cancel 之前清除——cancel 失败时原权限请求仍须
    // 可回答。清理只发生在取消事实成功写入（该轮确由本次 Stop 终止）之后。
    // R51: nothing is recorded before the cancel — a failed cancel must not
    // leave a cancelled terminal behind.
    if let Err(e) = hub.cancel(&ProductSessionId::from_existing(&id)).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    match state.store.record_attempt_cancelled_bound(&id, &stop_attempt, "User stopped") {
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e })),
            )
                .into_response();
        }
        Ok(false) => {
            // Superseded: the observed attempt already settled or a newer
            // attempt owns the session.
            return Json(json!({ "ok": true, "stopping": false, "superseded": true }))
                .into_response();
        }
        Ok(true) => {}
    }
    // The stopped attempt's approvals die with it — only now, when the cancel
    // actually took effect on the pinned attempt (R86).
    state.permission_router.clear_session(&id);
    // Cancel acked, not yet terminated: surface the stopping state now and
    // settle the cancelled projection when this run's stream really ends
    // (the run handler, after outcome_fut). R77: keyed by the pinned branch,
    // so a later run of the same session cannot consume it.
    state
        .stopping
        .lock()
        .unwrap()
        .insert(id.clone(), stop_attempt.clone());
    // R65: 终态抢在 ack 前落地的代际竞态——run 在 cancel await 期间结束
    // 时，active_hubs 条目已被移除、结算已（或将不带 marker）执行完毕；
    // 此时丢弃 marker，防止它残留。
    if state.active_hubs.lock().unwrap().get(&id).is_none() {
        state.stopping.lock().unwrap().remove(&id);
    }
    let _ = state.events.send(WireTimelineEvent {
        event: TimelineEvent {
            created_at: timeline_now_ms(),
            status: TimelineStatus::Running,
            engine_id: detail.session.engine_id,
            product_session_id: id,
            message: Some("User stopped — waiting for the engine to terminate".into()),
            item_id: None,
            activity: None,
            permission_request: None,
        },
        run_token: None,
    });
    Json(json!({ "ok": true, "stopping": true })).into_response()
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
    // RAII: releases the engine handles this run registers on every exit —
    // success, failure, and the handler future being dropped when the client
    // disconnects mid-run (an axum handler is cancelled at its await point, so
    // a statement after the `.await` would simply never run).
    let engine_lease = alva_hub_desktop::RunEngineLease::new(
        active_engines_for_build.clone(),
        permission_router.clone(),
    );
    let build_registry = engine_lease.registry();
    let autonomy_store = state.store.clone();
    let request_product_session = request.product_session_id.clone().unwrap_or_default();
    let request_engine_id = request.engine_id.clone();
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
    let stopping = state.stopping.clone();
    // R78: 这里不得按 pid 清理 stopping marker——本请求可能在认领前就被
    // 拒绝（同 session 已在运行），删掉的是活动轮的 marker，让用户已
    // ack 的 Stop 在该轮异常结束时丢失 Interrupted 结算。marker 的身份
    // 约束由 R77 的分支键保证：它只能被它 ack 时活跃的那轮结算消费。
    // R65: 引擎流是否自然完成。取消结算据此决定是否追加 Interrupted。
    let run_completed_normally = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Spawn the (potentially long) run on the runtime; emit each timeline
    // event to the broadcast channel.
    let active_engines_for_build_cloned = active_engines_for_build.clone();
    #[cfg(test)]
    let test_engine_factory = state.test_engine_factory.clone();
    let outcome_fut = run_persisted_with_target(
        &state.store,
        {
            let providers = providers.clone();
            let workspace = workspace.clone();
            let active_engines_for_build = active_engines_for_build_cloned.clone();
            move |target, product_session_id, profile| {
                #[cfg(test)]
                if let Some(factory) = test_engine_factory.as_ref() {
                    return factory(target, product_session_id, profile);
                }
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
                build_registry
                    .lock()
                    .unwrap()
                    .push(product_session_id.to_string());
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
        // R51: publish the live per-session HubRuntime so Stop routes through
        // the same Hub control plane the Tauri product uses.
        Some(state.active_hubs.as_ref()),
        {
            let tx = tx.clone();
            let run_token = run_token.clone();
            let permission_router = permission_router.clone();
            let active_engines = active_engines_for_emit.clone();
            let autonomy_store = autonomy_store.clone();
            let approval_scope = approval_scope.clone();
            let requester_label = requester_label.clone();
            let run_completed_normally = run_completed_normally.clone();
            move |event: TimelineEvent| {
                if event.status == alva_hub_desktop::TimelineStatus::Completed {
                    // R65: 引擎流自然完成（非取消截断）——结算时据此不再
                    // 追加 Interrupted，避免一个取消结算输出冲突终态。
                    run_completed_normally.store(true, std::sync::atomic::Ordering::SeqCst);
                }
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
                            // The audit line must follow the engine actually
                            // accepting the decision. Writing it unconditionally
                            // records an "Auto-allowed" for a tool the engine may
                            // have already timed out and blocked.
                            match router
                                .respond(
                                    &session,
                                    &request_id,
                                    alva_engine_runtime::PermissionDecision::Allow {
                                        updated_input: None,
                                    },
                                )
                                .await
                            {
                                Ok(()) => {
                                    let summary = format!(
                                        "Auto-allowed '{}'{} (autonomous TeamRun)",
                                        tool_name,
                                        resource.map(|r| format!(" on {r}")).unwrap_or_default()
                                    );
                                    let _ = store.record_permission_decision(&session, &summary);
                                }
                                Err(error) => {
                                    tracing::warn!(
                                        %error,
                                        %request_id,
                                        "autonomous auto-approval was not accepted by the engine; no audit line written"
                                    );
                                }
                            }
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
                    // R65: 这里不再结算取消——Failed 在 fatal Error 事件就
                    // 发出，不代表引擎流真的结束了。取消的 Interrupted 终态
                    // 由 run handler 在 outcome（真实流结束）之后统一结算。
                }
                let wire = WireTimelineEvent {
                    event: event.clone(),
                    run_token: run_token.clone(),
                };
                let _ = tx.send(wire);
            }
        },
    );

    let outcome_result = outcome_fut.await;
    // Clean up before branching: the error path used to `return` above this
    // block, so a failing turn (most easily a SQLite busy timeout in the
    // finish_turn / apply_pending_switch writes, both of which run after the
    // engine is already registered) left the handle in the map forever. A later
    // stop on that session would then reach a dead adapter and report success.
    drop(engine_lease);

    // R65: 取消的 SSE 终态在真实流结束（outcome 就绪）之后统一结算。
    // mid-stream 的 Failed 只代表 fatal Error 事件到达，不代表引擎流
    // 结束；这里消费 marker（恰好一次）后，仅在引擎没有自然完成时追加
    // 唯一的 Interrupted 终态，取消结算不再输出互相冲突的状态对。
    let settle_session = match &outcome_result {
        Ok(outcome) => outcome.product_session_id.as_str().to_string(),
        Err(_) => request_product_session.clone(),
    };
    // R77: 只结算绑定到本 run 自己 attempt 的 marker——旧轮迟到的 marker
    // 永远不会被新一轮误消费。
    let run_attempt = match state.store.latest_attempt_id(&settle_session) {
        Ok(attempt) => attempt.unwrap_or_default(),
        Err(error) => {
            warn!(error = %error, "could not read the settling attempt; leaving any marker untouched");
            // Leave the marker for a later settle of the same attempt.
            let outcome = match outcome_result {
                Ok(outcome) => outcome,
                Err(e) => {
                    warn!(error = %e, "run failed");
                    return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response();
                }
            };
            return Json(outcome).into_response();
        }
    };
    let was_stopping = stopping
        .lock()
        .expect("stopping lock poisoned")
        .remove(&settle_session)
        .is_some_and(|marker_attempt| marker_attempt == run_attempt);
    if was_stopping && !run_completed_normally.load(std::sync::atomic::Ordering::SeqCst) {
        let _ = tx.send(WireTimelineEvent {
            event: TimelineEvent {
                created_at: timeline_now_ms(),
                status: TimelineStatus::Interrupted,
                engine_id: request_engine_id,
                product_session_id: settle_session,
                message: Some("User stopped".into()),
                item_id: None,
                activity: None,
                permission_request: None,
            },
            run_token: run_token.clone(),
        });
    }

    let outcome = match outcome_result {
        Ok(outcome) => outcome,
        Err(e) => {
            warn!(error = %e, "run failed");
            return (StatusCode::BAD_REQUEST, Json(json!({ "error": e }))).into_response();
        }
    };
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
    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| format!("bind {addr}: {e}"))?;
    info!("alva-hub-server listening on http://{addr}");
    axum::serve(listener, app)
        .await
        .map_err(|e| format!("server error: {e}"))
}

/// R65: router 构建独立成函数，真实 HTTP/SSE 集成测试直接装配同一个 app。
fn build_app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/sessions", get(list_sessions))
        .route("/api/sessions/:id", get(get_session))
        .route("/api/sessions/:id/inspection", get(get_inspection))
        .route(
            "/api/providers",
            get(list_providers).post(save_provider),
        )
        .route("/api/providers/create", post(create_provider))
        .route("/api/providers/:id", delete(delete_provider))
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
        .with_state(state)
}

// ---------------------------------------------------------------------------
// R65: real HTTP/SSE stop-settlement integration tests. Axum serves the same
// `build_app` router on an ephemeral port; the fake engine is injected through
// the test-only engine factory and gated per scenario.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod r65_stop_integration {
    use super::*;
    use alva_engine_runtime::{
        PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent, RuntimeRequest,
    };
    use async_trait::async_trait;
    use futures_core::Stream;
    use std::collections::VecDeque;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpStream;

    type EngineStream = Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>;

    /// turn 0 的流脚本；turn ≥1 一律立即 Started + Completed。
    #[derive(Clone)]
    enum Step {
        /// gate 放行后发 fatal Error。
        Fatal(String),
        /// gate 放行后发 Completed。
        Complete,
        /// 立即发 Completed（无 gate）。
        CompleteNow,
        /// 停在 pending，直到 cancel 后被 kill（EOF 无终态）。
        HoldUntilKilled,
    }

    struct ScriptedEngine {
        turn: AtomicUsize,
        steps: std::sync::Mutex<VecDeque<Step>>,
        advance: Arc<tokio::sync::Notify>,
        started: Arc<tokio::sync::Notify>,
        cancel_fails: AtomicBool,
        /// false：cancel 立即返回 Ok（notify cancel_entered 后直接成功）。
        cancel_gates: AtomicBool,
        cancel_entered: Arc<tokio::sync::Notify>,
        cancel_release: Arc<tokio::sync::Notify>,
        /// HoldUntilKilled 的终止信号由测试在 stop ack 之后显式触发。
        killed: Arc<tokio::sync::Notify>,
        /// R77: 第二轮（n>=1）启动后停在 killed gate——用于构造"迟到 cancel
        /// ack 时 B 仍在运行"的交错。
        second_hold: AtomicBool,
    }

    impl ScriptedEngine {
        fn new(steps: Vec<Step>, cancel_fails: bool) -> Arc<Self> {
            Self::with_gates(steps, cancel_fails, true)
        }

        fn with_gates(steps: Vec<Step>, cancel_fails: bool, cancel_gates: bool) -> Arc<Self> {
            Arc::new(Self {
                turn: AtomicUsize::new(0),
                steps: std::sync::Mutex::new(steps.into()),
                advance: Arc::new(tokio::sync::Notify::new()),
                started: Arc::new(tokio::sync::Notify::new()),
                cancel_fails: AtomicBool::new(cancel_fails),
                cancel_gates: AtomicBool::new(cancel_gates),
                cancel_entered: Arc::new(tokio::sync::Notify::new()),
                cancel_release: Arc::new(tokio::sync::Notify::new()),
                killed: Arc::new(tokio::sync::Notify::new()),
                second_hold: AtomicBool::new(false),
            })
        }

        /// R77: 让第二轮（n>=1）启动后停在 killed gate。
        fn hold_second_turn(self: &Arc<Self>) -> Arc<Self> {
            self.second_hold.store(true, Ordering::SeqCst);
            self.clone()
        }

        async fn release(&self) {
            self.advance.notify_one();
            // Notify 允许唤醒先于泊车到达；再让一步确保 gate 内的任务
            // 真正拿到唤醒并继续产出。
            tokio::task::yield_now().await;
        }
    }

    fn started_event(session_id: String) -> RuntimeEvent {
        RuntimeEvent::SessionStarted {
            session_id,
            model: None,
            tools: vec![],
        }
    }

    fn completed_event(session_id: String) -> RuntimeEvent {
        RuntimeEvent::Completed {
            session_id,
            result: Some("done".into()),
            usage: None,
        }
    }

    #[async_trait]
    impl alva_engine_runtime::EngineRuntime for ScriptedEngine {
        fn execute(&self, _request: RuntimeRequest) -> Result<EngineStream, RuntimeError> {
            let n = self.turn.fetch_add(1, Ordering::SeqCst);
            self.started.notify_one();
            let session_id = format!("native-{n}");
            let steps: VecDeque<Step> = if n == 0 {
                self.steps.lock().unwrap().drain(..).collect()
            } else if self.second_hold.load(Ordering::SeqCst) {
                VecDeque::from(vec![Step::HoldUntilKilled])
            } else {
                VecDeque::from(vec![Step::CompleteNow])
            };
            let advance = self.advance.clone();
            let killed = self.killed.clone();
            let stream = async_stream::stream! {
                yield started_event(session_id.clone());
                for step in steps {
                    match step {
                        Step::CompleteNow => {
                            yield completed_event(session_id.clone());
                        }
                        Step::Fatal(message) => {
                            advance.notified().await;
                            yield RuntimeEvent::Error { message: message.clone(), recoverable: false };
                        }
                        Step::Complete => {
                            advance.notified().await;
                            yield completed_event(session_id.clone());
                        }
                        Step::HoldUntilKilled => {
                            // 模拟 cancel 导致引擎断流：EOF、无任何终态。
                            killed.notified().await;
                            return;
                        }
                    }
                }
            };
            Ok(Box::pin(stream))
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
            self.cancel_entered.notify_one();
            if self.cancel_gates.load(Ordering::SeqCst) {
                self.cancel_release.notified().await;
            }
            if self.cancel_fails.load(Ordering::SeqCst) {
                return Err(RuntimeError::ProcessError("cancel refused".into()));
            }
            Ok(())
        }

        async fn respond_permission(
            &self,
            _session_id: &str,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
            Ok(())
        }

        fn capabilities(&self) -> RuntimeCapabilities {
            RuntimeCapabilities {
                streaming: true,
                tool_control: false,
                permission_callback: false,
                resume: true,
                cancel: true,
            }
        }
    }

    struct TestServer {
        addr: std::net::SocketAddr,
        engine: Arc<ScriptedEngine>,
        data_dir: PathBuf,
    }

    static NEXT_PORT: AtomicUsize = AtomicUsize::new(0);

    async fn spawn_server(steps: Vec<Step>, cancel_fails: bool) -> TestServer {
        spawn_server_gated(steps, cancel_fails, true).await
    }

    /// R77: 注入预构建引擎的变体（测试需要先配置 second_hold 等开关）。
    async fn spawn_server_with_engine(
        _steps: Vec<Step>,
        engine: Arc<ScriptedEngine>,
    ) -> TestServer {
        spawn_server_engine(engine).await
    }

    async fn spawn_server_gated(
        steps: Vec<Step>,
        cancel_fails: bool,
        cancel_gates: bool,
    ) -> TestServer {
        spawn_server_engine(ScriptedEngine::with_gates(steps, cancel_fails, cancel_gates)).await
    }

    async fn spawn_server_engine(engine: Arc<ScriptedEngine>) -> TestServer {
        let data_dir = std::env::temp_dir().join(format!(
            "alva-hub-server-r65-{}-{}",
            std::process::id(),
            NEXT_PORT.fetch_add(1, Ordering::SeqCst)
        ));
        let workspace = data_dir.join("workspace");
        std::fs::create_dir_all(workspace.join(".alva")).unwrap();
        // quick_veto("alva") 依赖 workspace 的 provider 配置。
        std::fs::write(
            workspace.join(".alva/config.json"),
            r#"{"api_key":"test-key"}"#,
        )
        .unwrap();

        let mut state = Arc::new(AppState::new(data_dir.clone()).unwrap());
        // R65: 预置 anthropic→alva 默认路由，使 /api/run 的新 run 路径
        // 通过 canonical route 检查（引擎本体由测试工厂注入）。
        state
            .providers
            .save(
                SaveProviderProfile {
                    id: "anthropic".into(),
                    display_name: "Anthropic direct".into(),
                    base_url: "https://api.anthropic.com/".into(),
                    enabled: true,
                    api_key: Some("test-key".into()),
                    clear_api_key: false,
                    max_output_tokens: 4096,
                    models: vec![alva_hub_desktop::ProviderModel {
                        id: "test-model".into(),
                        display_name: "Test model".into(),
                        enabled: true,
                        registry_id: None,
                        context_window: Some(100_000),
                        capabilities: vec![],
                        supported_engines: vec!["alva".into()],
                        runtime_options: Default::default(),
                    }],
                },
                None,
            )
            .unwrap();
        {
            let mut target = EngineTarget::for_engine("alva").unwrap();
            target.model_id = "test-model".into();
            target.provider_id = "anthropic".into();
            state.providers.set_default_target(target, None).unwrap();
        }
        let factory_engine = engine.clone();
        Arc::get_mut(&mut state)
            .expect("state not yet shared")
            .test_engine_factory = Some(Arc::new(move |_target, _pid, _profile| {
            Ok(factory_engine.clone() as Arc<dyn alva_engine_runtime::EngineRuntime>)
        }));

        let app = build_app(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        TestServer {
            addr,
            engine,
            data_dir,
        }
    }

    impl Drop for TestServer {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }

    /// 订阅 /api/events，把全部 SSE 帧累积到共享缓冲。连接（含写半）
    /// 整体驻留在后台任务里——写半被丢弃会向服务端半关闭连接并终止 SSE 流。
    async fn subscribe_sse(addr: std::net::SocketAddr) -> Arc<std::sync::Mutex<String>> {
        let buffer = Arc::new(std::sync::Mutex::new(String::new()));
        let shared = buffer.clone();
        let request = format!(
            "GET /api/events HTTP/1.1\r\nHost: {addr}\r\nAccept: text/event-stream\r\n\r\n"
        );
        tokio::spawn(async move {
            let stream = TcpStream::connect(addr).await.expect("SSE connect");
            let (reader, mut writer) = stream.into_split();
            writer.write_all(request.as_bytes()).await.unwrap();
            let mut lines = BufReader::new(reader).lines();
            loop {
                match lines.next_line().await {
                    Ok(Some(line)) => {
                        shared.lock().unwrap().push_str(&line);
                        shared.lock().unwrap().push('\n');
                    }
                    Ok(None) => break,
                    Err(error) => {
                        eprintln!("SSE reader error: {error}");
                        break;
                    }
                }
            }
        });
        buffer
    }

    /// 有界轮询直到缓冲包含 needle；负向断言用 assert_absent。
    async fn wait_for(buffer: &Arc<std::sync::Mutex<String>>, needle: &str) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if buffer.lock().unwrap().contains(needle) {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "SSE stream should eventually contain {needle:?}; buffer:\n{}",
                buffer.lock().unwrap()
            )
        });
    }

    /// 有界轮询，直到 SSE 缓冲里出现首个带 product_session_id 的事件。
    async fn wait_for_pid(buffer: &Arc<std::sync::Mutex<String>>) -> String {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let pid = extract_pid(buffer);
                if let Some(pid) = pid {
                    return pid;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "SessionStarted event should announce the product session; buffer:\n{}",
                buffer.lock().unwrap()
            )
        })
    }

    fn extract_pid(buffer: &Arc<std::sync::Mutex<String>>) -> Option<String> {
        buffer
            .lock()
            .unwrap()
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .find_map(|line| {
                let event: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
                // WireTimelineEvent 把 TimelineEvent 扁平序列化。
                event["product_session_id"]
                    .as_str()
                    .map(str::to_string)
            })
    }

    fn assert_absent(buffer: &Arc<std::sync::Mutex<String>>, needle: &str) {
        assert!(
            !buffer.lock().unwrap().contains(needle),
            "SSE stream must not contain {needle:?}:\n{}",
            buffer.lock().unwrap()
        );
    }

    async fn http_post(
        addr: std::net::SocketAddr,
        path: String,
        body: String,
    ) -> (u16, String) {
        let stream = TcpStream::connect(addr).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let request = format!(
            "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        writer.write_all(request.as_bytes()).await.unwrap();
        let mut raw = String::new();
        let mut reader = BufReader::new(reader);
        reader.read_to_string(&mut raw).await.unwrap();
        let status: u16 = raw
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        let body = raw
            .split("\r\n\r\n")
            .nth(1)
            .unwrap_or_default()
            .to_string();
        (status, body)
    }

    fn run_body(workspace: &str, product_session_id: Option<&str>) -> String {
        let session_part = product_session_id
            .map(|id| format!(r#","product_session_id":"{id}""#))
            .unwrap_or_default();
        format!(
            r#"{{"prompt":"hello","workspace":"{workspace}","engine_id":"alva","run_token":"token-r65"{session_part}}}"#
        )
    }

    /// R65: ack 后流未结束时不发任何取消终态（Error 只是事件，不是流
    /// 结束）；真实流结束（outcome 就绪）后恰好结算一个 Interrupted，
    /// 作为最后的取消终态。
    #[tokio::test(flavor = "multi_thread")]
    async fn stop_acked_error_then_completed_projects_single_terminal() {
        let server = spawn_server_gated(
            vec![Step::Fatal("engine fatal".into()), Step::Complete],
            false,
            false,
        )
        .await;
        let sse = subscribe_sse(server.addr).await;
        // 等订阅握手完成（响应头进缓冲），再触发会产生事件的操作——
        // broadcast 不回放订阅前的事件。
        wait_for(&sse, "text/event-stream").await;

        let workspace = server.data_dir.join("workspace");
        let ws = workspace.display().to_string();
        let run_task = tokio::spawn(http_post(
            server.addr,
            "/api/run".to_string(),
            run_body(&ws, None),
        ));
        server.engine.started.notified().await;

        let pid = wait_for_pid(&sse).await;

        // Stop ack：引擎 cancel 立即 Ok，session 仍在 running（流被 gate 住）。
        let (status, body) = http_post(
            server.addr,
            format!("/api/sessions/{pid}/stop"),
            "{}".to_string(),
        )
        .await;
        assert_eq!(status, 200, "stop should ack: {body}");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&body).unwrap()["stopping"],
            json!(true)
        );
        wait_for(&sse, "waiting for the engine to terminate").await;

        // Error 后流仍未结束：不投影任何取消终态。
        server.engine.release().await;
        wait_for(&sse, "engine fatal").await;
        assert_absent(&sse, "\"interrupted\"");

        // 引擎 fatal 之后发 Completed：run.rs 对已带 terminal_error 的
        // Completed 不再投影 completed——用户停掉的这轮以唯一的
        // Interrupted 结算收尾（在 Failed 之后）。
        server.engine.release().await;
        let (run_status, run_response) = run_task.await.unwrap();
        assert_eq!(run_status, 200, "{run_response}");
        wait_for(&sse, "\"interrupted\"").await;
        let sse_text = sse.lock().unwrap().clone();
        assert_eq!(
            sse_text.matches("\"interrupted\"").count(),
            1,
            "exactly one cancelled settlement:\n{sse_text}"
        );
        assert!(
            sse_text.find("\"failed\"").unwrap() < sse_text.find("\"interrupted\"").unwrap(),
            "the cancelled settlement must be the final terminal"
        );
    }

    /// R65: ack 后引擎延迟自然完成（无 fatal）——期间保持停止中、不投影
    /// 取消终态；真实完成后取消结算不再追加 Interrupted，终态只有
    /// Completed 一个正确状态。
    #[tokio::test(flavor = "multi_thread")]
    async fn stop_acked_delayed_natural_completion_projects_completed_only() {
        let server = spawn_server(vec![Step::Complete], false).await;
        let sse = subscribe_sse(server.addr).await;
        wait_for(&sse, "text/event-stream").await;

        let workspace = server.data_dir.join("workspace");
        let ws = workspace.display().to_string();
        let run_task = tokio::spawn(http_post(
            server.addr,
            "/api/run".to_string(),
            run_body(&ws, None),
        ));
        server.engine.started.notified().await;
        let pid = wait_for_pid(&sse).await;

        let stop_task = tokio::spawn(http_post(
            server.addr,
            format!("/api/sessions/{pid}/stop"),
            "{}".to_string(),
        ));
        server.engine.cancel_entered.notified().await;
        server.engine.cancel_release.notify_one();
        let (status, body) = stop_task.await.unwrap();
        assert_eq!(status, 200, "stop should ack: {body}");
        wait_for(&sse, "waiting for the engine to terminate").await;

        // 流仍被 gate 住：没有任何终态。
        assert_absent(&sse, "\"completed\"");
        assert_absent(&sse, "\"interrupted\"");

        // 延迟自然完成：只有一个 Completed 终态，无取消结算。
        server.engine.release().await;
        let (run_status, run_response) = run_task.await.unwrap();
        assert_eq!(run_status, 200, "{run_response}");
        wait_for(&sse, "\"completed\"").await;
        let sse_text = sse.lock().unwrap().clone();
        assert_eq!(
            sse_text.matches("\"interrupted\"").count(),
            0,
            "a naturally completed turn must not gain a cancelled settlement:\n{sse_text}"
        );
    }

    /// R65: ack 后引擎被取消截断（EOF 无终态）→ 真实流结束时恰好投影
    /// 一个 Interrupted，且在合成的 Failed 之后。
    #[tokio::test(flavor = "multi_thread")]
    async fn stop_acked_killed_stream_settles_interrupted_once_at_real_end() {
        let server = spawn_server(vec![Step::HoldUntilKilled], false).await;
        let sse = subscribe_sse(server.addr).await;
        // 等订阅握手完成（响应头进缓冲），再触发会产生事件的操作——
        // broadcast 不回放订阅前的事件。
        wait_for(&sse, "text/event-stream").await;

        let workspace = server.data_dir.join("workspace");
        let ws = workspace.display().to_string();
        let run_task = tokio::spawn(http_post(
            server.addr,
            "/api/run".to_string(),
            run_body(&ws, None),
        ));
        server.engine.started.notified().await;

        let pid = wait_for_pid(&sse).await;

        // 先放行 cancel gate 之前，先停：cancel 会等 gate；这里让 cancel
        // 直接通过（无需先 release，cancel_release 未被等待前 notify 保留）。
        // 实际顺序：stop 请求 → cancel 进入引擎（cancel_entered）→ 释放
        // cancel_release → cancel Ok + kill → 引擎流 EOF。
        let stop_task = tokio::spawn(http_post(
            server.addr,
            format!("/api/sessions/{pid}/stop"),
            "{}".to_string(),
        ));
        server.engine.cancel_entered.notified().await;
        server.engine.cancel_release.notify_one();

        let (stop_status, stop_body) = stop_task.await.unwrap();
        assert_eq!(stop_status, 200, "{stop_body}");

        // ack 已落地（record/mark 已完成），此刻才让引擎断流。
        server.engine.killed.notify_one();
        let (run_status, run_response) = run_task.await.unwrap();
        assert_eq!(run_status, 200, "{run_response}");

        // 真实流结束（outcome 就绪）后才结算唯一的 Interrupted。
        wait_for(&sse, "\"interrupted\"").await;
        let sse_text = sse.lock().unwrap().clone();
        let interrupted = sse_text.matches("\"interrupted\"").count();
        let failed = sse_text.matches("\"failed\"").count();
        assert_eq!(interrupted, 1, "exactly one Interrupted:\n{sse_text}");
        assert!(
            failed >= 1,
            "synthesized fatal must surface as Failed before Interrupted"
        );
        let failed_pos = sse_text.find("\"failed\"").unwrap();
        let interrupted_pos = sse_text.find("\"interrupted\"").unwrap();
        assert!(
            failed_pos < interrupted_pos,
            "Interrupted must be the single final settlement, after the Failed"
        );
    }

    /// R78: 被拒绝的重复 Run 不得清理活动轮的 stopping marker。A 的 Stop
    /// 已 ack、真实流仍挂起；同 pid 重复 Run 在认领前被拒绝后，A 异常结束
    /// 时原 Stop 仍须恰好结算一次 Interrupted。
    #[tokio::test(flavor = "multi_thread")]
    async fn rejected_duplicate_run_preserves_the_active_stopping_marker() {
        let server = spawn_server(vec![Step::HoldUntilKilled], false).await;
        let sse = subscribe_sse(server.addr).await;
        wait_for(&sse, "text/event-stream").await;
        let ws = server.data_dir.join("workspace").display().to_string();
        let run_task = tokio::spawn(http_post(
            server.addr,
            "/api/run".to_string(),
            run_body(&ws, None),
        ));
        server.engine.started.notified().await;
        let pid = wait_for_pid(&sse).await;

        // Stop ack（cancel 直接 Ok）→ marker 落下，A 的流仍挂起。
        let stop_task = tokio::spawn(http_post(
            server.addr,
            format!("/api/sessions/{pid}/stop"),
            "{}".to_string(),
        ));
        server.engine.cancel_entered.notified().await;
        server.engine.cancel_release.notify_one();
        let (stop_status, stop_body) = stop_task.await.unwrap();
        assert_eq!(stop_status, 200, "{stop_body}");

        // 同 pid 重复 Run：必须被拒绝（already running）。
        let (dup_status, dup_body) = http_post(
            server.addr,
            "/api/run".to_string(),
            run_body(&ws, Some(&pid)),
        )
        .await;
        assert_ne!(dup_status, 200, "duplicate run must be rejected: {dup_body}");

        // 释放 A：异常结束（EOF 无终态）。原 Stop 仍须恰好结算一次
        // Interrupted——被拒的重复 Run 不得带走它的 marker。
        server.engine.killed.notify_one();
        let (run_status, run_response) = run_task.await.unwrap();
        assert_eq!(run_status, 200, "{run_response}");
        wait_for(&sse, "\"interrupted\"").await;
        let sse_text = sse.lock().unwrap().clone();
        assert_eq!(
            sse_text.matches("\"interrupted\"").count(),
            1,
            "the original stop must settle exactly one Interrupted despite the rejected duplicate run:\n{sse_text}"
        );
    }

    /// R77: 迟到的 cancel ack 不得把取消事实/marker/stopping 状态写到
    /// 同 pid 的下一轮 B 上。A 运行 → Stop 捕获 A 的 hub 后 await cancel
    /// → A 完成 → B 开始并保持运行 → A 的 cancel 才返回 Ok。
    #[tokio::test(flavor = "multi_thread")]
    async fn late_cancel_ack_never_marks_the_next_attempt() {
        let engine = ScriptedEngine::new(vec![Step::Complete], false)
            .hold_second_turn();
        let server = spawn_server_with_engine(vec![], engine.clone()).await;
        let sse = subscribe_sse(server.addr).await;
        wait_for(&sse, "text/event-stream").await;
        let ws = server.data_dir.join("workspace").display().to_string();
        let run_task = tokio::spawn(http_post(
            server.addr,
            "/api/run".to_string(),
            run_body(&ws, None),
        ));
        engine.started.notified().await;
        let pid = wait_for_pid(&sse).await;

        // Stop：cancel 进入引擎后停在 gate。
        let stop_task = tokio::spawn(http_post(
            server.addr,
            format!("/api/sessions/{pid}/stop"),
            "{}".to_string(),
        ));
        engine.cancel_entered.notified().await;

        // A 在 cancel 返回前自然完成。
        engine.release().await;
        let (run_status, run_response) = run_task.await.unwrap();
        assert_eq!(run_status, 200, "{run_response}");

        // B 在同一 pid 上开始并保持运行。
        let second_task = tokio::spawn(http_post(
            server.addr,
            "/api/run".to_string(),
            run_body(&ws, Some(&pid)),
        ));
        engine.started.notified().await;

        // A 的 cancel 才返回 Ok——此刻 B 正在运行。
        engine.cancel_release.notify_one();
        let (stop_status, stop_body) = stop_task.await.unwrap();
        assert_eq!(stop_status, 200, "{stop_body}");
        let stop_json: serde_json::Value = serde_json::from_str(&stop_body).unwrap();
        assert_eq!(
            stop_json["stopping"],
            json!(false),
            "a superseded stop must not mark the next attempt as stopping: {stop_body}"
        );
        assert_eq!(
            stop_json["superseded"],
            json!(true),
            "the response must say the stop was superseded: {stop_body}"
        );
        assert_absent(&sse, "User stopped");

        // B 正常收尾：不产生旧 Stop 导致的 Interrupted。
        engine.killed.notify_one();
        let (second_status, second_response) = second_task.await.unwrap();
        assert_eq!(second_status, 200, "{second_response}");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_absent(&sse, "\"interrupted\"");
    }

    /// R65: cancel Err 不记取消——stop 返回 500 携带错误，无 stopping
    /// marker、无 waiting/Interrupted 投影，run 随后正常完成。
    #[tokio::test(flavor = "multi_thread")]
    async fn failed_cancel_records_nothing_and_run_completes() {
        let server = spawn_server(vec![Step::Complete], true).await;
        let sse = subscribe_sse(server.addr).await;
        // 等订阅握手完成（响应头进缓冲），再触发会产生事件的操作——
        // broadcast 不回放订阅前的事件。
        wait_for(&sse, "text/event-stream").await;

        let workspace = server.data_dir.join("workspace");
        let ws = workspace.display().to_string();
        let run_task = tokio::spawn(http_post(
            server.addr,
            "/api/run".to_string(),
            run_body(&ws, None),
        ));
        server.engine.started.notified().await;

        let pid = wait_for_pid(&sse).await;

        let stop_task = tokio::spawn(http_post(
            server.addr,
            format!("/api/sessions/{pid}/stop"),
            "{}".to_string(),
        ));
        server.engine.cancel_entered.notified().await;
        server.engine.cancel_release.notify_one();
        let (stop_status, stop_body) = stop_task.await.unwrap();
        assert_eq!(stop_status, 500, "cancel Err must surface: {stop_body}");
        assert!(stop_body.contains("cancel refused"));

        // 无取消投影。
        assert_absent(&sse, "waiting for the engine to terminate");
        assert_absent(&sse, "\"interrupted\"");

        // 放行引擎完成：run 正常完成，session 进入 completed。
        server.engine.release().await;
        let (run_status, run_response) = run_task.await.unwrap();
        assert_eq!(run_status, 200, "{run_response}");
        wait_for(&sse, "\"completed\"").await;

        // 存储侧同样没有取消事实。
        let stream = TcpStream::connect(server.addr).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        writer
            .write_all(
                format!(
                    "GET /api/sessions/{pid} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                    server.addr
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        let mut raw = String::new();
        let mut reader = BufReader::new(reader);
        reader.read_to_string(&mut raw).await.unwrap();
        let detail = raw.split("\r\n\r\n").nth(1).unwrap_or_default();
        assert!(
            !detail.contains("attempt_cancelled"),
            "a failed cancel must not record the cancelled fact:\n{detail}"
        );
    }

    /// R65: 终态抢在 ack 前（turn 已完成后再 stop）→ 请求被诚实拒绝，
    /// 同 session 下一轮照常完成，取消事实零泄漏。
    #[tokio::test(flavor = "multi_thread")]
    async fn terminal_before_ack_leaves_no_marker_for_next_turn() {
        let server = spawn_server(vec![Step::CompleteNow], false).await;
        let sse = subscribe_sse(server.addr).await;
        // 等订阅握手完成（响应头进缓冲），再触发会产生事件的操作——
        // broadcast 不回放订阅前的事件。
        wait_for(&sse, "text/event-stream").await;

        let workspace = server.data_dir.join("workspace");
        let ws = workspace.display().to_string();

        // 第一轮直接跑完。
        let (run_status, run_response) = http_post(
            server.addr,
            "/api/run".to_string(),
            run_body(&ws, None),
        )
        .await;
        assert_eq!(run_status, 200, "{run_response}");
        wait_for(&sse, "\"completed\"").await;

        let pid = wait_for_pid(&sse).await;

        // 迟到的 stop：会话已终态，必须被拒绝而非静默成功。
        let (stop_status, stop_body) = http_post(
            server.addr,
            format!("/api/sessions/{pid}/stop"),
            "{}".to_string(),
        )
        .await;
        assert!(
            stop_status == 400 || stop_status == 404 || stop_status == 409,
            "late stop must be rejected honestly: {stop_status} {stop_body}"
        );
        assert_absent(&sse, "\"interrupted\"");

        // 同 session 下一轮：正常完成，无取消残留。
        let (next_status, next_response) = http_post(
            server.addr,
            "/api/run".to_string(),
            run_body(&ws, Some(&pid)),
        )
        .await;
        assert_eq!(next_status, 200, "{next_response}");
        // 有界等待第二轮的 completed 帧进缓冲（SSE 投递与响应到达可微乱序）。
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if sse.lock().unwrap().matches("\"completed\"").count() >= 2 {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("both turns must complete");
        let sse_text = sse.lock().unwrap().clone();
        assert_eq!(
            sse_text.matches("\"interrupted\"").count(),
            0,
            "no cancelled projection may leak into the next turn"
        );
    }
}
