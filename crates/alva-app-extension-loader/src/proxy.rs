// INPUT:  plugin_bundle::StagedPluginBundle, subprocess::SubprocessRuntime, dispatcher::RpcDispatcher,
//         protocol::{InitializeParams, InitializeResult, ExtensionAction, BeforeToolCallParams, ...}
// OUTPUT: RemotePluginProxy, ProxyError
// POS:    One capability-gated subprocess proxy executing from an immutable owner-only bundle snapshot.

//! A single dynamically-loaded plugin, wrapped so it can participate
//! in the host's phase/event dispatch machinery.
//!
//! One [`RemotePluginProxy`] owns one subprocess and one
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
//! [`AepPhaseHandler`](crate::aep_bridge::AepPhaseHandler)
//! translates executable phase subscriptions into current middleware hooks.
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
//! [`AepDispatchResult`] represents the subset of AEP actions the host
//! currently honours: `Continue`, `Block`, tool argument/result
//! mutation, and LLM request/response mutation.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use alva_kernel_abi::tool::execution::ToolOutput;
use alva_kernel_abi::Message;
use serde_json::Value;

use crate::dispatcher::{DispatchError, RpcDispatcher};
use crate::host_api::{AlvaHostHandler, StateSnapshot};
use crate::manifest::PluginManifest;
use crate::plugin_bundle::{PluginBundleError, StagedPluginBundle};
use crate::protocol::{
    methods, BeforeToolCallParams, ExtensionAction, HostCapabilities, HostInfo, InitializeParams,
    InitializeResult, StateHandle, ToolCallWire, PROTOCOL_VERSION,
};
use crate::subprocess::{SubprocessError, SubprocessRuntime};

// ===========================================================
// Loader-local event types (decoupled from agent-core)
// ===========================================================

/// A host event to dispatch to a plugin, owned entirely by this crate.
///
/// This is the loader's replacement for agent-core's `ExtensionEvent`:
/// it carries exactly what AEP subscriptions need, borrowing
/// from the caller (the AEP phase handler). Keeping it loader-local is what
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
    /// The LLM request is about to be sent (AEP `on_llm_call_start`).
    LlmCallStart { messages: &'a [Message] },
    /// The LLM response has been received (AEP `on_llm_call_end`).
    LlmCallEnd { response: &'a Message },
    /// The latest user message text (AEP `on_user_message`).
    UserMessage { text: &'a str },
}

/// What a plugin decided after seeing an [`AepEvent`].
#[derive(Debug)]
pub enum AepDispatchResult {
    /// Proceed normally.
    Continue,
    /// Reject the operation that triggered the event.
    Block { reason: String },
    /// Replace before-tool-call arguments before executing the tool.
    ModifyToolArguments { arguments: Value },
    /// Skip tool execution and use this result.
    ReplaceResult { result: ToolOutput },
    /// Replace the LLM-bound message list.
    ModifyMessages { messages: Vec<Message> },
    /// Replace the LLM response message.
    ModifyResponse { response: Message },
    /// Replace a completed tool result.
    ModifyResult { result: ToolOutput },
    /// Plugin returned a syntactically invalid or event-illegal action.
    ///
    /// Agent middleware treats this as non-blocking, but CLI/debug callers can
    /// surface the reason instead of losing it as a plain Continue.
    InvalidAction { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AepActionKind {
    Continue,
    Block,
    Modify,
    ReplaceResult,
    ModifyMessages,
    ModifyResponse,
    ModifyResult,
}

impl AepActionKind {
    fn name(self) -> &'static str {
        match self {
            Self::Continue => "continue",
            Self::Block => "block",
            Self::Modify => "modify",
            Self::ReplaceResult => "replace_result",
            Self::ModifyMessages => "modify_messages",
            Self::ModifyResponse => "modify_response",
            Self::ModifyResult => "modify_result",
        }
    }
}

fn legal_actions_for_event(event: &str) -> &'static [AepActionKind] {
    use AepActionKind::*;

    match event {
        "before_tool_call" => &[Continue, Block, Modify, ReplaceResult],
        "after_tool_call" => &[Continue, ModifyResult],
        "on_llm_call_start" => &[Continue, ModifyMessages, Block],
        "on_llm_call_end" => &[Continue, ModifyResponse],
        "on_user_message" => &[Continue],
        "on_agent_start" => &[Continue, Block],
        "on_agent_end" => &[Continue],
        _ => &[],
    }
}

fn invalid_action(event: &str, kind: AepActionKind) -> AepDispatchResult {
    AepDispatchResult::InvalidAction {
        reason: format!("{} is not legal for {event}", kind.name()),
    }
}

fn is_legal_action(event: &str, kind: AepActionKind) -> bool {
    legal_actions_for_event(event).contains(&kind)
}

/// How long we wait for a plugin event handler before assuming it is
/// wedged and treating the event as `Continue`. Matches the spec's
/// default `extension/*` timeout.
const EVENT_TIMEOUT: Duration = Duration::from_secs(5);
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
const TOOL_CALL_TIMEOUT: Duration = Duration::from_secs(120);
const SHUTDOWN_RPC_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_REMOTE_TOOLS: usize = 128;
const MAX_TOOL_NAME_BYTES: usize = 128;
const MAX_TOOL_DESCRIPTION_BYTES: usize = 8 * 1024;
const MAX_TOOL_SCHEMA_BYTES: usize = 256 * 1024;
const SUPPORTED_EVENT_SUBSCRIPTIONS: &[&str] = &[
    "before_tool_call",
    "after_tool_call",
    "on_llm_call_start",
    "on_llm_call_end",
    "on_agent_start",
    "on_agent_end",
    "on_user_message",
];

/// Event-scoped state handle used in AEP params. The host refreshes
/// the snapshot behind this handle immediately before dispatching
/// each event.
pub(crate) const AEP_STATE_HANDLE: &str = "current-event-state";

/// A live plugin subprocess with a completed AEP handshake.
pub struct RemotePluginProxy {
    name: String,
    manifest: PluginManifest,
    init_result: InitializeResult,
    dispatcher: RpcDispatcher,
    host_handler: Arc<AlvaHostHandler>,
    staged_bundle: StagedPluginBundle,
}

impl RemotePluginProxy {
    /// Spawn `manifest.entry` under `plugin_dir`, drive the handshake,
    /// and return a proxy ready to dispatch events to.
    pub async fn start(plugin_dir: PathBuf, manifest: PluginManifest) -> Result<Self, ProxyError> {
        let staged_bundle = StagedPluginBundle::stage(plugin_dir, manifest.clone()).await?;
        let entry = staged_bundle.entry().to_path_buf();
        let staged_root = staged_bundle.root().to_path_buf();
        tracing::info!(
            plugin = %manifest.name,
            entry = %entry.display(),
            content_sha256 = %staged_bundle.content_sha256(),
            "starting plugin subprocess"
        );

        let runtime = SubprocessRuntime::spawn(
            &manifest.name,
            manifest.runtime,
            entry,
            Some(staged_root),
            None,
        )
        .await?;

        let host_handler = Arc::new(AlvaHostHandler::with_capabilities(
            manifest.name.clone(),
            manifest.requested_capabilities.clone(),
        ));
        let dispatcher = RpcDispatcher::spawn(runtime, host_handler.clone());

        // --- initialize ---
        let init_params = InitializeParams {
            protocol_version: PROTOCOL_VERSION.to_string(),
            host_info: HostInfo {
                name: "alva".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            host_capabilities: HostCapabilities {
                state_access: vec!["messages".into(), "metadata".into()],
                events: SUPPORTED_EVENT_SUBSCRIPTIONS
                    .iter()
                    .map(|event| (*event).to_string())
                    .collect(),
                host_api: vec![
                    "log".into(),
                    "notify".into(),
                    "emit_metric".into(),
                    "state.get_messages".into(),
                    "state.get_metadata".into(),
                    "state.count_tokens".into(),
                ],
            },
        };

        let init_value = tokio::time::timeout(
            INITIALIZE_TIMEOUT,
            dispatcher.call(
                methods::INITIALIZE,
                Some(serde_json::to_value(&init_params)?),
            ),
        )
        .await
        .map_err(|_| ProxyError::Timeout {
            operation: "initialize",
            seconds: INITIALIZE_TIMEOUT.as_secs(),
        })??;
        let init_result: InitializeResult = serde_json::from_value(init_value)?;
        validate_initialize_result(&manifest, &init_result)?;
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
            host_handler,
            staged_bundle,
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

    /// Digest of the immutable bundle snapshot this process is executing.
    pub fn content_sha256(&self) -> &str {
        self.staged_bundle.content_sha256()
    }

    pub fn update_state_snapshot(&self, messages: Vec<Message>, metadata: Value) {
        self.host_handler.set_state_snapshot(StateSnapshot {
            handle: AEP_STATE_HANDLE.to_string(),
            messages,
            metadata,
        });
    }

    pub fn clear_state_snapshot(&self) {
        self.host_handler.clear_state_snapshot();
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

        dispatch_outcome(&self.name, bare_name, result)
    }

    /// Call one tool declared by this plugin during `initialize`.
    pub async fn call_tool(
        &self,
        tool_name: &str,
        arguments: Value,
    ) -> Result<ToolOutput, ProxyError> {
        let value = tokio::time::timeout(
            TOOL_CALL_TIMEOUT,
            self.dispatcher.call(
                methods::TOOLS_CALL,
                Some(serde_json::json!({
                    "name": tool_name,
                    "arguments": arguments,
                })),
            ),
        )
        .await
        .map_err(|_| ProxyError::Timeout {
            operation: "tools/call",
            seconds: TOOL_CALL_TIMEOUT.as_secs(),
        })??;
        parse_tool_output(value)
    }

    /// Drive an orderly shutdown: best-effort `shutdown` call, then
    /// tear down the dispatcher (which reaps the subprocess).
    pub async fn shutdown(self) -> Result<(), ProxyError> {
        let plugin = self.name.clone();
        // Best-effort: plugin might already be gone.
        match tokio::time::timeout(
            SHUTDOWN_RPC_TIMEOUT,
            self.dispatcher.call(methods::SHUTDOWN, None),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => {
                tracing::warn!(plugin = %plugin, %error, "shutdown rpc failed");
            }
            Err(_) => {
                tracing::warn!(
                    plugin = %plugin,
                    timeout_seconds = SHUTDOWN_RPC_TIMEOUT.as_secs(),
                    "shutdown rpc timed out"
                );
            }
        }
        let status = self.dispatcher.shutdown().await?;
        tracing::info!(plugin = %plugin, ?status, "plugin subprocess exited");
        Ok(())
    }
}

fn validate_initialize_result(
    manifest: &PluginManifest,
    result: &InitializeResult,
) -> Result<(), ProxyError> {
    if result.protocol_version != PROTOCOL_VERSION {
        return Err(ProxyError::InvalidInitialize(format!(
            "protocol version mismatch: host supports {PROTOCOL_VERSION}, plugin returned {}",
            result.protocol_version
        )));
    }
    if result.plugin.name != manifest.name || result.plugin.version != manifest.version {
        return Err(ProxyError::InvalidInitialize(format!(
            "plugin identity mismatch: manifest is {}@{}, initialize returned {}@{}",
            manifest.name, manifest.version, result.plugin.name, result.plugin.version
        )));
    }

    let declared_capabilities: HashSet<&str> = manifest
        .requested_capabilities
        .iter()
        .map(String::as_str)
        .collect();
    let returned_capabilities: HashSet<&str> = result
        .requested_capabilities
        .iter()
        .map(String::as_str)
        .collect();
    if returned_capabilities.len() != result.requested_capabilities.len() {
        return Err(ProxyError::InvalidInitialize(
            "initialize returned duplicate requested capabilities".to_string(),
        ));
    }
    if returned_capabilities != declared_capabilities {
        return Err(ProxyError::InvalidInitialize(
            "initialize requestedCapabilities must exactly match the validated manifest"
                .to_string(),
        ));
    }

    if result.event_subscriptions.len() > SUPPORTED_EVENT_SUBSCRIPTIONS.len() {
        return Err(ProxyError::InvalidInitialize(format!(
            "plugin declared more than {} event subscriptions",
            SUPPORTED_EVENT_SUBSCRIPTIONS.len()
        )));
    }
    let mut subscriptions = HashSet::new();
    for subscription in &result.event_subscriptions {
        if !SUPPORTED_EVENT_SUBSCRIPTIONS.contains(&subscription.as_str()) {
            return Err(ProxyError::InvalidInitialize(format!(
                "unsupported event subscription: {subscription}"
            )));
        }
        if !subscriptions.insert(subscription.as_str()) {
            return Err(ProxyError::InvalidInitialize(format!(
                "duplicate event subscription: {subscription}"
            )));
        }
    }

    if result.tools.len() > MAX_REMOTE_TOOLS {
        return Err(ProxyError::InvalidInitialize(format!(
            "plugin declared more than {MAX_REMOTE_TOOLS} tools"
        )));
    }
    let mut tool_names = HashSet::new();
    for tool in &result.tools {
        if tool.name.is_empty()
            || tool.name.len() > MAX_TOOL_NAME_BYTES
            || !tool
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(ProxyError::InvalidInitialize(format!(
                "invalid remote tool name: {:?}",
                tool.name
            )));
        }
        if !tool_names.insert(tool.name.as_str()) {
            return Err(ProxyError::InvalidInitialize(format!(
                "duplicate remote tool name: {}",
                tool.name
            )));
        }
        if tool.description.len() > MAX_TOOL_DESCRIPTION_BYTES {
            return Err(ProxyError::InvalidInitialize(format!(
                "remote tool '{}' description exceeds {MAX_TOOL_DESCRIPTION_BYTES} bytes",
                tool.name
            )));
        }
        if !tool.input_schema.is_object() {
            return Err(ProxyError::InvalidInitialize(format!(
                "remote tool '{}' inputSchema must be a JSON object",
                tool.name
            )));
        }
        let schema_size = serde_json::to_vec(&tool.input_schema)?.len();
        if schema_size > MAX_TOOL_SCHEMA_BYTES {
            return Err(ProxyError::InvalidInitialize(format!(
                "remote tool '{}' inputSchema exceeds {MAX_TOOL_SCHEMA_BYTES} bytes",
                tool.name
            )));
        }
    }

    Ok(())
}

fn parse_tool_output(mut value: Value) -> Result<ToolOutput, ProxyError> {
    if let Some(obj) = value.as_object_mut() {
        if let Some(is_error) = obj.remove("isError") {
            obj.entry("is_error".to_string()).or_insert(is_error);
        }
    }
    match serde_json::from_value::<ToolOutput>(value.clone()) {
        Ok(output) => Ok(output),
        Err(_) => Ok(ToolOutput::text(value.to_string())),
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
        AepEvent::LlmCallStart { .. } => "on_llm_call_start",
        AepEvent::LlmCallEnd { .. } => "on_llm_call_end",
        AepEvent::UserMessage { .. } => "on_user_message",
    }
}

/// Serialize an [`AepEvent`] into the AEP params JSON shape the plugin
/// expects.
///
/// Uses [`AEP_STATE_HANDLE`] wherever the spec asks for a `stateHandle`.
fn aep_event_to_params(event: &AepEvent<'_>) -> Option<Value> {
    let state_handle: StateHandle = AEP_STATE_HANDLE.to_string();
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
        AepEvent::AgentStart => serde_json::to_value(serde_json::json!({
            "stateHandle": state_handle,
        }))
        .ok(),
        AepEvent::AgentEnd { error } => serde_json::to_value(serde_json::json!({
            "stateHandle": state_handle,
            "error": error,
        }))
        .ok(),
        AepEvent::LlmCallStart { messages } => serde_json::to_value(serde_json::json!({
            "stateHandle": state_handle,
            "messages": messages,
        }))
        .ok(),
        AepEvent::LlmCallEnd { response } => serde_json::to_value(serde_json::json!({
            "stateHandle": state_handle,
            "response": response,
        }))
        .ok(),
        AepEvent::UserMessage { text } => serde_json::to_value(serde_json::json!({
            "stateHandle": state_handle,
            "message": { "text": text },
        }))
        .ok(),
    }
}

/// Parse a plugin's JSON-RPC `result` into an `ExtensionAction` and
/// translate it to an [`AepDispatchResult`].
fn action_to_dispatch_result(plugin: &str, event: &str, value: Value) -> AepDispatchResult {
    let action: ExtensionAction = match serde_json::from_value(value.clone()) {
        Ok(a) => a,
        Err(e) => {
            let reason = format!("malformed action for {event}: {e}");
            tracing::error!(
                plugin = %plugin,
                event = event,
                error = %e,
                raw = %value,
                "plugin returned malformed action"
            );
            return AepDispatchResult::InvalidAction { reason };
        }
    };

    match action {
        ExtensionAction::Continue => AepDispatchResult::Continue,
        ExtensionAction::Block { reason } => {
            if !is_legal_action(event, AepActionKind::Block) {
                tracing::warn!(
                    plugin = %plugin,
                    event = event,
                    "plugin returned block for unsupported event"
                );
                return invalid_action(event, AepActionKind::Block);
            }
            AepDispatchResult::Block { reason }
        }
        ExtensionAction::Modify { modified_arguments } => {
            if !is_legal_action(event, AepActionKind::Modify) {
                tracing::warn!(
                    plugin = %plugin,
                    event = event,
                    "plugin returned modify for unsupported event"
                );
                return invalid_action(event, AepActionKind::Modify);
            }
            AepDispatchResult::ModifyToolArguments {
                arguments: modified_arguments,
            }
        }
        ExtensionAction::ReplaceResult { result } => {
            if !is_legal_action(event, AepActionKind::ReplaceResult) {
                tracing::warn!(
                    plugin = %plugin,
                    event = event,
                    "plugin returned replace_result for unsupported event"
                );
                return invalid_action(event, AepActionKind::ReplaceResult);
            }
            match parse_tool_output(result) {
                Ok(result) => AepDispatchResult::ReplaceResult { result },
                Err(e) => {
                    let reason = format!("malformed replace_result payload for {event}: {e}");
                    tracing::warn!(
                        plugin = %plugin,
                        event = event,
                        error = %e,
                        "plugin returned malformed replace_result payload"
                    );
                    AepDispatchResult::InvalidAction { reason }
                }
            }
        }
        ExtensionAction::ModifyMessages { messages } => {
            if !is_legal_action(event, AepActionKind::ModifyMessages) {
                tracing::warn!(
                    plugin = %plugin,
                    event = event,
                    "plugin returned modify_messages for unsupported event"
                );
                return invalid_action(event, AepActionKind::ModifyMessages);
            }
            match serde_json::from_value::<Vec<Message>>(messages) {
                Ok(messages) => AepDispatchResult::ModifyMessages { messages },
                Err(e) => {
                    let reason = format!("malformed modify_messages payload for {event}: {e}");
                    tracing::warn!(
                        plugin = %plugin,
                        event = event,
                        error = %e,
                        "plugin returned malformed modify_messages payload"
                    );
                    AepDispatchResult::InvalidAction { reason }
                }
            }
        }
        ExtensionAction::ModifyResponse { response } => {
            if !is_legal_action(event, AepActionKind::ModifyResponse) {
                tracing::warn!(
                    plugin = %plugin,
                    event = event,
                    "plugin returned modify_response for unsupported event"
                );
                return invalid_action(event, AepActionKind::ModifyResponse);
            }
            match serde_json::from_value::<Message>(response) {
                Ok(response) => AepDispatchResult::ModifyResponse { response },
                Err(e) => {
                    let reason = format!("malformed modify_response payload for {event}: {e}");
                    tracing::warn!(
                        plugin = %plugin,
                        event = event,
                        error = %e,
                        "plugin returned malformed modify_response payload"
                    );
                    AepDispatchResult::InvalidAction { reason }
                }
            }
        }
        ExtensionAction::ModifyResult { result } => {
            if !is_legal_action(event, AepActionKind::ModifyResult) {
                tracing::warn!(
                    plugin = %plugin,
                    event = event,
                    "plugin returned modify_result for unsupported event"
                );
                return invalid_action(event, AepActionKind::ModifyResult);
            }
            match parse_tool_output(result) {
                Ok(result) => AepDispatchResult::ModifyResult { result },
                Err(e) => {
                    let reason = format!("malformed modify_result payload for {event}: {e}");
                    tracing::warn!(
                        plugin = %plugin,
                        event = event,
                        error = %e,
                        "plugin returned malformed modify_result payload"
                    );
                    AepDispatchResult::InvalidAction { reason }
                }
            }
        }
    }
}

/// Translate the raw dispatcher result into an [`AepDispatchResult`].
///
/// **Fail-open**: transport-level failures — RPC error, timeout
/// (surfaced as `ChannelClosed`), channel closure, serialization failure —
/// map to `Continue`. A wedged or crashed plugin therefore never blocks a
/// tool; AEP plugins are an *enhancement* layer, not an authoritative
/// security gate (the built-in SECURITY-tier middleware is). Plugin-authored
/// malformed/illegal actions are represented as `InvalidAction` so CLI/debug
/// can surface them, while the agent loop still treats them as non-blocking.
fn dispatch_outcome(
    plugin: &str,
    event: &str,
    result: Result<Value, DispatchError>,
) -> AepDispatchResult {
    match result {
        Ok(value) => action_to_dispatch_result(plugin, event, value),
        Err(DispatchError::Rpc(err)) => {
            tracing::warn!(
                plugin = %plugin,
                event = event,
                code = err.code,
                message = %err.message,
                "plugin returned rpc error; failing open (continue)"
            );
            AepDispatchResult::Continue
        }
        Err(e) => {
            tracing::error!(
                plugin = %plugin,
                event = event,
                error = %e,
                "dispatch failed; failing open (continue)"
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
            match tokio::time::timeout(EVENT_TIMEOUT, dispatcher.call(method, Some(params))).await {
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
    #[error("plugin bundle error: {0}")]
    Bundle(#[from] PluginBundleError),

    #[error("subprocess error: {0}")]
    Subprocess(#[from] SubprocessError),

    #[error("dispatcher error: {0}")]
    Dispatch(#[from] DispatchError),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("plugin initialize contract rejected: {0}")]
    InvalidInitialize(String),

    #[error("plugin {operation} timed out after {seconds}s")]
    Timeout {
        operation: &'static str,
        seconds: u64,
    },
}

#[cfg(test)]
mod tests {
    //! Tests for proxy.rs helpers and timeout contracts.
    //! contract families:
    //!
    //! 1. **`aep_event_name` wire-name translation** — [`AepEvent`]
    //!    variants map to AEP RPC method names. The non-obvious
    //!    asymmetries: `AgentStart`/`AgentEnd` carry an `"on_"` prefix
    //!    and `UserMessage` maps to `"on_user_message"`. A silent
    //!    change would rename every plugin RPC method — no
    //!    compile-time hint, every deployed plugin breaks. One
    //!    parametric test pins all 7 variants in one pass.
    //!
    //! 2. **`aep_event_to_params` wire JSON shape** — produces the
    //!    object plugins receive. Three simple shapes (AgentStart /
    //!    AgentEnd / UserMessage) merged into one test; the two complex
    //!    shapes (BeforeToolCall typed serialization, AfterToolCall
    //!    ad-hoc camelCase) kept as separate tests because their
    //!    assertion sets are substantially different.
    //!
    //! 3. **`action_to_dispatch_result` decision branches** — Continue
    //!    passthrough + Block reason propagation + LLM mutation parsing
    //!    + a merged forward-compat/defensive test covering unsupported
    //!      variants AND malformed JSON.
    use super::*;
    use crate::manifest::Runtime;
    use crate::protocol::{PluginInfo, ToolDef};

    fn handshake_manifest() -> PluginManifest {
        PluginManifest {
            name: "example-plugin".to_string(),
            version: "1.2.3".to_string(),
            description: None,
            author: None,
            runtime: Runtime::Python,
            entry: "main.py".to_string(),
            requested_capabilities: vec!["host:log".to_string()],
            content_sha256: None,
        }
    }

    fn handshake_result() -> InitializeResult {
        InitializeResult {
            protocol_version: PROTOCOL_VERSION.to_string(),
            plugin: PluginInfo {
                name: "example-plugin".to_string(),
                version: "1.2.3".to_string(),
                description: None,
                author: None,
            },
            tools: vec![ToolDef {
                name: "remote_echo".to_string(),
                description: "Echo a value".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
            }],
            event_subscriptions: vec!["before_tool_call".to_string()],
            requested_capabilities: vec!["host:log".to_string()],
        }
    }

    #[test]
    fn initialize_contract_accepts_matching_bounded_declarations() {
        validate_initialize_result(&handshake_manifest(), &handshake_result()).unwrap();
    }

    #[test]
    fn initialize_contract_rejects_protocol_identity_and_capability_drift() {
        let manifest = handshake_manifest();

        let mut result = handshake_result();
        result.protocol_version = "99.0.0".to_string();
        assert!(validate_initialize_result(&manifest, &result)
            .unwrap_err()
            .to_string()
            .contains("protocol version mismatch"));

        let mut result = handshake_result();
        result.plugin.name = "impersonator".to_string();
        assert!(validate_initialize_result(&manifest, &result)
            .unwrap_err()
            .to_string()
            .contains("identity mismatch"));

        let mut result = handshake_result();
        result.requested_capabilities = vec!["host:notify".to_string()];
        assert!(validate_initialize_result(&manifest, &result)
            .unwrap_err()
            .to_string()
            .contains("exactly match"));
    }

    #[test]
    fn initialize_contract_rejects_unknown_or_duplicate_declarations() {
        let manifest = handshake_manifest();

        let mut result = handshake_result();
        result.event_subscriptions.push("future_event".to_string());
        assert!(validate_initialize_result(&manifest, &result)
            .unwrap_err()
            .to_string()
            .contains("unsupported event"));

        let mut result = handshake_result();
        result.tools.push(result.tools[0].clone());
        assert!(validate_initialize_result(&manifest, &result)
            .unwrap_err()
            .to_string()
            .contains("duplicate remote tool"));

        let mut result = handshake_result();
        result.tools[0].input_schema = Value::Bool(true);
        assert!(validate_initialize_result(&manifest, &result)
            .unwrap_err()
            .to_string()
            .contains("inputSchema must be a JSON object"));
    }

    #[test]
    fn rpc_timeouts_are_finite() {
        assert_eq!(INITIALIZE_TIMEOUT, Duration::from_secs(10));
        assert_eq!(EVENT_TIMEOUT, Duration::from_secs(5));
        assert_eq!(TOOL_CALL_TIMEOUT, Duration::from_secs(120));
        assert_eq!(SHUTDOWN_RPC_TIMEOUT, Duration::from_secs(3));
    }
    use alva_kernel_abi::{ContentBlock, MessageRole};

    // -- aep_event_name: parametric over 7 variants --------------------

    #[test]
    fn aep_name_each_variant_maps_to_spec_wire_name() {
        // CRITICAL asymmetries: AgentStart/AgentEnd carry an "on_"
        // prefix; UserMessage maps to "on_user_message". The
        // parametric loop pins all 7 variants in one pass.
        let args = serde_json::json!({});
        let messages = vec![Message::user("hello")];
        let response = assistant_message("ok");
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
            (
                AepEvent::LlmCallStart {
                    messages: &messages,
                },
                "on_llm_call_start",
            ),
            (
                AepEvent::LlmCallEnd {
                    response: &response,
                },
                "on_llm_call_end",
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
        assert_eq!(v["stateHandle"], serde_json::json!(AEP_STATE_HANDLE));
        assert_eq!(
            v.as_object().expect("must be an object").len(),
            1,
            "AgentStart payload must contain only stateHandle: {v}"
        );

        let v = aep_event_to_params(&AepEvent::AgentEnd { error: Some("oom") }).unwrap();
        assert_eq!(v["stateHandle"], serde_json::json!(AEP_STATE_HANDLE));
        assert_eq!(v["error"], serde_json::json!("oom"));

        let v = aep_event_to_params(&AepEvent::UserMessage { text: "hello" }).unwrap();
        assert_eq!(v["stateHandle"], serde_json::json!(AEP_STATE_HANDLE));
        assert_eq!(v["message"]["text"], serde_json::json!("hello"));
    }

    #[test]
    fn params_llm_call_shapes_include_messages_and_response() {
        let messages = vec![Message::user("hello")];
        let v = aep_event_to_params(&AepEvent::LlmCallStart {
            messages: &messages,
        })
        .unwrap();
        assert_eq!(v["stateHandle"], serde_json::json!(AEP_STATE_HANDLE));
        assert_eq!(v["messages"][0]["role"], serde_json::json!("user"));
        assert_eq!(
            v["messages"][0]["content"][0]["text"],
            serde_json::json!("hello")
        );

        let response = assistant_message("ok");
        let v = aep_event_to_params(&AepEvent::LlmCallEnd {
            response: &response,
        })
        .unwrap();
        assert_eq!(v["stateHandle"], serde_json::json!(AEP_STATE_HANDLE));
        assert_eq!(v["response"]["role"], serde_json::json!("assistant"));
        assert_eq!(v["response"]["content"][0]["text"], serde_json::json!("ok"));
    }

    fn assistant_message(text: &str) -> Message {
        Message {
            id: "assistant-test".to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        }
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
        assert!(
            v.is_object(),
            "BeforeToolCall params must serialize to object: {v}"
        );
        let serialized = v.to_string();
        assert!(
            serialized.contains("tc-1"),
            "tool_call_id must appear: {serialized}"
        );
        assert!(
            serialized.contains("shell"),
            "tool_name must appear: {serialized}"
        );
        assert!(
            serialized.contains("ls"),
            "argument value must appear: {serialized}"
        );
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
        assert_eq!(v["stateHandle"], serde_json::json!(AEP_STATE_HANDLE));
        assert_eq!(v["toolCall"]["id"], serde_json::json!("tc-9"));
        assert_eq!(v["toolCall"]["name"], serde_json::json!("shell"));
        assert!(v.get("result").is_some(), "result key required: {v}");
    }

    // -- action_to_dispatch_result -------------------------------------

    #[test]
    fn action_continue_passes_through_to_dispatch_result_continue() {
        let v = serde_json::json!({"action": "continue"});
        let r = action_to_dispatch_result("p", "on_agent_start", v);
        assert!(matches!(r, AepDispatchResult::Continue));
    }

    #[test]
    fn legal_actions_table_covers_every_aep_event() {
        assert_eq!(
            legal_actions_for_event("before_tool_call"),
            &[
                AepActionKind::Continue,
                AepActionKind::Block,
                AepActionKind::Modify,
                AepActionKind::ReplaceResult,
            ]
        );
        assert_eq!(
            legal_actions_for_event("after_tool_call"),
            &[AepActionKind::Continue, AepActionKind::ModifyResult]
        );
        assert_eq!(
            legal_actions_for_event("on_llm_call_start"),
            &[
                AepActionKind::Continue,
                AepActionKind::ModifyMessages,
                AepActionKind::Block,
            ]
        );
        assert_eq!(
            legal_actions_for_event("on_llm_call_end"),
            &[AepActionKind::Continue, AepActionKind::ModifyResponse]
        );
        assert_eq!(
            legal_actions_for_event("on_user_message"),
            &[AepActionKind::Continue]
        );
        assert_eq!(
            legal_actions_for_event("on_agent_start"),
            &[AepActionKind::Continue, AepActionKind::Block]
        );
        assert_eq!(
            legal_actions_for_event("on_agent_end"),
            &[AepActionKind::Continue]
        );
        assert!(legal_actions_for_event("unknown").is_empty());
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
    fn action_block_is_invalid_for_non_blocking_events() {
        let v = serde_json::json!({"action": "block", "reason": "too late"});
        match action_to_dispatch_result("p", "on_agent_end", v) {
            AepDispatchResult::InvalidAction { reason } => {
                assert!(reason.contains("block"));
                assert!(reason.contains("on_agent_end"));
            }
            other => panic!("expected InvalidAction, got {other:?}"),
        }
    }

    #[test]
    fn action_modify_messages_parses_for_llm_start() {
        let replacement = vec![Message::system("use short answers")];
        let v = serde_json::json!({
            "action": "modify_messages",
            "messages": replacement,
        });
        match action_to_dispatch_result("p", "on_llm_call_start", v) {
            AepDispatchResult::ModifyMessages { messages } => {
                assert_eq!(messages.len(), 1);
                assert_eq!(messages[0].role, MessageRole::System);
                assert_eq!(messages[0].text_content(), "use short answers");
            }
            other => panic!("expected ModifyMessages, got {other:?}"),
        }
    }

    #[test]
    fn action_modify_response_parses_for_llm_end() {
        let v = serde_json::json!({
            "action": "modify_response",
            "response": assistant_message("rewritten"),
        });
        match action_to_dispatch_result("p", "on_llm_call_end", v) {
            AepDispatchResult::ModifyResponse { response } => {
                assert_eq!(response.role, MessageRole::Assistant);
                assert_eq!(response.text_content(), "rewritten");
            }
            other => panic!("expected ModifyResponse, got {other:?}"),
        }
    }

    #[test]
    fn action_modify_result_parses_for_after_tool_call() {
        let v = serde_json::json!({
            "action": "modify_result",
            "result": {
                "content": [{"type": "text", "text": "rewritten tool"}],
                "is_error": false
            },
        });
        match action_to_dispatch_result("p", "after_tool_call", v) {
            AepDispatchResult::ModifyResult { result } => {
                assert!(!result.is_error);
                assert_eq!(result.model_text(), "rewritten tool");
            }
            other => panic!("expected ModifyResult, got {other:?}"),
        }
    }

    #[test]
    fn action_modify_parses_for_before_tool_call() {
        let v = serde_json::json!({
            "action": "modify",
            "modified_arguments": {"command": "echo rewritten"},
        });
        match action_to_dispatch_result("p", "before_tool_call", v) {
            AepDispatchResult::ModifyToolArguments { arguments } => {
                assert_eq!(arguments["command"], serde_json::json!("echo rewritten"));
            }
            other => panic!("expected ModifyToolArguments, got {other:?}"),
        }
    }

    #[test]
    fn action_replace_result_parses_for_before_tool_call() {
        let v = serde_json::json!({
            "action": "replace_result",
            "result": {
                "content": [{"type": "text", "text": "replacement"}],
                "is_error": false
            },
        });
        match action_to_dispatch_result("p", "before_tool_call", v) {
            AepDispatchResult::ReplaceResult { result } => {
                assert!(!result.is_error);
                assert_eq!(result.model_text(), "replacement");
            }
            other => panic!("expected ReplaceResult, got {other:?}"),
        }
    }

    #[test]
    fn action_catch_all_reports_invalid_action_for_unsupported_variants_and_malformed_input() {
        // A plugin that successfully responds with an illegal action should
        // be diagnosable by CLI/debug tooling. The agent loop still treats
        // InvalidAction as non-blocking, but we do not silently erase the
        // protocol error at the parser boundary.
        let modify = serde_json::json!({
            "action": "modify",
            "modified_arguments": {"k": "v"}
        });
        match action_to_dispatch_result("p", "after_tool_call", modify) {
            AepDispatchResult::InvalidAction { reason } => {
                assert!(reason.contains("modify"));
                assert!(reason.contains("after_tool_call"));
            }
            other => panic!("expected InvalidAction, got {other:?}"),
        }

        let malformed = serde_json::json!({"this": "is not an ExtensionAction"});
        match action_to_dispatch_result("p", "after_tool_call", malformed) {
            AepDispatchResult::InvalidAction { reason } => {
                assert!(reason.contains("malformed"));
            }
            other => panic!("expected InvalidAction, got {other:?}"),
        }
    }

    // -- dispatch_outcome: fail-open on transport errors ---------------

    #[test]
    fn dispatch_outcome_ok_passes_action_through() {
        // The Ok arm delegates to action_to_dispatch_result, so Block
        // still propagates when the plugin actually responded.
        let v = serde_json::json!({"action": "block", "reason": "nope"});
        match dispatch_outcome("p", "before_tool_call", Ok(v)) {
            AepDispatchResult::Block { reason } => assert_eq!(reason, "nope"),
            other => panic!("expected Block, got {other:?}"),
        }
    }

    #[test]
    fn dispatch_outcome_fails_open_on_transport_errors() {
        // CRITICAL trust-model pin: a wedged/crashed plugin must NOT
        // block the tool. Every transport-level DispatchError maps to
        // Continue (fail-open). If this ever flips to fail-closed, a
        // single buggy third-party plugin could wedge every tool call.
        use crate::dispatcher::DispatchError;
        use crate::protocol::RpcError;

        // Timeout surfaces as ChannelClosed (see call_dispatcher_blocking).
        assert!(
            matches!(
                dispatch_outcome("p", "before_tool_call", Err(DispatchError::ChannelClosed)),
                AepDispatchResult::Continue
            ),
            "timeout / channel-closed must fail open (Continue)"
        );

        // Plugin returned a JSON-RPC error object.
        assert!(
            matches!(
                dispatch_outcome(
                    "p",
                    "before_tool_call",
                    Err(DispatchError::Rpc(RpcError::new(-32603, "boom"))),
                ),
                AepDispatchResult::Continue
            ),
            "plugin RPC error must fail open (Continue)"
        );
    }
}
