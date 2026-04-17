// INPUT:  subprocess::SubprocessRuntime, dispatcher::RpcDispatcher, manifest::PluginManifest,
//         protocol::{InitializeParams, InitializeResult, ExtensionAction, BeforeToolCallParams, ...}
// OUTPUT: RemoteExtensionProxy, ProxyError
// POS:    Phase 3 — one subprocess plugin, ready to dispatch events to.

//! A single dynamically-loaded plugin, wrapped so it can participate
//! in the host's `Extension` event machinery.
//!
//! One [`RemoteExtensionProxy`] owns one subprocess and one
//! [`RpcDispatcher`]. It knows how to:
//!
//! - drive the AEP handshake (`initialize` + `initialized`)
//! - report which events the plugin subscribed to
//! - **synchronously** dispatch an [`ExtensionEvent`] to the plugin
//!   and translate the response into an [`EventResult`]
//!
//! The synchronous dispatch is the tricky part: the host's
//! `ExtensionHost::emit` path is pure sync, but our dispatcher is
//! async. We bridge via `tokio::task::block_in_place` + a current
//! runtime `block_on`. That requires the process to use a
//! multi-thread tokio runtime — the agent already does.
//!
//! ## Phase 3 action coverage
//!
//! The host's `EventResult` currently has only `Continue`, `Block`,
//! and `Handled`. AEP defines seven `ExtensionAction` variants. Phase
//! 3 only honours `Continue` and `Block` — anything else is logged
//! and treated as `Continue`. Growing the host to honour `Modify` /
//! `ReplaceResult` / etc. is a follow-up that touches
//! `alva-agent-core::extension::events`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use alva_agent_core::extension::{EventResult, ExtensionEvent};
use serde_json::Value;

use crate::dispatcher::{DispatchError, RpcDispatcher};
use crate::host_api::AlvaHostHandler;
use crate::manifest::PluginManifest;
use crate::protocol::{
    methods, BeforeToolCallParams, ExtensionAction, HostCapabilities, HostInfo,
    InitializeParams, InitializeResult, StateHandle, ToolCallWire, PROTOCOL_VERSION,
};
use crate::subprocess::{SubprocessError, SubprocessRuntime};

/// How long we wait for a plugin event handler before assuming it is
/// wedged and treating the event as `Continue`. Matches the spec's
/// default `extension/*` timeout.
const EVENT_TIMEOUT: Duration = Duration::from_secs(5);

/// A placeholder used wherever AEP requires a `stateHandle` but Phase
/// 3 has not yet wired real host-state access. Plugins that try to
/// pass this to `host/state.*` will receive `METHOD_NOT_FOUND` from
/// `NoopHostHandler` — documented and intentional for this phase.
const PHASE3_STATE_PLACEHOLDER: &str = "phase3-placeholder-state";

/// A live plugin subprocess with a completed AEP handshake.
pub struct RemoteExtensionProxy {
    name: String,
    manifest: PluginManifest,
    init_result: InitializeResult,
    dispatcher: RpcDispatcher,
}

impl RemoteExtensionProxy {
    /// Spawn `manifest.entry` under `plugin_dir`, drive the handshake,
    /// and return a proxy ready to dispatch events to.
    pub async fn start(
        plugin_dir: PathBuf,
        manifest: PluginManifest,
    ) -> Result<Self, ProxyError> {
        let entry = plugin_dir.join(&manifest.entry);
        tracing::info!(
            plugin = %manifest.name,
            entry = %entry.display(),
            "starting plugin subprocess"
        );

        let runtime = SubprocessRuntime::spawn(
            &manifest.name,
            manifest.runtime,
            entry,
            Some(plugin_dir),
            None,
        )
        .await?;

        let host_handler = Arc::new(AlvaHostHandler::new(&manifest.name));
        let dispatcher = RpcDispatcher::spawn(runtime, host_handler);

        // --- initialize ---
        let init_params = InitializeParams {
            protocol_version: PROTOCOL_VERSION.to_string(),
            host_info: HostInfo {
                name: "alva".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            host_capabilities: HostCapabilities {
                state_access: vec!["messages".into(), "metadata".into()],
                events: vec![
                    "before_tool_call".into(),
                    "after_tool_call".into(),
                    "on_agent_start".into(),
                    "on_agent_end".into(),
                    "on_user_message".into(),
                ],
                host_api: vec![
                    "log".into(),
                    "notify".into(),
                    "emit_metric".into(),
                ],
            },
        };

        let init_value = dispatcher
            .call(
                methods::INITIALIZE,
                Some(serde_json::to_value(&init_params)?),
            )
            .await?;
        let init_result: InitializeResult = serde_json::from_value(init_value)?;
        tracing::info!(
            plugin = %manifest.name,
            declared = %init_result.plugin.name,
            subscriptions = ?init_result.event_subscriptions,
            "plugin initialized"
        );

        // --- initialized notification ---
        dispatcher.notify(methods::INITIALIZED, None).await?;

        Ok(Self {
            name: manifest.name.clone(),
            manifest,
            init_result,
            dispatcher,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    #[allow(dead_code)]
    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    #[allow(dead_code)]
    pub fn init_result(&self) -> &InitializeResult {
        &self.init_result
    }

    /// Does this plugin subscribe to this AEP event name? `name` is
    /// the bare event name (`"before_tool_call"`), not the full
    /// `extension/before_tool_call` method.
    pub fn subscribes_to(&self, name: &str) -> bool {
        self.init_result
            .event_subscriptions
            .iter()
            .any(|m| m == name)
    }

    /// Synchronous dispatch of an `ExtensionEvent` to this plugin,
    /// translating the response into an `EventResult`.
    ///
    /// Called from inside a sync extension handler that itself runs
    /// inside the tokio runtime. Uses `block_in_place` to bridge the
    /// async dispatcher.
    pub fn dispatch_event_sync(&self, event: &ExtensionEvent) -> EventResult {
        let Some(bare_name) = core_event_to_aep_name(event) else {
            return EventResult::Continue;
        };
        if !self.subscribes_to(bare_name) {
            return EventResult::Continue;
        }

        let Some(params) = core_event_to_params(event) else {
            tracing::error!(
                plugin = %self.name,
                event = bare_name,
                "failed to serialize event params"
            );
            return EventResult::Continue;
        };

        let method = format!("extension/{}", bare_name);
        let result = call_dispatcher_blocking(&self.dispatcher, &method, params);

        match result {
            Ok(value) => action_to_event_result(&self.name, bare_name, value),
            Err(DispatchError::Rpc(err)) => {
                tracing::warn!(
                    plugin = %self.name,
                    event = bare_name,
                    code = err.code,
                    message = %err.message,
                    "plugin returned rpc error"
                );
                EventResult::Continue
            }
            Err(e) => {
                tracing::error!(
                    plugin = %self.name,
                    event = bare_name,
                    error = %e,
                    "dispatch failed"
                );
                EventResult::Continue
            }
        }
    }

    /// Drive an orderly shutdown: best-effort `shutdown` call, then
    /// tear down the dispatcher (which reaps the subprocess).
    pub async fn shutdown(self) -> Result<(), ProxyError> {
        let plugin = self.name.clone();
        // Best-effort: plugin might already be gone.
        if let Err(e) = self.dispatcher.call(methods::SHUTDOWN, None).await {
            tracing::warn!(plugin = %plugin, error = %e, "shutdown rpc failed");
        }
        let status = self.dispatcher.shutdown().await?;
        tracing::info!(plugin = %plugin, ?status, "plugin subprocess exited");
        Ok(())
    }
}

// ===========================================================
// Bridging core ExtensionEvent ↔ AEP
// ===========================================================

/// Convert a core `ExtensionEvent` variant to the corresponding bare
/// AEP event name (no `extension/` prefix). Events the host supports
/// but AEP does not map get `None`.
fn core_event_to_aep_name(event: &ExtensionEvent) -> Option<&'static str> {
    match event {
        ExtensionEvent::BeforeToolCall { .. } => Some("before_tool_call"),
        ExtensionEvent::AfterToolCall { .. } => Some("after_tool_call"),
        ExtensionEvent::AgentStart => Some("on_agent_start"),
        ExtensionEvent::AgentEnd { .. } => Some("on_agent_end"),
        ExtensionEvent::Input { .. } => Some("on_user_message"),
    }
}

/// Serialize an `ExtensionEvent` into the AEP params JSON shape the
/// plugin expects.
///
/// Phase 3 uses [`PHASE3_STATE_PLACEHOLDER`] wherever the spec asks
/// for a `stateHandle`; real state handles land in a later phase
/// when we implement the host API handler side.
fn core_event_to_params(event: &ExtensionEvent) -> Option<Value> {
    let state_handle: StateHandle = PHASE3_STATE_PLACEHOLDER.to_string();
    match event {
        ExtensionEvent::BeforeToolCall {
            tool_name,
            tool_call_id,
            arguments,
        } => {
            let params = BeforeToolCallParams {
                state_handle,
                tool_call: ToolCallWire {
                    id: tool_call_id.clone(),
                    name: tool_name.clone(),
                    arguments: arguments.clone(),
                },
            };
            serde_json::to_value(&params).ok()
        }
        ExtensionEvent::AfterToolCall {
            tool_name,
            tool_call_id,
            result,
        } => {
            // No dedicated params struct yet — use an ad-hoc shape
            // that matches the spec. A typed struct will land when
            // we wire after_tool_call in Phase 4+.
            serde_json::to_value(serde_json::json!({
                "stateHandle": state_handle,
                "toolCall": {
                    "id": tool_call_id,
                    "name": tool_name,
                },
                "result": result,
            }))
            .ok()
        }
        ExtensionEvent::AgentStart => {
            serde_json::to_value(serde_json::json!({
                "stateHandle": state_handle,
            }))
            .ok()
        }
        ExtensionEvent::AgentEnd { error } => {
            serde_json::to_value(serde_json::json!({
                "stateHandle": state_handle,
                "error": error,
            }))
            .ok()
        }
        ExtensionEvent::Input { text } => {
            serde_json::to_value(serde_json::json!({
                "stateHandle": state_handle,
                "message": { "text": text },
            }))
            .ok()
        }
    }
}

/// Parse a plugin's JSON-RPC `result` into an `ExtensionAction` and
/// translate it to the host's `EventResult`. Unsupported action
/// variants are logged and treated as `Continue`.
fn action_to_event_result(plugin: &str, event: &str, value: Value) -> EventResult {
    let action: ExtensionAction = match serde_json::from_value(value.clone()) {
        Ok(a) => a,
        Err(e) => {
            tracing::error!(
                plugin = %plugin,
                event = event,
                error = %e,
                raw = %value,
                "plugin returned malformed action"
            );
            return EventResult::Continue;
        }
    };

    match action {
        ExtensionAction::Continue => EventResult::Continue,
        ExtensionAction::Block { reason } => EventResult::Block { reason },
        other => {
            // Modify / ReplaceResult / ModifyMessages / ModifyResponse /
            // ModifyResult — all land here. See module-level docs.
            tracing::warn!(
                plugin = %plugin,
                event = event,
                action = ?other,
                "plugin returned action not yet supported by host; treating as continue"
            );
            EventResult::Continue
        }
    }
}

/// Bridge a sync caller to the async dispatcher. Must be called from
/// within a multi-thread tokio runtime.
fn call_dispatcher_blocking(
    dispatcher: &RpcDispatcher,
    method: &str,
    params: Value,
) -> Result<Value, DispatchError> {
    let handle = match tokio::runtime::Handle::try_current() {
        Ok(h) => h,
        Err(_) => {
            return Err(DispatchError::ChannelClosed);
        }
    };

    tokio::task::block_in_place(|| {
        handle.block_on(async {
            match tokio::time::timeout(EVENT_TIMEOUT, dispatcher.call(method, Some(params))).await
            {
                Ok(result) => result,
                Err(_) => {
                    tracing::warn!(method = method, "plugin event timed out");
                    Err(DispatchError::ChannelClosed)
                }
            }
        })
    })
}

// ===========================================================
// Error
// ===========================================================

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("subprocess error: {0}")]
    Subprocess(#[from] SubprocessError),

    #[error("dispatcher error: {0}")]
    Dispatch(#[from] DispatchError),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}
