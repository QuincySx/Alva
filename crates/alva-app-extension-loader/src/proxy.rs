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
//! - **synchronously** dispatch an [`AepEvent`] to the plugin
//!   and translate the response into an [`AepDispatchResult`]
//!
//! ## Decoupled from agent-core's event layer
//!
//! This crate deliberately does **not** depend on agent-core's
//! `ExtensionEvent` / `EventResult` types. Those are the synchronous
//! mirror of the middleware hooks and are being removed. Instead the
//! loader defines its own light-weight [`AepEvent`] /
//! [`AepDispatchResult`] types (below) and the
//! [`AepBridgeMiddleware`](crate::aep_bridge::AepBridgeMiddleware)
//! translates real `Middleware` hooks into them.
//!
//! The synchronous dispatch is the tricky part: the middleware hooks
//! that call us are `async`, but each hook ultimately drives plugins
//! through this pure-sync entry point. We bridge to the async
//! dispatcher via `tokio::task::block_in_place` + a current-runtime
//! `block_on`. That requires the process to use a multi-thread tokio
//! runtime — the agent already does.
//!
//! ## Action coverage
//!
//! [`AepDispatchResult`] only has `Continue` and `Block`. AEP defines
//! seven `ExtensionAction` variants; we only honour `Continue` and
//! `Block` — anything else is logged and treated as `Continue`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use alva_kernel_abi::tool::execution::ToolOutput;
use serde_json::Value;

use crate::dispatcher::{DispatchError, RpcDispatcher};
use crate::host_api::AlvaHostHandler;
use crate::manifest::PluginManifest;
use crate::protocol::{
    methods, BeforeToolCallParams, ExtensionAction, HostCapabilities, HostInfo,
    InitializeParams, InitializeResult, StateHandle, ToolCallWire, PROTOCOL_VERSION,
};
use crate::subprocess::{SubprocessError, SubprocessRuntime};

// ===========================================================
// Loader-local event types (decoupled from agent-core)
// ===========================================================

/// A host event to dispatch to a plugin, owned entirely by this crate.
///
/// This is the loader's replacement for agent-core's `ExtensionEvent`:
/// it carries exactly what the five AEP subscriptions need, borrowing
/// from the caller (the [`AepBridgeMiddleware`](crate::aep_bridge)) so
/// no cloning happens on the hot path. Keeping it loader-local is what
/// lets agent-core delete its event layer without touching AEP.
#[derive(Debug)]
pub enum AepEvent<'a> {
    /// Agent run is starting (AEP `on_agent_start`).
    AgentStart,
    /// Agent run is ending (AEP `on_agent_end`).
    AgentEnd { error: Option<&'a str> },
    /// A tool is about to run (AEP `before_tool_call`). May be blocked.
    BeforeToolCall {
        tool_name: &'a str,
        tool_call_id: &'a str,
        arguments: &'a Value,
    },
    /// A tool finished running (AEP `after_tool_call`).
    AfterToolCall {
        tool_name: &'a str,
        tool_call_id: &'a str,
        result: &'a ToolOutput,
    },
    /// The latest user message text (AEP `on_user_message`).
    UserMessage { text: &'a str },
}

/// What a plugin decided after seeing an [`AepEvent`].
///
/// The loader's replacement for agent-core's `EventResult` — but only
/// the two variants AEP plugins can actually produce. `Block` is only
/// meaningful for `before_tool_call`; for every other event a `Block`
/// from a plugin is honoured by the bridge but has no effect beyond
/// surfacing the reason.
#[derive(Debug)]
pub enum AepDispatchResult {
    /// Proceed normally.
    Continue,
    /// Reject the operation that triggered the event.
    Block { reason: String },
}

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

    /// Synchronous dispatch of an [`AepEvent`] to this plugin,
    /// translating the response into an [`AepDispatchResult`].
    ///
    /// Called from inside an `async` middleware hook that itself runs
    /// inside the tokio runtime. Uses `block_in_place` to bridge the
    /// async dispatcher. Plugins that did not subscribe to the event,
    /// or that error / time out, are treated as `Continue`.
    pub fn dispatch_event_sync(&self, event: &AepEvent<'_>) -> AepDispatchResult {
        let bare_name = aep_event_name(event);
        if !self.subscribes_to(bare_name) {
            return AepDispatchResult::Continue;
        }

        let Some(params) = aep_event_to_params(event) else {
            tracing::error!(
                plugin = %self.name,
                event = bare_name,
                "failed to serialize event params"
            );
            return AepDispatchResult::Continue;
        };

        let method = format!("extension/{}", bare_name);
        let result = call_dispatcher_blocking(&self.dispatcher, &method, params);

        match result {
            Ok(value) => action_to_dispatch_result(&self.name, bare_name, value),
            Err(DispatchError::Rpc(err)) => {
                tracing::warn!(
                    plugin = %self.name,
                    event = bare_name,
                    code = err.code,
                    message = %err.message,
                    "plugin returned rpc error"
                );
                AepDispatchResult::Continue
            }
            Err(e) => {
                tracing::error!(
                    plugin = %self.name,
                    event = bare_name,
                    error = %e,
                    "dispatch failed"
                );
                AepDispatchResult::Continue
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
// Bridging AepEvent ↔ AEP wire
// ===========================================================

/// The bare AEP event name (no `extension/` prefix) for an
/// [`AepEvent`]. This is also the name plugins declare in their
/// `eventSubscriptions` list.
fn aep_event_name(event: &AepEvent<'_>) -> &'static str {
    match event {
        AepEvent::BeforeToolCall { .. } => "before_tool_call",
        AepEvent::AfterToolCall { .. } => "after_tool_call",
        AepEvent::AgentStart => "on_agent_start",
        AepEvent::AgentEnd { .. } => "on_agent_end",
        AepEvent::UserMessage { .. } => "on_user_message",
    }
}

/// Serialize an [`AepEvent`] into the AEP params JSON shape the plugin
/// expects.
///
/// Uses [`PHASE3_STATE_PLACEHOLDER`] wherever the spec asks for a
/// `stateHandle`; real state handles land in a later phase when we
/// implement the host API handler side.
fn aep_event_to_params(event: &AepEvent<'_>) -> Option<Value> {
    let state_handle: StateHandle = PHASE3_STATE_PLACEHOLDER.to_string();
    match event {
        AepEvent::BeforeToolCall {
            tool_name,
            tool_call_id,
            arguments,
        } => {
            let params = BeforeToolCallParams {
                state_handle,
                tool_call: ToolCallWire {
                    id: (*tool_call_id).to_string(),
                    name: (*tool_name).to_string(),
                    arguments: (*arguments).clone(),
                },
            };
            serde_json::to_value(&params).ok()
        }
        AepEvent::AfterToolCall {
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
        AepEvent::AgentStart => {
            serde_json::to_value(serde_json::json!({
                "stateHandle": state_handle,
            }))
            .ok()
        }
        AepEvent::AgentEnd { error } => {
            serde_json::to_value(serde_json::json!({
                "stateHandle": state_handle,
                "error": error,
            }))
            .ok()
        }
        AepEvent::UserMessage { text } => {
            serde_json::to_value(serde_json::json!({
                "stateHandle": state_handle,
                "message": { "text": text },
            }))
            .ok()
        }
    }
}

/// Parse a plugin's JSON-RPC `result` into an `ExtensionAction` and
/// translate it to an [`AepDispatchResult`]. Unsupported action
/// variants are logged and treated as `Continue`.
fn action_to_dispatch_result(plugin: &str, event: &str, value: Value) -> AepDispatchResult {
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
            return AepDispatchResult::Continue;
        }
    };

    match action {
        ExtensionAction::Continue => AepDispatchResult::Continue,
        ExtensionAction::Block { reason } => AepDispatchResult::Block { reason },
        other => {
            // Modify / ReplaceResult / ModifyMessages / ModifyResponse /
            // ModifyResult — all land here. See module-level docs.
            tracing::warn!(
                plugin = %plugin,
                event = event,
                action = ?other,
                "plugin returned action not yet supported by host; treating as continue"
            );
            AepDispatchResult::Continue
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

#[cfg(test)]
mod tests {
    //! Tests for proxy.rs pure-sync helpers — 7 tests covering 3
    //! contract families:
    //!
    //! 1. **`aep_event_name` wire-name translation** — [`AepEvent`]
    //!    variants map to AEP RPC method names. The non-obvious
    //!    asymmetries: `AgentStart`/`AgentEnd` carry an `"on_"` prefix
    //!    and `UserMessage` maps to `"on_user_message"`. A silent
    //!    change would rename every plugin RPC method — no
    //!    compile-time hint, every deployed plugin breaks. One
    //!    parametric test pins all 5 variants in one pass.
    //!
    //! 2. **`aep_event_to_params` wire JSON shape** — produces the
    //!    object plugins receive. Three simple shapes (AgentStart /
    //!    AgentEnd / UserMessage) merged into one test; the two complex
    //!    shapes (BeforeToolCall typed serialization, AfterToolCall
    //!    ad-hoc camelCase) kept as separate tests because their
    //!    assertion sets are substantially different.
    //!
    //! 3. **`action_to_dispatch_result` decision branches** — Continue
    //!    passthrough + Block reason propagation + a merged forward-
    //!    compat/defensive test covering unsupported variants AND
    //!    malformed JSON (both fall into the same `_ => Continue`
    //!    catch-all branch).
    use super::*;

    // -- aep_event_name: parametric over 5 variants --------------------

    #[test]
    fn aep_name_each_variant_maps_to_spec_wire_name() {
        // CRITICAL asymmetries: AgentStart/AgentEnd carry an "on_"
        // prefix; UserMessage maps to "on_user_message". The
        // parametric loop pins all 5 variants in one pass.
        let args = serde_json::json!({});
        let result = ToolOutput::text("");
        let cases: Vec<(AepEvent<'_>, &str)> = vec![
            (AepEvent::AgentStart, "on_agent_start"),
            (AepEvent::AgentEnd { error: None }, "on_agent_end"),
            (
                AepEvent::BeforeToolCall {
                    tool_name: "x",
                    tool_call_id: "id",
                    arguments: &args,
                },
                "before_tool_call",
            ),
            (
                AepEvent::AfterToolCall {
                    tool_name: "x",
                    tool_call_id: "id",
                    result: &result,
                },
                "after_tool_call",
            ),
            (AepEvent::UserMessage { text: "hi" }, "on_user_message"),
        ];
        for (event, expected) in cases {
            assert_eq!(
                aep_event_name(&event),
                expected,
                "wire name mismatch for event {event:?}"
            );
        }
    }

    // -- aep_event_to_params: 3 simple shapes merged + 2 complex kept --

    #[test]
    fn params_simple_shapes_for_agent_start_agent_end_and_user_message() {
        // AgentStart: only stateHandle. AgentEnd: stateHandle + error.
        // UserMessage: stateHandle + message.text (NOT bare text;
        // plugins read params.message.text per spec).
        let v = aep_event_to_params(&AepEvent::AgentStart).unwrap();
        assert_eq!(v["stateHandle"], serde_json::json!(PHASE3_STATE_PLACEHOLDER));
        assert_eq!(
            v.as_object().expect("must be an object").len(),
            1,
            "AgentStart payload must contain only stateHandle: {v}"
        );

        let v = aep_event_to_params(&AepEvent::AgentEnd {
            error: Some("oom"),
        })
        .unwrap();
        assert_eq!(v["stateHandle"], serde_json::json!(PHASE3_STATE_PLACEHOLDER));
        assert_eq!(v["error"], serde_json::json!("oom"));

        let v = aep_event_to_params(&AepEvent::UserMessage {
            text: "hello",
        })
        .unwrap();
        assert_eq!(v["stateHandle"], serde_json::json!(PHASE3_STATE_PLACEHOLDER));
        assert_eq!(v["message"]["text"], serde_json::json!("hello"));
    }

    #[test]
    fn params_before_tool_call_serializes_typed_params_shape() {
        // Pin: BeforeToolCall uses the typed BeforeToolCallParams
        // struct; verify the serialized JSON contains tool_call_id /
        // tool_name / arguments. A refactor that switched to an
        // ad-hoc shape would fail this test.
        let args = serde_json::json!({"cmd": "ls"});
        let ev = AepEvent::BeforeToolCall {
            tool_name: "shell",
            tool_call_id: "tc-1",
            arguments: &args,
        };
        let v = aep_event_to_params(&ev).unwrap();
        assert!(v.is_object(), "BeforeToolCall params must serialize to object: {v}");
        let serialized = v.to_string();
        assert!(serialized.contains("tc-1"), "tool_call_id must appear: {serialized}");
        assert!(serialized.contains("shell"), "tool_name must appear: {serialized}");
        assert!(serialized.contains("ls"), "argument value must appear: {serialized}");
    }

    #[test]
    fn params_after_tool_call_uses_camel_case_state_handle_and_includes_result() {
        // Pin: AfterToolCall uses an ad-hoc JSON (not a typed struct)
        // with camelCase keys (stateHandle / toolCall). This deviates
        // from BeforeToolCall's typed shape — comment in source notes
        // a typed struct will land later; pin current behavior.
        let result = ToolOutput::text("done");
        let ev = AepEvent::AfterToolCall {
            tool_name: "shell",
            tool_call_id: "tc-9",
            result: &result,
        };
        let v = aep_event_to_params(&ev).unwrap();
        assert_eq!(v["stateHandle"], serde_json::json!(PHASE3_STATE_PLACEHOLDER));
        assert_eq!(v["toolCall"]["id"], serde_json::json!("tc-9"));
        assert_eq!(v["toolCall"]["name"], serde_json::json!("shell"));
        assert!(v.get("result").is_some(), "result key required: {v}");
    }

    // -- action_to_dispatch_result: 3 decision branches ----------------

    #[test]
    fn action_continue_passes_through_to_dispatch_result_continue() {
        let v = serde_json::json!({"action": "continue"});
        let r = action_to_dispatch_result("p", "on_agent_start", v);
        assert!(matches!(r, AepDispatchResult::Continue));
    }

    #[test]
    fn action_block_propagates_reason_verbatim() {
        // Pin: Block.reason MUST round-trip — that string surfaces to
        // the user (e.g. "tool blocked: rate limit"). Losing it would
        // leave the user staring at a bare "blocked".
        let v = serde_json::json!({"action": "block", "reason": "rate limit"});
        let r = action_to_dispatch_result("p", "before_tool_call", v);
        match r {
            AepDispatchResult::Block { reason } => assert_eq!(reason, "rate limit"),
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn action_catch_all_continues_for_unsupported_variants_and_malformed_input() {
        // Both forward-compat (unsupported Modify variant defined in
        // protocol but not implemented) AND defensive (malformed JSON
        // that doesn't deserialize as ExtensionAction) fall into the
        // same `_ => AepDispatchResult::Continue` catch-all branch. A
        // refactor that panicked or Blocked here would crash hosts
        // when a new-protocol plugin shipped, OR when a buggy plugin
        // sent garbage.
        let modify = serde_json::json!({
            "action": "modify",
            "modified_arguments": {"k": "v"}
        });
        assert!(
            matches!(action_to_dispatch_result("p", "before_tool_call", modify), AepDispatchResult::Continue),
            "unsupported Modify variant must downgrade to Continue, not crash"
        );

        let malformed = serde_json::json!({"this": "is not an ExtensionAction"});
        assert!(
            matches!(action_to_dispatch_result("p", "after_tool_call", malformed), AepDispatchResult::Continue),
            "malformed action JSON must defensively Continue"
        );
    }
}
