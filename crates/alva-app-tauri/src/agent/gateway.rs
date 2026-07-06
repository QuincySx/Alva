// Embedded gateway control: start/stop a localhost gateway that exposes the
// app's configured provider through the standard protocol routes. Split from
// `agent.rs` (PR-13a); command names are unchanged (= function names).

use tauri::State;

use super::run::build_provider_config;
use super::{AppState, SendMessageRequest};

/// Start an embedded gateway instance that exposes the app's configured
/// provider through the standard protocol routes:
///
/// - `POST /v1/responses`         → OpenAI Responses API
/// - `POST /v1/chat/completions`  → OpenAI Chat Completions
/// - `POST /v1/messages`          → Anthropic Messages
///
/// The `req` parameter carries the same provider settings the frontend uses
/// for `send_message`, so the caller can pass the same settings object.
/// `port` is the TCP port to bind on localhost.
///
/// The gateway is registered under the model name from `req.model` — that
/// is the alias clients must send in their request body.
///
/// Any previously running gateway is aborted before the new one starts.
/// Returns `"http://127.0.0.1:{port}"` on success.
#[tauri::command]
pub async fn start_gateway(
    state: State<'_, AppState>,
    req: SendMessageRequest,
    port: u16,
) -> Result<String, String> {
    // Build the ProviderConfig without constructing an unused LanguageModel.
    let config = build_provider_config(&req)?;
    let alias = config.model.clone();

    // Build the AliasRouter with one entry: alias → config.
    let mut router = alva_llm_provider::AliasRouter::new();
    router.insert(alias, config);

    let addr = format!("127.0.0.1:{port}");
    let addr_clone = addr.clone();

    // Spawn serve on the app's tokio runtime handle.
    // `spawn` is synchronous — it returns a JoinHandle immediately without
    // blocking, so there is no `.await` here and we never hold a Mutex
    // guard across an await point.
    let join_handle = state.tokio.spawn(async move {
        if let Err(e) = alva_app_gateway::serve(router, &addr_clone).await {
            tracing::error!(addr = %addr_clone, error = %e, "embedded gateway exited with error");
        }
    });
    let abort_handle = join_handle.abort_handle();

    // Abort any previously running gateway before storing the new handle.
    let mut guard = state
        .gateway
        .lock()
        .map_err(|_| "gateway mutex poisoned".to_string())?;
    if let Some(old) = guard.take() {
        old.abort();
    }
    *guard = Some(abort_handle);
    drop(guard); // release the lock before returning

    tracing::info!(addr = %addr, "embedded gateway started");
    Ok(format!("http://127.0.0.1:{port}"))
}

/// Stop the embedded gateway instance previously started by [`start_gateway`].
///
/// Idempotent: returns `Ok(())` even when no gateway is running.
#[tauri::command]
pub async fn stop_gateway(state: State<'_, AppState>) -> Result<(), String> {
    let mut guard = state
        .gateway
        .lock()
        .map_err(|_| "gateway mutex poisoned".to_string())?;
    if let Some(handle) = guard.take() {
        handle.abort();
        tracing::info!("embedded gateway stopped");
    }
    Ok(())
}
