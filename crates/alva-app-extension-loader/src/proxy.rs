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

#[cfg(test)]
mod tests {
    //! Tests for proxy.rs pure-sync helpers — 7 tests covering 3
    //! contract families:
    //!
    //! 1. **`core_event_to_aep_name` wire-name translation** —
    //!    `ExtensionEvent` variants map to AEP RPC method names that
    //!    DIFFER from `ExtensionEvent::event_type()`. e.g.
    //!    `AgentStart` → AEP `"on_agent_start"` but event_type
    //!    returns `"agent_start"`; `Input` → `"on_user_message"` not
    //!    `"input"`. A silent change to use event_type() would rename
    //!    every plugin RPC method — no compile-time hint, every
    //!    deployed plugin breaks. One parametric test pins all 5
    //!    variants in one pass.
    //!
    //! 2. **`core_event_to_params` wire JSON shape** — produces the
    //!    object plugins receive. Three simple shapes (AgentStart /
    //!    AgentEnd / Input) merged into one test; the two complex
    //!    shapes (BeforeToolCall typed serialization, AfterToolCall
    //!    ad-hoc camelCase) kept as separate tests because their
    //!    assertion sets are substantially different.
    //!
    //! 3. **`action_to_event_result` decision branches** — Continue
    //!    passthrough + Block reason propagation + a merged forward-
    //!    compat/defensive test covering unsupported variants AND
    //!    malformed JSON (both fall into the same `_ => Continue`
    //!    catch-all branch).
    //!
    //! Removed: EVENT_TIMEOUT and PHASE3_STATE_PLACEHOLDER literal
    //! pins — both are internal consts with no external spec; the
    //! PHASE3 placeholder is implicitly verified by the shape tests
    //! (which assert it appears in the wire payload via the source
    //! const, so renaming the const breaks the shape tests).
    use super::*;
    use alva_kernel_abi::tool::execution::ToolOutput;

    // -- core_event_to_aep_name: parametric over 5 variants -------------

    #[test]
    fn aep_name_each_variant_maps_to_spec_wire_name() {
        // CRITICAL asymmetries: AgentStart/AgentEnd carry an "on_"
        // prefix that event_type() doesn't; Input maps to
        // "on_user_message" (NOT "input"). Either silent change
        // would rename plugin RPC methods. The parametric loop
        // pins all 5 variants in one pass.
        let cases: Vec<(ExtensionEvent, &str)> = vec![
            (ExtensionEvent::AgentStart, "on_agent_start"),
            (ExtensionEvent::AgentEnd { error: None }, "on_agent_end"),
            (
                ExtensionEvent::BeforeToolCall {
                    tool_name: "x".into(),
                    tool_call_id: "id".into(),
                    arguments: serde_json::json!({}),
                },
                "before_tool_call",
            ),
            (
                ExtensionEvent::AfterToolCall {
                    tool_name: "x".into(),
                    tool_call_id: "id".into(),
                    result: ToolOutput::text(""),
                },
                "after_tool_call",
            ),
            (ExtensionEvent::Input { text: "hi".into() }, "on_user_message"),
        ];
        for (event, expected) in cases {
            assert_eq!(
                core_event_to_aep_name(&event),
                Some(expected),
                "wire name mismatch for event {event:?}"
            );
        }
    }

    // -- core_event_to_params: 3 simple shapes merged + 2 complex kept --

    #[test]
    fn params_simple_shapes_for_agent_start_agent_end_and_input() {
        // AgentStart: only stateHandle. AgentEnd: stateHandle + error.
        // Input: stateHandle + message.text (NOT bare text; plugins
        // read params.message.text per spec).
        let v = core_event_to_params(&ExtensionEvent::AgentStart).unwrap();
        assert_eq!(v["stateHandle"], serde_json::json!(PHASE3_STATE_PLACEHOLDER));
        assert_eq!(
            v.as_object().expect("must be an object").len(),
            1,
            "AgentStart payload must contain only stateHandle: {v}"
        );

        let v = core_event_to_params(&ExtensionEvent::AgentEnd {
            error: Some("oom".into()),
        })
        .unwrap();
        assert_eq!(v["stateHandle"], serde_json::json!(PHASE3_STATE_PLACEHOLDER));
        assert_eq!(v["error"], serde_json::json!("oom"));

        let v = core_event_to_params(&ExtensionEvent::Input {
            text: "hello".into(),
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
        let ev = ExtensionEvent::BeforeToolCall {
            tool_name: "shell".into(),
            tool_call_id: "tc-1".into(),
            arguments: serde_json::json!({"cmd": "ls"}),
        };
        let v = core_event_to_params(&ev).unwrap();
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
        let ev = ExtensionEvent::AfterToolCall {
            tool_name: "shell".into(),
            tool_call_id: "tc-9".into(),
            result: ToolOutput::text("done"),
        };
        let v = core_event_to_params(&ev).unwrap();
        assert_eq!(v["stateHandle"], serde_json::json!(PHASE3_STATE_PLACEHOLDER));
        assert_eq!(v["toolCall"]["id"], serde_json::json!("tc-9"));
        assert_eq!(v["toolCall"]["name"], serde_json::json!("shell"));
        assert!(v.get("result").is_some(), "result key required: {v}");
    }

    // -- action_to_event_result: 3 decision branches --------------------

    #[test]
    fn action_continue_passes_through_to_event_result_continue() {
        let v = serde_json::json!({"action": "continue"});
        let r = action_to_event_result("p", "on_agent_start", v);
        assert!(matches!(r, EventResult::Continue));
    }

    #[test]
    fn action_block_propagates_reason_verbatim() {
        // Pin: Block.reason MUST round-trip — that string surfaces to
        // the user (e.g. "tool blocked: rate limit"). Losing it would
        // leave the user staring at a bare "blocked".
        let v = serde_json::json!({"action": "block", "reason": "rate limit"});
        let r = action_to_event_result("p", "before_tool_call", v);
        match r {
            EventResult::Block { reason } => assert_eq!(reason, "rate limit"),
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn action_catch_all_continues_for_unsupported_variants_and_malformed_input() {
        // Both forward-compat (unsupported Modify variant defined in
        // protocol but not implemented) AND defensive (malformed JSON
        // that doesn't deserialize as ExtensionAction) fall into the
        // same `_ => EventResult::Continue` catch-all branch. A
        // refactor that panicked or Blocked here would crash hosts
        // when a new-protocol plugin shipped, OR when a buggy plugin
        // sent garbage.
        let modify = serde_json::json!({
            "action": "modify",
            "modified_arguments": {"k": "v"}
        });
        assert!(
            matches!(action_to_event_result("p", "before_tool_call", modify), EventResult::Continue),
            "unsupported Modify variant must downgrade to Continue, not crash"
        );

        let malformed = serde_json::json!({"this": "is not an ExtensionAction"});
        assert!(
            matches!(action_to_event_result("p", "after_tool_call", malformed), EventResult::Continue),
            "malformed action JSON must defensively Continue"
        );
    }
}
